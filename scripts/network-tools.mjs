// Test-only curl. Production builds never call this installer.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

export const curlVersion = '8.21.0';
const archiveSha256 = 'aa1b66a70eace83dc624508745646c08ae561de512ab403adffb93ac87fc72e6';
function version(command) {
  const result = spawnSync(command, ['--version'], { encoding: 'utf8', windowsHide: true });
  return result.status === 0 && result.stdout.startsWith(`curl ${curlVersion} `);
}
function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status})`);
}
export async function testCurl() {
  const supplied = process.env.SODECK_TEST_CURL;
  const system = process.platform === 'win32' ? 'curl.exe' : 'curl';
  if (version(supplied || system)) return supplied || system;
  if (supplied) throw new Error(`SODECK_TEST_CURL must be curl ${curlVersion}`);
  const root = path.resolve('artifacts/network-tools', `${process.platform}-${os.arch()}`);
  const build = path.join(root, 'build');
  const executable = path.join(build, 'src', ...(process.platform === 'win32' ? ['Release', 'curl.exe'] : ['curl']));
  if (version(executable)) return executable;
  fs.mkdirSync(root, { recursive: true });
  const response = await fetch(`https://curl.se/download/curl-${curlVersion}.tar.xz`);
  if (!response.ok) throw new Error(`curl download failed: ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (createHash('sha256').update(bytes).digest('hex') !== archiveSha256) throw new Error('curl source checksum mismatch');
  const archive = path.join(root, 'curl.tar.xz');
  fs.writeFileSync(archive, bytes);
  const tar = process.platform === 'win32' ? path.join(process.env.SystemRoot, 'System32/tar.exe') : 'tar';
  run(tar, ['-xf', archive, '-C', root]);
  const options = ['-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF', '-DBUILD_STATIC_CURL=ON',
    '-DBUILD_TESTING=OFF', '-DHTTP_ONLY=ON', '-DCURL_ENABLE_SSL=OFF', '-DCURL_USE_LIBPSL=OFF',
    '-DCURL_ZLIB=OFF', '-DCURL_BROTLI=OFF', '-DCURL_ZSTD=OFF', '-DCURL_USE_LIBSSH2=OFF'];
  if (process.platform === 'win32') options.push('-A', os.arch() === 'arm64' ? 'ARM64' : 'x64');
  run('cmake', ['-S', path.join(root, `curl-${curlVersion}`), '-B', build, ...options]);
  run('cmake', ['--build', build, '--config', 'Release', '--parallel', '4']);
  if (!version(executable)) throw new Error('built curl version mismatch');
  return executable;
}
