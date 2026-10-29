import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {launch,rpc,pluginRpc,pluginPages,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t023/terminal');await mkdir(output,{recursive:true});
const pageId='plugin:sodeck.terminal:general',checks=[];let app;
const note=text=>{checks.push(text);console.log(text);};
async function change(changes){const {snapshot:s}=await rpc(app.page,'settings.page.get',{pageId});return rpc(app.page,'settings.commit',{pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,operationId:crypto.randomUUID(),changes});}
async function terminal(viewId){return until(async()=>{for(const p of await pluginPages(app))if(!p.isClosed()&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===viewId)return p;});}
async function settingSize(page,size){await until(()=>page.locator('.terminal-status').innerText().then(t=>t.includes(`字号 ${size}`)));}
async function shellMarker(page,tag){await until(()=>page.locator('.xterm-rows').innerText().then(t=>t.includes('tester@')),30000);await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type(`printf '%s=%s\\n' '${tag}' "\${PROMPT_COMMAND:-none}"`);await page.keyboard.press('Enter');let text;try{text=await until(async()=>{const t=await page.locator('.xterm-rows').innerText();return t.includes(`${tag}=`)?t:false;},30000);}finally{await writeFile(join(output,tag+'.txt'),await page.locator('.xterm-rows').innerText());}return text;}
try{
  app=await launch({data:join(output,'data-'+Date.now()),shellPort:9851,pluginPort:9852});
  const server=await rpc(app.page,'servers.create',{name:'T023 isolated Bash',host:'127.0.0.1',port:22341,username:'tester',authType:'key'});
  await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});
  try{const l=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:l.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  let snapshot=await until(async()=>{const w=await rpc(app.page,'workbench.snapshot');return w.views.some(v=>v.pluginId==='sodeck.terminal')?w:false;});
  const first=snapshot.views.find(v=>v.pluginId==='sodeck.terminal');let a=await terminal(first.viewId);await until(async()=>(await pluginRpc(a,'view.initialize')).session.status==='running',30000);
  await until(async()=>!(await rpc(app.page,'workbench.snapshot')).pendingOpens.length);await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()});
  const second=await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal'&&v.viewId!==first.viewId));const b=await terminal(second.viewId);await until(async()=>(await pluginRpc(b,'view.initialize')).session.status==='running',30000);
  const other=await rpc(app.page,'servers.create',{name:'T023 second server workspace',host:'127.0.0.1',port:22341,username:'tester',authType:'key'});
  await rpc(app.page,'auth.ssh.set',{serverId:other.id,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});await rpc(app.page,'workspaces.open',{serverId:other.id});
  const otherView=await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===other.id));const d=await terminal(otherView.viewId);await until(async()=>(await pluginRpc(d,'view.initialize')).session.status==='running',30000);
  const ptys=(await rpc(app.page,'resources.list')).ptys.map(v=>v.id??v.ptyId).sort();assert.equal(ptys.length,3);
  await change([{key:'fontSize',value:24}]);await settingSize(a,24);await settingSize(b,24);await settingSize(d,24);
  assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(v=>v.id??v.ptyId).sort(),ptys);
  await rpc(app.page,'workspaces.activate',{workspaceId:server.id});await rpc(app.page,'views.activate',{viewId:first.viewId});await rpc(app.page,'views.move',{viewId:first.viewId,placement:'detached',group:0});a=await terminal(first.viewId);await settingSize(a,24);
  await change([{key:'fontSize',value:10}]);await settingSize(a,10);await settingSize(b,10);await settingSize(d,10);
  await rpc(app.page,'views.move',{viewId:first.viewId,placement:'center',group:0});a=await terminal(first.viewId);await settingSize(a,10);
  assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(v=>v.id??v.ptyId).sort(),ptys);
  note('Three real Bash sessions across two server workspaces on one isolated SSH host, hidden view and detach/return synchronize font limits 10/24 without recreating PTYs');
  await rpc(app.page,'views.activate',{viewId:first.viewId});assert.ok((await shellMarker(a,'BEFORE')).includes('BEFORE=__sodeck_prompt'));
  await change([{key:'shellIntegration',value:false}]);assert.ok((await shellMarker(a,'EXISTING')).includes('EXISTING=__sodeck_prompt'));
  await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:server.id,operationId:crypto.randomUUID()});
  const third=await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===server.id&&![first.viewId,second.viewId].includes(v.viewId)));const c=await terminal(third.viewId);await until(async()=>(await pluginRpc(c,'view.initialize')).session.status==='running',30000);
  assert.ok((await shellMarker(c,'NEW')).includes('NEW=none'));
  await rpc(app.page,'views.activate',{viewId:first.viewId});await rpc(app.page,'commands.execute',{commandId:'terminal.reconnect',viewId:first.viewId,operationId:crypto.randomUUID()});
  await until(async()=>(await pluginRpc(a,'view.initialize')).session.status==='running',30000);assert.ok((await shellMarker(a,'RECONNECTED')).includes('RECONNECTED=none'));
  note('Shell integration preserves existing Bash session, while new and explicitly reconnected sessions use the new preference');
  await a.screenshot({path:join(output,'terminal-settings.png')});
  const size=await a.evaluate(()=>{const last=[...document.querySelectorAll('.xterm-rows>div')].at(-1).getBoundingClientRect();return {last:last.bottom,grid:document.querySelector('.terminal-grid').getBoundingClientRect().bottom};});assert.ok(size.last<=size.grid+1);
  assert.deepEqual(app.errors,[]);await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,data:app.data,geometry:size},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack,data:app?.data,workbench:app?await rpc(app.page,'workbench.snapshot').catch(()=>null):null},null,2));throw e;}finally{await stop(app);}
