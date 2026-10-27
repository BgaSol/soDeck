import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {mkdir,copyFile,writeFile,unlink} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {build} from 'esbuild';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';

assert.ok(process.env.SODECK_MOUNT_SOURCE_DATA,'Explicit source data required');
assert.ok(process.env.SODECK_OPENWRT_HOST,'Explicit OpenWrt host required');
assert.ok(process.env.SODECK_GNU_HOST,'Explicit GNU/Linux host required');
const source=resolve(process.env.SODECK_MOUNT_SOURCE_DATA);
const output=resolve('.qa/openwrt-mounts',String(Date.now())),data=join(output,'data');
const base=resolve('.local/dev/SoDeck-v2-T011');
const terminal=join(base,'resources/terminal.sodeck-plugin');
const oldFiles=join(base,'updates/files-0.3.1.sodeck-plugin');
process.env.SODECK_PLUGIN_PACKAGES=[terminal,oldFiles].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9892,'sodeck.terminal':9893});
const servers=[],results={output,checks:[],mounts:[]};
let app,browser;
const bundle=await build({entryPoints:['packages/sdk/src/remote-client.ts'],bundle:true,write:false,format:'esm',platform:'node'});
const {Remote}=await import('data:text/javascript;base64,'+Buffer.from(bundle.outputFiles[0].text).toString('base64'));
async function start() {
  app=await launch({data,shellPort:9891,pluginPort:9892,executable:join(base,'sodeck-desktop.exe'),packagePath:terminal});
}
async function close() {await stop(app);app=undefined;await browser?.close().catch(()=>{});browser=undefined;}
async function view(server) {
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9892'));
  return until(async()=>{
    for(const p of browser.contexts().flatMap(c=>c.pages()))
      if(!p.isClosed()&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===server.id)return p;
  });
}
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const settled=page=>until(async()=>{const s=await invoke(page,'files.snapshot');return s.state==='ready'&&!s.loading&&s.details&&!s.details.loading?s:false;},30000);
async function systemCapacity(server,mode) {
  const workspace=(await rpc(app.page,'workbench.snapshot')).workspaces.find(w=>w.workspaceId===server.id);
  const r=new Remote({call:(m,p)=>rpc(app.page,m,p)},{workspaceId:server.id,workspaceGeneration:workspace.generation});
  r.lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});
  try {
    const raw=Buffer.from(await r.exec(mode==='df'?"LC_ALL=C df -Pk -- /":"LC_ALL=C stat -f -c '%S %b %f %a' -- /")).toString();
    if(mode==='df') {
      const row=raw.trim().split('\n')[1].trim().split(/\s+/);
      return String(BigInt(row[1])*1024n);
    }
    const [unit,total]=raw.trim().split(/\s+/).map(BigInt);return String(unit*total);
  } finally {await r.dispose();}
}
try {
  await mkdir(join(data,'core/secrets'),{recursive:true});
  const original=new DatabaseSync(join(source,'core/core.sqlite'),{readOnly:true});
  const dest=new DatabaseSync(join(data,'core/core.sqlite'));
  try {
    dest.exec('CREATE TABLE servers(id TEXT PRIMARY KEY, data TEXT NOT NULL); CREATE TABLE host_keys(endpoint TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, algorithm TEXT NOT NULL);');
    for(const host of [process.env.SODECK_OPENWRT_HOST,process.env.SODECK_GNU_HOST]) {
      const matches=original.prepare('SELECT data FROM servers').all().map(row=>JSON.parse(row.data)).filter(s=>s.host===host);
      assert.equal(matches.length,1);const server=matches[0];assert.match(server.id,/^[0-9a-f-]{36}$/i);
      const key=original.prepare('SELECT * FROM host_keys WHERE endpoint=?').get(`${server.host}:${server.port}`);assert.ok(key);
      dest.prepare('INSERT INTO servers VALUES (?,?)').run(server.id,JSON.stringify(server));
      dest.prepare('INSERT INTO host_keys VALUES (?,?,?)').run(key.endpoint,key.fingerprint,key.algorithm);
      servers.push(server);
      await copyFile(join(source,'core/secrets',server.id+'.dpapi'),join(data,'core/secrets',server.id+'.dpapi'));
    }
  } finally {original.close();dest.close();}
  await start();
  let page=await view(servers[0]);
  const before=await settled(page);
  assert.match(before.mountError,/只读采集能力/);
  results.checks.push('0.3.1 reproduces missing-stat mount failure on OpenWrt');
  await rpc(app.page,'plugins.prepareUpdate',{path:resolve('resources/files.sodeck-plugin')});
  const staged=(await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files');
  assert.equal(staged.activeVersion,'0.3.1');assert.equal(staged.pendingVersion,'0.3.2');
  await close();await start();
  assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files').activeVersion,'0.3.2');
  for(let index=0;index<servers.length;index++) {
    const server=servers[index],mode=index===0?'df':'stat';
    page=await view(server);await settled(page);await invoke(page,'follow.setEnabled',{enabled:false});
    for(const target of ['/','/proc','/tmp']) {
      await invoke(page,'files.locate',{text:target});
      const s=await settled(page);assert.equal(s.path,target);assert.ok(!s.mountError,s.mountError);
      assert.ok(s.details.mounts.length);const mount=s.details.mounts[0];
      if(target==='/') {
        assert.ok(s.details.capacity,s.details.capacityError);
        assert.equal(s.details.capacity.source,mode);
        assert.equal(s.details.capacity.total,await systemCapacity(server,mode));
        if(mode==='df')assert.equal(s.details.capacity.free,null);
        else assert.ok(s.details.capacity.free!==null);
      }
      results.mounts.push({mode,path:target,source:mount.source,type:mount.fsType,capacity:s.details.capacity,capacityError:s.details.capacityError});
    }
    await invoke(page,'files.locate',{text:'/'});await settled(page);
    await page.getByRole('button',{name:'刷新目录与挂载'}).click();
    const refreshed=await settled(page);assert.ok(!refreshed.mountError,refreshed.mountError);assert.ok(refreshed.details.capacity);
    await page.screenshot({path:join(output,mode+'-root.png')});
    if(mode==='df')await page.getByText('容量按 1 KiB 精度提供；部分字段不可用。').scrollIntoViewIfNeeded();
    else await page.getByText('账号可用',{exact:true}).scrollIntoViewIfNeeded();
    await page.screenshot({path:join(output,mode+'-capacity.png')});
    await rpc(app.page,'workspaces.close',{workspaceId:server.id});
    await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.leases.length&&!r.connections.length&&!r.ptys.length;});
    await browser.close().catch(()=>{});browser=undefined;
    results.checks.push(`${mode}: real mount verification, root capacity matches system output, refresh and all resources released`);
  }
  assert.deepEqual(app.errors,[]);
} catch(error) {
  results.error=String(error);
  throw error;
} finally {
  await close();
  for(const file of [...servers.map(s=>join(data,'core/secrets',s.id+'.dpapi')),...['core.sqlite','core.sqlite-wal','core.sqlite-shm'].map(n=>join(data,'core',n))])
    await unlink(file).catch(e=>{if(e.code!=='ENOENT')throw e;});
  results.copiedCredentialAndDatabaseRemoved=true;
  await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));console.log(JSON.stringify(results));
}
