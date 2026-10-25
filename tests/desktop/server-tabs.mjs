import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages,nativeWindow,delay} from './harness.mjs';

const output=join(root,'.qa/t-004/native',String(Date.now()));await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python')}});
let fixtureOutput='',fixtureErrors='';fixture.stdout.on('data',d=>fixtureOutput+=d);fixture.stderr.on('data',d=>fixtureErrors+=d);
let app;const checks=[];
const findSurface=async id=>until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===id)return page;}catch{}}});
try{
  await until(()=>{if(fixture.exitCode!==null)throw new Error(fixtureErrors);return fixtureOutput.includes('\n');});const config=JSON.parse(fixtureOutput.split('\n')[0]);
  app=await launch({data:join(output,'app'),shellPort:9423,pluginPort:9424});
  const create=async name=>{
    const server=await rpc(app.page,'servers.create',{name,host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
    let error;try{await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id});}catch(e){error=e;}
    assert.ok(['HOST_KEY_REQUIRED','AUTH_REQUIRED'].includes(error?.code));if(error.code==='HOST_KEY_REQUIRED')await rpc(app.page,'hosts.trust',{serverId:server.id,...error.details});await rpc(app.page,'auth.set',{serverId:server.id,password:'isolated-test-password',remember:false});return server;
  };
  const a=await create('本机工作区 A'),b=await create('本机工作区 B');
  await rpc(app.page,'workspaces.open',{serverId:a.id});await rpc(app.page,'workspaces.open',{serverId:b.id});
  assert.deepEqual((await rpc(app.page,'workbench.snapshot')).workspaceIds,[a.id,b.id]);
  const bWindow=await rpc(app.page,'workspaces.move',{workspaceId:b.id,operationId:crypto.randomUUID()});
  assert.equal(bWindow.committed,true);const bShell=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId)===bWindow.targetWindowId)return p;});
  assert.equal((await rpc(bShell,'workbench.snapshot')).activeWorkspaceId,b.id);assert.equal((await rpc(app.page,'workbench.snapshot')).activeWorkspaceId,a.id);
  const duplicate=await rpc(app.page,'workspaces.open',{serverId:b.id});assert.equal(duplicate.created,false);assert.equal(duplicate.ownerWindowId,bWindow.targetWindowId);
  checks.push('ST-02/03: separate window activation and duplicate open locate one authoritative workspace');
  const first=await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:a.id,operationId:crypto.randomUUID()});
  let terminal=await findSurface(first.viewId);await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')));
  await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.type('t004-before-move');await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('t004-before-move')));
  const before=await rpc(app.page,'resources.list');
  const operationId=crypto.randomUUID();const moved=await rpc(app.page,'workspaces.move',{workspaceId:a.id,targetWindowId:bWindow.targetWindowId,operationId});assert.equal(moved.committed,true);
  assert.equal((await rpc(app.page,'workbench.operation.get',{operationId})).committed,true);
  terminal=await findSurface(first.viewId);await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('t004-before-move')));
  await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.type('-after');await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('-after')));
  assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
  checks.push('ST-04/06/11: batch move retains screen, PTY identity, input and operation result');
  await rpc(bShell,'commands.execute',{commandId:'terminal.rename',viewId:first.viewId,args:{title:'移动后名称'}});assert.equal((await rpc(bShell,'workbench.snapshot')).views.find(v=>v.viewId===first.viewId).title,'移动后名称');
  checks.push('ST-16: plugin publishes metadata via generic views.update');
  await rpc(bShell,'windows.merge',{operationId:crypto.randomUUID()});
  assert.deepEqual((await rpc(app.page,'workbench.snapshot')).workspaceIds,[b.id,a.id]);
  assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
  await app.page.screenshot({path:join(output,'shell.png')});await delay(400);const window=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck'));assert.ok(window);await nativeWindow(app,'capture',{Title:window.title,OutputPath:join(output,'native-composite.png')});
  await rpc(app.page,'commands.execute',{commandId:'terminal.reconnect',viewId:first.viewId,operationId:crypto.randomUUID()});
  const reconnected=await rpc(app.page,'resources.list');assert.equal(reconnected.ptys.length,1);assert.notEqual(reconnected.ptys[0].id,before.ptys[0].id);
  terminal=await findSurface(first.viewId);await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')));
  await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.type('reconnected-new-binding');
  await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('reconnected-new-binding')));
  checks.push('ST-16: generic reconnect command replaces PTY and lease, retains the logical view, and resumes input through the new resource binding');
  await rpc(app.page,'workspaces.close',{workspaceId:a.id});assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  const next=await rpc(app.page,'commands.execute',{commandId:'terminal.new',workspaceId:b.id});assert.ok(next.viewId);await rpc(app.page,'workspaces.close',{workspaceId:b.id});
  const resources=await rpc(app.page,'resources.list');assert.equal(resources.ptys.length,0);assert.equal(resources.leases.length,0);assert.equal(resources.connections.length,0);
  checks.push('ST-07/09: merge and close respect scope; shared worker continues after workspace close');
  assert.deepEqual(app.errors,[]);
  await writeFile(join(output,'results.json'),JSON.stringify({checks,output},null,2));console.log(JSON.stringify({checks,output},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);fixture.kill();await writeFile(join(output,'fixture.log'),fixtureErrors);}
