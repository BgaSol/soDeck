import assert from 'node:assert/strict';
import {readFile,writeFile,cp,mkdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {spawnSync} from 'node:child_process';
import {build as bundle} from 'esbuild';
import {build as viteBuild} from 'vite';
import react from '@vitejs/plugin-react';
import {filesHarness,invoke,snapshot} from './files-harness.mjs';
import {rpc,until} from './harness.mjs';
const h=await filesHarness('platform',9721),checks=[];
const source=join(h.output,'fixture-source');await mkdir(source,{recursive:true});await cp('plugins/files/src',join(source,'src'),{recursive:true});await cp('plugins/files/index.html',join(source,'index.html'));
const ui=join(source,'src/view.tsx');let code=await readFile(ui,'utf8');assert.ok(code.includes('await core.call("view.ready")'));code=code.replace('await core.call("view.ready");','if(ctx.snapshot)throw Error("T008_TARGET_READY_FAILED");await core.call("view.ready");');await writeFile(ui,code);
await viteBuild({root:source,configFile:false,base:'./',plugins:[react()],define:{__FILES_VERSION__:'"0.1.9"'},build:{outDir:'dist/ui',emptyOutDir:true}});
await bundle({entryPoints:[resolve('plugins/files/src/worker.ts')],outfile:join(source,'dist/worker/main.js'),bundle:true,format:'iife',platform:'neutral',define:{__FILES_VERSION__:'"0.1.9"'}});
const worker=join(source,'dist/worker/main.js');await writeFile(worker,(await readFile(worker,'utf8'))+`
{const original=globalThis.sodeckPlugin.invoke;let count=0,fail=false;globalThis.sodeckPlugin.invoke=async(command,p)=>{
 if(command==='qa.count')return {count};if(command==='qa.failInit'){fail=p.value;return {set:true};}
 if(command==='command.execute'&&p.commandId==='terminal.new'){count++;return {owner:'sodeck.files'};}
 if(command==='lifecycle.initialize'&&fail)throw Error('T008_DEFAULT_FAILED');
 return original(command,p);
};}
`);
const manifest=JSON.parse(await readFile('plugins/files/manifest.json','utf8'));manifest.version='0.1.9';manifest.contributes.commands.push({id:'terminal.new',title:'同名动作夹具',scope:'server',primary:false});manifest.contributes.workspaceInitializers=[{id:'terminal.initial',commandId:'terminal.new'}];await writeFile(join(source,'dist/manifest.json'),JSON.stringify(manifest));
const archive=join(h.output,'files-platform.sodeck-plugin');const packed=spawnSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','files','--source',join(source,'dist'),'--version','0.1.9','--out',archive],{encoding:'utf8',windowsHide:true});assert.equal(packed.status,0,packed.stdout+packed.stderr);
process.env.SODECK_PLUGIN_PACKAGES=[resolve('.qa/t008/baseline/terminal-0.4.0.sodeck-plugin'),archive].join(delimiter);
try{
  const app=await h.start(),server=await h.create('同名动作验证');await rpc(app.page,'workspaces.open',{serverId:server.id});const f=await h.view('sodeck.files',server.id),t=await h.view('sodeck.terminal',server.id);await until(async()=>(await snapshot(f)).state==='ready',25000);
  await until(async()=>(await rpc(app.page,'workbench.snapshot')).workspaces[0].initializers.every(i=>i.state==='finished'));
  assert.equal((await invoke(f,'qa.count')).count,1);assert.equal((await rpc(app.page,'resources.list')).ptys.length,1);
  const initializers=(await rpc(app.page,'workbench.snapshot')).workspaces[0].initializers;assert.equal(new Set(initializers.map(i=>i.id)).size,2);
  await assert.rejects(rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()}),e=>e.code==='AMBIGUOUS_COMMAND');
  const owned=await rpc(app.page,'commands.execute',{pluginId:'sodeck.files',commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()});assert.equal(owned.owner,'sodeck.files');await rpc(app.page,'workspaces.open',{serverId:server.id});assert.equal((await invoke(f,'qa.count')).count,2);checks.push('MP-01: identical local command and initializer IDs stay isolated by actual plugin owner; unqualified collision is rejected; each initializer runs once');
  const pty=(await rpc(app.page,'resources.list')).ptys[0].id;
  await assert.rejects(rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()}),e=>['MOVE_TIMEOUT','MOVE_CANCELLED'].includes(e.code));
  assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces[0].ownerWindowId,'main');assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,pty);
  await t.locator('.xterm-helper-textarea').pressSequentially("printf 'TARGET-ROLLBACK-OK\\n'\r");await until(async()=>(await t.locator('.xterm-rows').innerText()).includes('TARGET-ROLLBACK-OK'));checks.push('MP-03: file target surface failure rolls the complete workspace back and keeps the original PTY usable');
  await invoke(f,'qa.failInit',{value:true});const other=await h.create('默认文件失败');await rpc(app.page,'workspaces.open',{serverId:other.id});await h.view('sodeck.terminal',other.id);await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===2,25000);checks.push('MP-02: file default-view initialization failure does not prevent the other plugin from creating and connecting its first terminal');
}finally{await writeFile(join(h.output,'results.json'),JSON.stringify(checks,null,2));await h.stop();}
