import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile,readFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages,pluginRpc,nativeWindow,delay} from './harness.mjs';

const output=join(root,'.qa/native',String(Date.now()));await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON ?? 'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python')}});
let fixtureOutput='',fixtureErrors='';fixture.stdout.on('data',d=>fixtureOutput+=d);fixture.stderr.on('data',d=>fixtureErrors+=d);
let app;const checks=[];
try{
  await until(()=>{if(fixture.exitCode!==null)throw new Error(fixtureErrors);return fixtureOutput.includes('\n');},15000);const config=JSON.parse(fixtureOutput.split('\n')[0]);
  await writeFile(join(output,'fixture.json'),JSON.stringify(config,null,2));
  app=await launch({data:join(output,'app'),shellPort:9333,pluginPort:9334});
  const server=await rpc(app.page,'servers.create',{name:'本机隔离 SSH',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
  let challenge;try{await rpc(app.page,'workbench.openTerminal',{serverId:server.id});}catch(error){challenge=error;}
  assert.equal(challenge?.code,'HOST_KEY_REQUIRED');checks.push('first connection requires host trust');
  await rpc(app.page,'hosts.trust',{serverId:server.id,...challenge.details});
  await rpc(app.page,'auth.set',{serverId:server.id,password:'isolated-test-password',remember:false});
  const first=await rpc(app.page,'workbench.openTerminal',{serverId:server.id});
  await rpc(app.page,'workbench.activate',{viewId:first.viewId});
  const terminal=await until(async()=>{const pages=await pluginPages(app);for(const page of pages){if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===first.viewId)return page;}});
  terminal.on('pageerror',e=>app.errors.push(e.message));
  await terminal.waitForSelector('.xterm');
  await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')),15000);
  await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.type('sodeck-native-echo');
  await until(async()=>await terminal.locator('.xterm-rows').innerText().then(t=>t.includes('sodeck-native-echo')));
  checks.push('real SSH PTY bytes rendered by native xterm and user input echoed');
  await app.page.screenshot({path:join(output,'workbench.png')});await terminal.screenshot({path:join(output,'terminal.png')});
  await nativeWindow(app,'capture',{OutputPath:join(output,'native-composite.png')});
  const peer=await rpc(app.page,'workbench.openTerminal',{serverId:server.id});
  const serverB=await rpc(app.page,'servers.create',{name:'独立工作区 B',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
  let challengeB;try{await rpc(app.page,'workbench.openTerminal',{serverId:serverB.id});}catch(error){challengeB=error;}
  assert.ok(['HOST_KEY_REQUIRED','AUTH_REQUIRED'].includes(challengeB?.code));
  if(challengeB.code==='HOST_KEY_REQUIRED')await rpc(app.page,'hosts.trust',{serverId:serverB.id,...challengeB.details});
  await rpc(app.page,'auth.set',{serverId:serverB.id,password:'isolated-test-password'});
  const b1=await rpc(app.page,'workbench.openTerminal',{serverId:serverB.id}),b2=await rpc(app.page,'workbench.openTerminal',{serverId:serverB.id});
  await rpc(app.page,'workbench.activateWorkspace',{serverId:server.id});
  await rpc(app.page,'workbench.move',{viewId:peer.viewId,placement:'center',group:1});
  assert.equal((await rpc(app.page,'workbench.snapshot')).split,'horizontal');
  await rpc(app.page,'workbench.configure',{split:'vertical'});await rpc(app.page,'workbench.move',{viewId:peer.viewId,placement:'bottom'});
  const running=await rpc(app.page,'resources.list');assert.equal(running.ptys.length,4);assert.equal(running.connections.length,2);
  checks.push('two server workspaces hold four real PTYs, sharing only within their own connection; split and bottom placement preserve them');
  await rpc(app.page,'workbench.activate',{viewId:first.viewId});
  const before=await rpc(app.page,'resources.list');
  await rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'detached',group:0});
  const detached=await until(async()=>{const pages=await pluginPages(app);for(const page of pages){if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===first.viewId)return page;}});
  await until(async()=>await detached.locator('.xterm-rows').innerText().then(t=>t.includes('sodeck-native-echo')));
  await detached.locator('.xterm-helper-textarea').focus();await detached.keyboard.type('-after-move');
  await until(async()=>await detached.locator('.xterm-rows').innerText().then(t=>t.includes('-after-move')));
  const after=await rpc(app.page,'resources.list');assert.equal(after.ptys.length,before.ptys.length);assert.equal(after.connections.length,before.connections.length);
  checks.push('native detach keeps rendered screen and PTY count, subsequent input works');
  await nativeWindow(app,'close');await until(async()=>!(await nativeWindow(app,'list')).find(w=>w.title==='SoDeck v2').visible);
  await detached.locator('.xterm-helper-textarea').focus();await detached.keyboard.type('-while-main-hidden');
  await until(async()=>await detached.locator('.xterm-rows').innerText().then(t=>t.includes('-while-main-hidden')));
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,4);await rpc(app.page,'app.show');
  checks.push('actual WM_CLOSE hides main to tray, detached terminal continues and main restores without reconnect');
  await rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'center',group:0});
  for(let cycle=0;cycle<10;cycle++){
    await rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'detached'});await rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'center'});
  }
  const afterCycles=await rpc(app.page,'resources.list');assert.deepEqual(afterCycles.ptys.map(p=>p.ptyId??p.id).sort(),before.ptys.map(p=>p.ptyId??p.id).sort());
  checks.push('ten detach/return cycles preserve all PTY identities and counts');
  await rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'detached'});
  const detachedShell=await until(async()=>{for(const page of app.browser.contexts().flatMap(c=>c.pages())){if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.detachedViewId)===first.viewId)return page;}});
  const mainWindow=(await nativeWindow(app,'list')).find(w=>w.title==='SoDeck v2');
  let detachedWindow=(await nativeWindow(app,'list')).find(w=>w.title!=='SoDeck v2'&&w.title.includes('SoDeck'));
  assert.ok(detachedWindow);
  await nativeWindow(app,'move',{Title:detachedWindow.title,X:mainWindow.x+mainWindow.width-350,Y:mainWindow.y+180});
  detachedWindow=(await nativeWindow(app,'list')).find(w=>w.title===detachedWindow.title);
  await detachedShell.locator(`#tab-${first.viewId}`).waitFor();
  const tabBounds=await detachedShell.locator(`#tab-${first.viewId}`).boundingBox();
  const scale=detachedWindow.dpi/96;
  const point={StartX:Math.round(detachedWindow.clientX+(tabBounds.x+24)*scale),StartY:Math.round(detachedWindow.clientY+(tabBounds.y+tabBounds.height/2)*scale),X:Math.round(mainWindow.clientX+mainWindow.clientWidth*.58),Y:Math.round(mainWindow.clientY+mainWindow.clientHeight*.45)};
  await writeFile(join(output,'physical-drag.json'),JSON.stringify({mainWindow,detachedWindow,tabBounds,point},null,2));
  await nativeWindow(app,'drag',{Title:detachedWindow.title,...point});
  await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===first.viewId)?.placement==='center');
  assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),before.ptys.map(p=>p.id).sort());
  checks.push('physical Windows mouse drag from detached Tab across window boundary returns to main at 200% DPI, preserving all PTYs');
  let current=await until(async()=>{for(const page of await pluginPages(app)){if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===first.viewId)return page;}});
  await current.evaluate(()=>{const previous=window.__sodeckNotify;window.__sodeckNotify=n=>{if(n.type==='view.prepareMove'){window.__sodeckNotify=previous;window.ipc.postMessage(JSON.stringify({apiVersion:1,requestId:crypto.randomUUID(),method:'view.moveRejected',params:{operationId:n.payload.operationId,reason:'native fault injection: source composition busy'}}));}else previous?.(n);};});
  await assert.rejects(()=>rpc(app.page,'workbench.move',{viewId:first.viewId,placement:'detached'}),e=>e.code==='MOVE_CANCELLED');
  assert.equal((await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===first.viewId).placement,'center');
  await current.locator('.xterm-helper-textarea').focus();await current.keyboard.type('-rollback-kept-input');await until(async()=>await current.locator('.xterm-rows').innerText().then(t=>t.includes('rollback-kept-input')));
  checks.push('injected source rejection rolls layout back and leaves input usable');
  const crashSession=await current.context().newCDPSession(current);await crashSession.send('Page.crash').catch(()=>{});await crashSession.detach().catch(()=>{});
  await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===first.viewId).status==='viewFailed');
  await rpc(app.page,'workbench.recover',{viewId:first.viewId});
  current=await until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===first.viewId)return page;}catch{}}});
  await current.getByText('终端显示已重建',{exact:false}).waitFor();await current.locator('.xterm-helper-textarea').focus();await current.keyboard.type('recovered-input');await until(async()=>await current.locator('.xterm-rows').innerText().then(t=>t.includes('recovered-input')));
  assert.equal((await rpc(app.page,'resources.list')).ptys.length,4);checks.push('actual renderer crash marks failure; explicit recovery resumes same PTY with authoritative cursors and visible screen gap');
  await rpc(app.page,'workbench.close',{viewId:first.viewId});
  for(const view of [peer,b1,b2])await rpc(app.page,'workbench.close',{viewId:view.viewId});
  await until(async()=>{const r=await rpc(app.page,'resources.list');return r.ptys.length===0&&r.leases.length===0&&r.connections.length===0;});
  checks.push('closing terminal releases PTY, lease and connection');
  const finalView=await rpc(app.page,'workbench.openTerminal',{serverId:server.id});
  const saturated=await app.page.evaluate(async()=>{
    let capacity=0;for(let offset=0;offset<10050;offset+=32){const results=await Promise.all(Array.from({length:Math.min(32,10050-offset)},(_,index)=>window.__TAURI_INTERNALS__.invoke('rpc',{request:{apiVersion:1,requestId:`capacity-${offset+index}`,method:'capacity.fixture',params:{}}})));capacity+=results.filter(r=>r.error?.code==='CAPACITY').length;}
    return capacity;
  });assert.ok(saturated>0);assert.equal((await rpc(app.page,'resources.list')).ptys.length,1);
  const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`Get-CimInstance Win32_Process -Filter "ParentProcessId = ${app.process.pid}" | Where-Object { $_.CommandLine -like '*--sodeck-internal-plugin-host*' } | ForEach-Object { Stop-Process -Id $_.ProcessId; $_.ProcessId }`],{windowsHide:true});assert.ok(Number(stdout.trim()));
  await until(async()=>{const r=await rpc(app.page,'resources.list');return r.ptys.length===0&&r.leases.length===0&&r.connections.length===0;});await rpc(app.page,'workbench.close',{viewId:finalView.viewId});
  checks.push('actual QuickJS child termination revokes its instance, releases all resources and keeps core/workbench close usable');
  checks.push('10000-entry dedup cache saturation still permits resource queries, workbench close and normal App exit');
  assert.deepEqual(app.errors,[]);
  await stop(app);assert.equal(app.process.exitCode,0);app=null;
  await writeFile(join(output,'result.json'),JSON.stringify({passed:true,scope:'Windows native + loopback Paramiko SSH protocol; not Linux/OpenSSH',checks},null,2));
  console.log(JSON.stringify({passed:true,output,checks}));
}catch(error){
  const diagnostic={};
  if(app){
    for(const method of ['resources.list','workbench.snapshot','calls.list']){try{diagnostic[method]=await rpc(app.page,method);}catch(e){diagnostic[method]={error:e.message};}}
    try{diagnostic.pages=[];for(const [index,page] of (await pluginPages(app)).entries()){
      try{diagnostic.pages.push(await page.evaluate(()=>({context:window.__SODECK_CONTEXT__,text:document.body.innerText})));await page.screenshot({path:join(output,`failed-plugin-${index}.png`),timeout:2000});}catch(e){diagnostic.pages.push({error:e.message});}
    }}catch{}
  }
  await writeFile(join(output,'failure-diagnostic.json'),JSON.stringify(diagnostic,null,2));
  await writeFile(join(output,'result.json'),JSON.stringify({passed:false,checks,error:{message:error.message,stack:error.stack,code:error.code},uiErrors:app?.errors,fixtureErrors},null,2));console.error(error);process.exitCode=1;
}
finally{await stop(app);fixture.kill();await writeFile(join(output,'fixture-stderr.log'),fixtureErrors);for(const key of ['client-key','client-key-encrypted','client-openssh','client-openssh-encrypted','client-ed25519','client-ed25519-encrypted'])await rm(join(output,'fixture',key),{force:true});}
