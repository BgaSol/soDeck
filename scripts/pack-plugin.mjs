import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign } from 'node:crypto';
import { mkdir, readFile, readdir, lstat, writeFile, copyFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { resolve, dirname, relative, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  if (!process.argv[i].startsWith('--') || !process.argv[i + 1]) throw new Error('Options require values');
  args.set(process.argv[i], process.argv[i + 1]);
}
const pluginName = args.get('--plugin') ?? 'terminal';
if (!/^[a-z0-9-]+$/.test(pluginName)) throw new Error('Invalid plugin name');
const pluginDir = join(root, 'plugins', pluginName);
const version = args.get('--version') ?? '0.3.0';
const source = resolve(args.get('--source') ?? join(pluginDir, 'dist'));
if (!args.has('--source')) {
  const vite = spawnSync(process.execPath, [join(root, 'node_modules/vite/bin/vite.js'), 'build'], {
    cwd: pluginDir, stdio: 'inherit', windowsHide: true,
    env: { ...process.env, TERMINAL_PLUGIN_VERSION: version },
  });
  if (vite.status !== 0) throw new Error('Plugin UI build failed');
  const { build } = await import('esbuild');
  await build({ entryPoints: [join(pluginDir, 'src/worker.ts')], outfile: join(source, 'worker/main.js'),
    bundle: true, format: 'iife', platform: 'neutral', target: 'es2022',
    define: { __TERMINAL_VERSION__: JSON.stringify(version) }, minify: false,
  });
  await copyFile(join(pluginDir, 'manifest.json'), join(source, 'manifest.json'));
}
const manifest = JSON.parse(await readFile(join(source, 'manifest.json'), 'utf8'));
if (args.has('--version') || !args.has('--source')) manifest.version = version;
if (!/^\d+\.\d+\.\d+(?:[-+][a-zA-Z0-9.-]+)?$/.test(manifest.version)) throw new Error('Invalid plugin version');
const entries = new Map();
const walk = async directory => {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name), name = relative(source, path).replaceAll('\\', '/');
    const stat = await lstat(path);
    if (stat.isSymbolicLink()) throw new Error(`Package cannot contain links: ${name}`);
    if (stat.isDirectory()) await walk(path);
    else if (stat.isFile() && !['integrity.json', 'signature.json'].includes(name)) {
      if (name.includes(':') || name.split('/').some(part=>!part || part==='.' || part==='..')) throw new Error('Invalid archive path');
      entries.set(name, name === 'manifest.json' ? Buffer.from(JSON.stringify(manifest, null, 2) + '\n') : await readFile(path));
    }
  }
};
await walk(source);
if (entries.size + 2 > 2000 || [...entries.values()].reduce((n,b)=>n+b.length,0) > 128 * 1024 * 1024) throw new Error('Package exceeds expanded limits');
const hashes = Object.fromEntries([...entries].sort(([a],[b])=>a.localeCompare(b, 'en')).map(([name,bytes])=>[name,createHash('sha256').update(bytes).digest('hex')]));
const integrity = Buffer.from(JSON.stringify(hashes));
const keyId = args.get('--key-id') ?? 'sodeck-local-development';
let keyPath = args.get('--key');
if (!keyPath) {
  keyPath = join(root, '.local/signing/development-private.pem');
  if (!existsSync(keyPath)) {
    await mkdir(dirname(keyPath), { recursive: true });
    const { privateKey } = generateKeyPairSync('ed25519');
    await writeFile(keyPath, privateKey.export({ type: 'pkcs8', format: 'pem' }), { flag: 'wx', mode: 0o600 });
  }
}
const privateKey = createPrivateKey(await readFile(keyPath));
if (privateKey.asymmetricKeyType !== 'ed25519') throw new Error('Signing key must be Ed25519');
const publicKey = createPublicKey(privateKey).export({ format: 'jwk' });
const rawPublic = Buffer.from(publicKey.x, 'base64url');
const signature = sign(null, integrity, privateKey);
entries.set('integrity.json', integrity);
entries.set('signature.json', Buffer.from(JSON.stringify({ keyId, signature: signature.toString('base64') })));
const crcTable = Array.from({length:256},(_,i)=>{for(let n=0;n<8;n++) i=i&1?(i>>>1)^0xedb88320:i>>>1;return i>>>0;});
const crc32 = bytes => {let crc=0xffffffff;for(const byte of bytes)crc=crcTable[(crc^byte)&255]^(crc>>>8);return (crc^0xffffffff)>>>0;};
const local = [], central = []; let offset = 0;
for (const [name, bytes] of [...entries].sort(([a],[b])=>a.localeCompare(b,'en'))) {
  const encoded = Buffer.from(name), crc = crc32(bytes);
  const header = Buffer.alloc(30); header.writeUInt32LE(0x04034b50); header.writeUInt16LE(20,4); header.writeUInt16LE(0x800,6);
  header.writeUInt32LE(crc,14); header.writeUInt32LE(bytes.length,18); header.writeUInt32LE(bytes.length,22); header.writeUInt16LE(encoded.length,26);
  local.push(header,encoded,bytes);
  const directory = Buffer.alloc(46); directory.writeUInt32LE(0x02014b50); directory.writeUInt16LE(20,4); directory.writeUInt16LE(20,6); directory.writeUInt16LE(0x800,8);
  directory.writeUInt32LE(crc,16); directory.writeUInt32LE(bytes.length,20); directory.writeUInt32LE(bytes.length,24); directory.writeUInt16LE(encoded.length,28); directory.writeUInt32LE(offset,42);
  central.push(directory,encoded); offset += header.length + encoded.length + bytes.length;
}
const directory = Buffer.concat(central), end = Buffer.alloc(22);
end.writeUInt32LE(0x06054b50); end.writeUInt16LE(entries.size,8); end.writeUInt16LE(entries.size,10); end.writeUInt32LE(directory.length,12); end.writeUInt32LE(offset,16);
const archive = Buffer.concat([...local,directory,end]);
if (archive.length > 32 * 1024 * 1024) throw new Error('Package exceeds 32 MiB compressed limit');
const output = resolve(args.get('--out') ?? join(root, 'resources', `${manifest.pluginId}-${manifest.version}.sodeck-plugin`));
await mkdir(dirname(output),{recursive:true}); await writeFile(output,archive);
// Keep the baseline factory package separate from the independently built update.
if (!args.has('--out') && manifest.version === '0.3.0') await writeFile(join(root, 'resources', `${pluginName}.sodeck-plugin`), archive);
if (!args.has('--key')) {
  const publicPath = join(root, 'resources/development-public-key.json');
  await mkdir(dirname(publicPath),{recursive:true});
  const publicText = JSON.stringify({ keyId, publicKey:rawPublic.toString('base64'), developmentOnly:true },null,2)+'\n';
  // Native instances may read this trust file while an independent plugin build
  // runs. Avoid truncating an unchanged public key during that read.
  if (!existsSync(publicPath) || await readFile(publicPath, 'utf8') !== publicText) await writeFile(publicPath, publicText);
}
console.log(JSON.stringify({package:output,pluginId:manifest.pluginId,version:manifest.version,sha256:createHash('sha256').update(archive).digest('hex'),keyId}));
