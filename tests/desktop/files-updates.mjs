import assert from 'node:assert/strict';
import {readFile,writeFile,cp,mkdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync,execFileSync} from 'node:child_process';
import {build} from 'esbuild';
import {filesHarness,invoke,snapshot} from './files-harness.mjs';
import {rpc,until,pluginRpc} from './harness.mjs';
const h=await filesHarness('updates',9691),checks=[];
const digest=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const core=resolve('target/debug/sodeck-desktop.exe'),terminal=resolve('.qa/t008/baseline/terminal-0.4.0.sodeck-plugin');
const baseline={core:await digest(core),terminal:await digest(terminal)};
const pack=args=>{const r=spawnSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','files',...args],{encoding:'utf8',windowsHide:true});assert.equal(r.status,0,r.stderr+r.stdout);};
const source=join(h.output,'update-source');await cp('plugins/files/dist',source,{recursive:true});
// Candidate is built from the actual file plugin, with a visible parser rule
// change confined to the test package. Production 0.1.0 stays byte-for-byte.
await build({entryPoints:[resolve('plugins/files/src/worker.ts')],outfile:join(source,'worker/main.js'),bundle:true,format:'iife',platform:'neutral',define:{__FILES_VERSION__:'"0.1.1"'},plugins:[{name:'update-rule',setup(b){b.onLoad({filter:/mounts\.ts$/},async args=>({loader:'ts',contents:(await readFile(args.path,'utf8')).replace(/["']本地文件系统["']/,JSON.stringify("本地文件系统（更新验证）"))}));}}]});
const updated=join(h.output,'files-0.1.1.sodeck-plugin');pack(['--source',source,'--version','0.1.1','--out',updated]);
const badSource=join(h.output,'bad-source');await cp(source,badSource,{recursive:true});
await writeFile(join(badSource,'worker/main.js'),await readFile(join(badSource,'worker/main.js'),'utf8')+'\nglobalThis.sodeckPlugin.prepare=async()=>{await sodeck.call("storage.set",{key:"preferences",value:{showHidden:false}});throw Error("T008_UPDATE_FAILED");};');
const bad=join(h.output,'files-0.1.2-bad.sodeck-plugin');pack(['--source',badSource,'--version','0.1.2','--out',bad]);
const data=join(h.output,'data-'+Date.now());let app,server;
const start=async()=>{app=await h.start({data,allowStartupWarning:true});const w=await rpc(app.page,'workbench.snapshot');assert.equal(w.views.length,0);assert.equal(w.workspaces.length,0);return app;};
const status=async()=>(await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files');
const open=async()=>{await rpc(app.page,'workspaces.open',{serverId:server.id});const f=await h.view('sodeck.files',server.id),t=await h.view('sodeck.terminal',server.id);await until(async()=>{const s=await snapshot(f);return s.state==='ready'&&s.details?.capacity;},25000);return {f,t};};
try{
  await start();server=await h.create('文件独立更新');let {f,t}=await open();
  const pty=(await rpc(app.page,'resources.list')).ptys[0].id;
  await rpc(app.page,'plugins.prepareUpdate',{path:updated});assert.equal((await status()).activeVersion,'0.1.0');assert.equal((await status()).pendingVersion,'0.1.1');assert.equal((await snapshot(f)).version,'0.1.0');assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,pty);
  await h.stop();await start();assert.equal((await status()).activeVersion,'0.1.1');({f,t}=await open());
  let s=await snapshot(f);assert.equal(s.version,'0.1.1');assert.match(s.details.mounts[0].kind,/更新验证/);await f.screenshot({path:join(h.output,'updated-files.png')});checks.push('MP-04: file 0.1.1 UI and parsing change activate only on restart; original core and terminal remain unchanged; startup stays empty');
  await rpc(app.page,'plugins.prepareUpdate',{path:bad});await h.stop();await start();assert.equal((await status()).activeVersion,'0.1.1');assert.match((await status()).error,/T008_UPDATE_FAILED/);({f,t}=await open());assert.equal((await snapshot(f)).showHidden,true);checks.push('MP-04: failed file candidate falls back to 0.1.1 and rolls back candidate preference writes without disabling terminal');
  const before=(await rpc(app.page,'resources.list')).ptys[0].id;
  const instances=(await rpc(app.page,'diagnostics.get')).pluginInstances;
  const victim=instances.find(p=>p.pluginId==='sodeck.files');assert.ok(victim?.processId);
  execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${Number(victim.processId)} -ErrorAction Stop`],{windowsHide:true});
  await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.some(v=>v.pluginId==='sodeck.files'&&v.status==='viewFailed'));
  assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,before);
  await t.locator('.xterm-helper-textarea').pressSequentially("printf 'FILES-CRASH-PTY-OK\\n'\r");await until(async()=>(await t.locator('.xterm-rows').innerText()).includes('FILES-CRASH-PTY-OK'));
  const failed=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.files');await rpc(app.page,'views.recover',{viewId:failed.viewId});f=await h.view('sodeck.files',server.id);await until(async()=>(await snapshot(f)).state==='ready',25000);
  checks.push('MP-02: killing the exact file worker revokes only its resources; terminal PTY survives and file view reloads with a fresh instance');
  // A file surface cannot impersonate the terminal owner or obtain its view.
  const termView=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal');
  await assert.rejects(pluginRpc(f,'views.presentation',{viewId:termView.viewId}),e=>e.code==='SOURCE_INVALID');
  await assert.rejects(pluginRpc(f,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()}),e=>e.code==='SOURCE_INVALID');checks.push('MP-01: true view source rejects cross-plugin presentation and command impersonation');
  assert.deepEqual({core:await digest(core),terminal:await digest(terminal)},baseline);
}finally{await writeFile(join(h.output,'results.json'),JSON.stringify({checks,baseline},null,2));await h.stop();}
