import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { mkdir, copyFile, writeFile, unlink } from 'node:fs/promises';
import { resolve, join, delimiter } from 'node:path';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { launch, rpc, pluginRpc, until, stop } from './harness.mjs';

// Explicit opt-in: reuse only one authorized server and its encrypted credential
// in isolated data. Never open the user's live database for writes.
assert.ok(process.env.SODECK_MOUNT_SOURCE_DATA, 'Set SODECK_MOUNT_SOURCE_DATA');
assert.ok(process.env.SODECK_MOUNT_TEST_HOST, 'Set SODECK_MOUNT_TEST_HOST');
const source = resolve(process.env.SODECK_MOUNT_SOURCE_DATA);
const output = resolve('.qa/files-mount-roots', String(Date.now()));
const data = join(output, 'data');
await mkdir(join(data, 'core/secrets'), { recursive: true });
const original = new DatabaseSync(join(source, 'core/core.sqlite'), { readOnly: true });
let server;
try {
  const candidates = original.prepare('SELECT data FROM servers').all()
    .map(row => JSON.parse(row.data))
    .filter(s => s.host === process.env.SODECK_MOUNT_TEST_HOST);
  assert.equal(candidates.length, 1, 'Expected one authorized server');
  server = candidates[0];
  const copy = new DatabaseSync(join(data, 'core/core.sqlite'));
  try {
    copy.exec('CREATE TABLE servers(id TEXT PRIMARY KEY, data TEXT NOT NULL); CREATE TABLE host_keys(endpoint TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, algorithm TEXT NOT NULL);');
    copy.prepare('INSERT INTO servers VALUES (?,?)').run(server.id, JSON.stringify(server));
    const key = original.prepare('SELECT * FROM host_keys WHERE endpoint=?').get(`${server.host}:${server.port}`);
    assert.ok(key, 'Previously trusted host key required');
    copy.prepare('INSERT INTO host_keys VALUES (?,?,?)').run(key.endpoint, key.fingerprint, key.algorithm);
  } finally { copy.close(); }
} finally { original.close(); }
assert.match(server.id, /^[0-9a-f-]{36}$/i);
await copyFile(join(source, 'core/secrets', server.id + '.dpapi'), join(data, 'core/secrets', server.id + '.dpapi'));

const oldFiles = resolve('.local/dev/SoDeck-v2-T011/resources/files.sodeck-plugin');
const terminal = resolve('.local/dev/SoDeck-v2-T011/resources/terminal.sodeck-plugin');
process.env.SODECK_PLUGIN_PACKAGES = [terminal, oldFiles].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS = JSON.stringify({'sodeck.files': 9862, 'sodeck.terminal': 9863});
const bundle = await build({entryPoints:['packages/sdk/src/remote-client.ts'],bundle:true,write:false,format:'esm',platform:'node'});
const { Remote } = await import('data:text/javascript;base64,' + Buffer.from(bundle.outputFiles[0].text).toString('base64'));
const results = { output, checks: [] };
let app, browser;
const invoke = (command, args = {}) => pluginRpc(browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')), 'plugin.invoke', {command,args});
async function start() {
  app = await launch({data,shellPort:9861,pluginPort:9862,executable:resolve('.local/dev/SoDeck-v2-T011/sodeck-desktop.exe'),packagePath:terminal});
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  browser = await until(()=>chromium.connectOverCDP('http://127.0.0.1:9862'));
  return until(async()=>{const s=await invoke('files.snapshot');return s.state==='ready'&&s.details&&!s.details.loading?s:false;},30000);
}
async function close() {
  await stop(app); app=undefined;
  await browser?.close().catch(()=>{}); browser=undefined;
}
try {
  const before = await start();
  assert.equal(before.mountError, '挂载路径不是绝对路径');
  results.checks.push('Original 0.3.0 reproduces the reported mount-path error on real Linux');
  const workspace=(await rpc(app.page,'workbench.snapshot')).workspaces.find(w=>w.workspaceId===server.id);
  const remote = new Remote({call:(method,p)=>rpc(app.page,method,p)}, {workspaceId:server.id,workspaceGeneration:workspace.generation});
  remote.lease = await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});
  let raw;
  try { raw=await remote.readFile({bytesBase64:Buffer.from('/proc/self/mountinfo').toString('base64')}); }
  finally { await remote.dispose(); }
  results.specialRoots=Buffer.from(raw).toString('utf8').trim().split('\n').map(line=>{
    const [left,right]=line.split(' - '),fields=left.split(' ');
    return {root:fields[3],target:fields[4],fsType:right.split(' ')[0]};
  }).filter(m=>!m.root.startsWith('/'));
  assert.ok(results.specialRoots.some(m=>m.fsType==='nsfs'));
  // Keep only the fields needed for diagnosis; mount options may contain secrets.
  await writeFile(join(output,'diagnosis.json'),JSON.stringify(results,null,2));
  await rpc(app.page,'plugins.prepareUpdate',{path:resolve('resources/files.sodeck-plugin')});
  const staged=(await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files');
  assert.equal(staged.activeVersion,'0.3.0'); assert.equal(staged.pendingVersion,'0.3.1');
  await close();
  await start();
  assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files').activeVersion,'0.3.1');
  await invoke('follow.setEnabled',{enabled:false});
  for(const target of ['/', '/proc', results.specialRoots.find(m=>m.fsType==='nsfs').target]) {
    await invoke('files.locate',{text:target});
    const s=await until(async()=>{const s=await invoke('files.snapshot');return s.path===target&&s.details&&!s.details.loading?s:false;},30000);
    assert.ok(!s.mountError, s.mountError); assert.equal(s.details.exact,true);
    assert.ok(s.details.mounts.length);
    if(target==='/')assert.ok(s.details.capacity,s.details.capacityError);
    if(target===results.specialRoots.find(m=>m.fsType==='nsfs').target) {
      assert.equal(s.details.mounts[0].kind,'虚拟文件系统');
      assert.equal(Buffer.from(s.details.mounts[0].root.bytesBase64,'base64').toString(),results.specialRoots.find(m=>m.fsType==='nsfs').root);
    }
    results.checks.push(`0.3.1: ${target} has verified mount details`);
  }
  await invoke('files.locate',{text:'/'});
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin'));
  await page.getByRole('button',{name:'刷新目录与挂载'}).click();
  await until(async()=>{const s=await invoke('files.snapshot');return !s.loading&&s.details?.capacity&&!s.mountError;},30000);
  await page.screenshot({path:join(output,'root-fixed.png')});
  await rpc(app.page,'workspaces.close',{workspaceId:server.id});
  await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.leases.length&&!r.connections.length&&!r.ptys.length;});
  results.checks.push('Refresh preserves valid root capacity; closing workspace releases all leases, connections and PTYs');
  assert.deepEqual(app.errors,[]);
} finally {
  await close();
  for (const file of [
    join(data,'core/secrets',server.id+'.dpapi'),
    ...['core.sqlite','core.sqlite-wal','core.sqlite-shm'].map(name=>join(data,'core',name)),
  ]) await unlink(file).catch(error=>{if(error.code!=='ENOENT')throw error;});
  results.copiedCredentialAndDatabaseRemoved = true;
  await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));
  console.log(JSON.stringify({output,checks:results.checks,specialRoots:results.specialRoots}));
}
