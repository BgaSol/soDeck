import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {filesHarness,snapshot,locate,invoke} from './files-harness.mjs';
import {rpc,until,pluginRpc} from './harness.mjs';
const h=await filesHarness('degraded',9701),checks=[];
try{
  const app=await h.start(),server=await h.create('迁移失败验证');await rpc(app.page,'workspaces.open',{serverId:server.id});
  const f=await h.view('sodeck.files',server.id),t=await h.view('sodeck.terminal',server.id);
  await until(async()=>(await snapshot(f)).state==='ready',25000);await locate(f,'/srv/sodeck-t008/ordinary');
  const before=(await rpc(app.page,'resources.list')).ptys[0].id;
  // Hold the second plugin's prepare callback after the first source is ready.
  await t.evaluate(()=>{const original=window.__sodeckNotify;window.__qaOriginalNotify=original;window.__sodeckNotify=n=>{if(n.type==='view.prepareMove'){window.__qaHeldMove=n;return;}original(n);};});
  const operationId=crypto.randomUUID();const moving=rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId}).then(v=>({value:v}),e=>({error:e}));
  await t.waitForFunction(()=>Boolean(window.__qaHeldMove));await rpc(app.page,'workbench.operation.cancel',{operationId});const canceled=await moving;assert.equal(canceled.error?.code,'CANCELED');
  await t.evaluate(()=>{window.__sodeckNotify=window.__qaOriginalNotify;});
  assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces[0].ownerWindowId,'main');assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,before);assert.equal((await snapshot(f)).path,'/srv/sodeck-t008/ordinary');
  await t.locator('.xterm-helper-textarea').pressSequentially("printf 'ROLLBACK-PTY-OK\\n'\r");await until(async()=>(await t.locator('.xterm-rows').innerText()).includes('ROLLBACK-PTY-OK'));checks.push('MP-03: cancellation while the second plugin prepares rolls both sources back, preserving file selection and original PTY');
  const termView=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal');await rpc(app.page,'views.close',{viewId:termView.viewId});
  await locate(f,'/srv/sodeck-t008/ordinary/zero');assert.equal((await snapshot(f)).details.attributes.size,'0');checks.push('FT-13: file browsing continues after the final terminal closes');
  await rpc(app.page,'workspaces.close',{workspaceId:server.id});
  const jail=await h.create('受限 SFTP','jailed');await rpc(app.page,'workspaces.open',{serverId:jail.id});const jailed=await h.view('sodeck.files',jail.id);
  await until(async()=>{const s=await snapshot(jailed);return s.state==='ready'&&s.mountError;},25000);let s=await locate(jailed,'/data/only-inside');assert.equal(s.details.attributes.type,'file');assert.ok(s.mountError);assert.equal(s.details.mounts.length,0);await jailed.screenshot({path:join(h.output,'chroot-degraded.png')});checks.push('FT-05: real chroot/forced internal-SFTP keeps directory browsing and marks unprovable mount information unavailable');
  await rpc(app.page,'workspaces.close',{workspaceId:jail.id});
  // A file view created while terminal authentication is canceled must not
  // silently restart authentication; explicit file connect remains available.
  const canceledServer=await rpc(app.page,'servers.create',{name:'取消认证',host:h.config.host,port:h.config.port,username:h.config.username,authType:'key'});await rpc(app.page,'workspaces.open',{serverId:canceledServer.id});
  await app.page.getByRole('button',{name:'取消',exact:true}).click();const idle=await h.view('sodeck.files',canceledServer.id);await until(async()=>(await snapshot(idle)).state==='idle');
  assert.equal((await rpc(app.page,'resources.list')).connections.length,0);await rpc(app.page,'auth.ssh.set',{serverId:canceledServer.id,privateKeyPath:h.config.privateKeyPath});
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.files',commandId:'files.connect',workspaceId:canceledServer.id,operationId:crypto.randomUUID()});await until(async()=>(await snapshot(idle)).state==='ready',25000);checks.push('FT-14/RI-07: canceled terminal authentication stays canceled; explicit file command can connect independently');
}finally{await writeFile(join(h.output,'results.json'),JSON.stringify(checks,null,2));await h.stop();}
