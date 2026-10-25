import {openTerminal} from './workbench-helpers.mjs';
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { root, launch, rpc, until, stop, pluginPages, nativeWindow, delay } from './harness.mjs';

const output=join(root,'.qa/ime',String(Date.now())); await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python')}});
let fixtureOutput='',fixtureErrors='',app,terminal,original,shiftChanged=false,mainHandle;const checks=[],attempts=[];
fixture.stdout.on('data',data=>fixtureOutput+=data);fixture.stderr.on('data',data=>fixtureErrors+=data);
async function nativeIme(action,options={}) {
 const args=['-NoProfile','-File',join(root,'tests/desktop/ime-windows.ps1'),'-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(mainHandle)];
 for(const [name,value] of Object.entries(options))args.push(`-${name}`,String(value));
 const {stdout}=await promisify(execFile)('powershell.exe',args,{windowsHide:true,timeout:15000});return JSON.parse(stdout);
}
async function events(){return terminal.evaluate(()=>window.__imeEvents);}
async function ownFocus(){await nativeWindow(app,'show',{WindowHandle:mainHandle});await nativeIme('focus');await rpc(app.page,'views.activate',{viewId:await terminal.evaluate(()=>window.__SODECK_CONTEXT__.viewId)});await terminal.locator('.xterm-helper-textarea').focus();return nativeIme('state');}
try {
 await until(()=>{if(fixture.exitCode!==null)throw new Error(fixtureErrors);return fixtureOutput.includes('\n');},15000);
 const config=JSON.parse(fixtureOutput.split('\n')[0]);await writeFile(join(output,'fixture.json'),JSON.stringify(config,null,2));
 app=await launch({data:join(output,'app'),shellPort:9393,pluginPort:9394});
 mainHandle=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck')).handle;
 const server=await rpc(app.page,'servers.create',{name:'IME 本机隔离 SSH',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
 let challenge;try{await openTerminal(app.page,server.id);}catch(error){challenge=error;}
 assert.equal(challenge?.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...challenge.details});await rpc(app.page,'auth.set',{serverId:server.id,password:'isolated-test-password'});
 const view=await openTerminal(app.page,server.id);
 terminal=await until(async()=>{for(const page of await pluginPages(app))if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===view.viewId)return page;});
 await until(()=>terminal.locator('.xterm-rows').innerText().then(text=>text.includes('fixture-ready')));
 await terminal.evaluate(()=>{
  window.__imeEvents=[];window.__imeComposing=false;
  for(const type of ['compositionstart','compositionupdate','compositionend','beforeinput','input','keydown'])document.addEventListener(type,event=>{
   if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;
   window.__imeEvents.push({type,data:event.data??null,key:event.key??null,isComposing:event.isComposing??null,isTrusted:event.isTrusted,time:performance.now()});
  },true);
 });
 original=await ownFocus();await writeFile(join(output,'original-state.json'),JSON.stringify(original,null,2));
 assert.equal(original.foregroundPid,app.process.pid);
 const requested=await nativeIme('language',{Layout:'0000000008040804'});await writeFile(join(output,'requested-state.json'),JSON.stringify(requested,null,2));
 const resourcesBefore=await rpc(app.page,'resources.list');let composed;
 for(const [index,letters] of ['zhongwen','ceshi'].entries()) {
  if(index===1){await ownFocus();await nativeIme('keys',{Keys:'SHIFT'});shiftChanged=true;}
  const start=(await events()).length;await nativeIme('keys',{Keys:letters});await delay(350);
  const currentEvents=(await events()).slice(start),isComposing=await terminal.evaluate(()=>window.__imeComposing);
  const attempt={letters,index,isComposing,events:currentEvents,state:await nativeIme('state')};attempts.push(attempt);
  if(isComposing&&currentEvents.some(event=>event.type==='compositionstart'&&event.isTrusted)) {
   await terminal.screenshot({path:join(output,'composition-page.png')});await nativeWindow(app,'capture',{WindowHandle:mainHandle,OutputPath:join(output,'composition-native.png')});
   let movementError;try{await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});}catch(error){movementError={code:error.code,message:error.message};}
   attempt.movementError=movementError;assert.equal(movementError?.code,'MOVE_CANCELLED');
   assert.equal((await rpc(app.page,'workbench.snapshot')).views.find(item=>item.viewId===view.viewId).placement,'center');
   checks.push('actual trusted Microsoft Pinyin composition rejects cross-window move with MOVE_CANCELLED');
   await nativeIme('keys',{Keys:'SPACE'});await until(async()=>!(await terminal.evaluate(()=>window.__imeComposing)),3000);
   const complete=(await events()).slice(start);attempt.completedEvents=complete;
   // WebView2 can report an untrusted compositionend after trusted native
   // compositionupdate/input. Require those native events and the real PTY
   // echo below instead of treating the end event flag as the input boundary.
   const end=complete.filter(event=>event.type==='compositionend').at(-1);
   assert.ok(end&&/[\u3400-\u9fff]/.test(end.data??''),'composition must commit Han characters');
   for(const type of ['compositionupdate','input'])assert.ok(complete.some(event=>event.type===type&&event.isTrusted&&event.data===end.data),`native trusted ${type} must match committed Han characters`);
   composed=end.data;await until(()=>terminal.locator('.xterm-rows').innerText().then(text=>text.includes(composed)));
   const screen=await terminal.locator('.xterm-rows').innerText();assert.equal(screen.split(composed).length-1,1);attempt.screenAfter=screen;
   checks.push(`actual VK composition commits ${JSON.stringify(composed)} and real SSH PTY echoes it exactly once`);
   // Pinyin Enter commits literal Latin text, giving a distinct echoed marker.
   await nativeIme('keys',{Keys:'afterime'});await nativeIme('keys',{Keys:'ENTER'});
   await until(()=>terminal.locator('.xterm-rows').innerText().then(text=>text.includes('afterime')));
   await nativeIme('keys',{Keys:'ENTER'});
   assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(item=>item.id),resourcesBefore.ptys.map(item=>item.id));
   const afterCommit=await events();assert.ok(afterCommit.some(event=>event.type==='keydown'&&event.key==='Enter'&&event.isTrusted));
   checks.push('completion after rejected move keeps the same PTY and subsequent physical input remains usable');
   break;
  }
  if(isComposing)await nativeIme('keys',{Keys:'ESC'});
 }
 if(!composed)throw new Error('Bounded two-mode VK attempt did not produce real trusted Chinese composition');
 await terminal.screenshot({path:join(output,'committed-terminal.png')});await nativeWindow(app,'capture',{WindowHandle:mainHandle,OutputPath:join(output,'committed-native.png')});
 if(shiftChanged){await ownFocus();await nativeIme('keys',{Keys:'SHIFT'});shiftChanged=false;}
 const restored=await nativeIme('state');await writeFile(join(output,'restored-state.json'),JSON.stringify(restored,null,2));
 if(original.focusLayout!==restored.focusLayout)await nativeIme('language',{Layout:original.focusLayout});
 if(original.open!==null&&restored.open!==null)assert.equal(restored.open,original.open,'own-window original IME open state restored');
 if(original.conversion!==null&&restored.conversion!==null)assert.equal(restored.conversion,original.conversion,'own-window original IME conversion state restored');
 await writeFile(join(output,'events.json'),JSON.stringify(await events(),null,2));
 // Exercise real IME again on the newly mounted workspace surface, then on
 // a different workspace in the original window. No synthetic text injection.
 const originalMain=mainHandle;
 const moved=await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});
 terminal=await until(async()=>{for(const p of await pluginPages(app))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===view.viewId)return p;});
 mainHandle=(await nativeWindow(app,'list')).find(w=>w.visible&&w.handle!==originalMain&&w.title.includes('SoDeck')).handle;
 const commitAt=async(letters,label)=>{
  await terminal.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
  const before=await ownFocus();await nativeIme('language',{Layout:'0000000008040804'});
  let switched=false,committed;
  try{
   for(let n=0;n<2;n++){
    await nativeIme('keys',{Keys:letters});await delay(200);
    if(await terminal.evaluate(()=>window.__imeComposing)){
     await nativeIme('keys',{Keys:'SPACE'});await until(()=>terminal.evaluate(()=>!window.__imeComposing));
     const es=await events(),end=es.filter(e=>e.type==='compositionend').at(-1);committed=end?.data;
     assert.match(committed??'',/[\u3400-\u9fff]/);assert.ok(es.some(e=>e.type==='input'&&e.isTrusted&&e.data===committed));
     await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes(committed)));
     assert.equal((await terminal.locator('.xterm-rows').innerText()).split(committed).length-1,1);
     await writeFile(join(output,`${label}-events.json`),JSON.stringify(es,null,2));await nativeWindow(app,'capture',{WindowHandle:mainHandle,OutputPath:join(output,`${label}.png`)});break;
    }
    await nativeIme('keys',{Keys:'SHIFT'});switched=!switched;
   }
   assert.ok(committed,'native composition must commit');
  }finally{if(switched)await nativeIme('keys',{Keys:'SHIFT'});if(before.focusLayout!==(await nativeIme('state')).focusLayout)await nativeIme('language',{Layout:before.focusLayout});}
 };
 await commitAt('ceshi','workspace-child');
 assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),resourcesBefore.ptys.map(p=>p.id));
 const other=await rpc(app.page,'servers.create',{name:'IME 独立工作区',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
 await rpc(app.page,'auth.set',{serverId:other.id,password:'isolated-test-password'});
 const second=await openTerminal(app.page,other.id);
 terminal=await until(async()=>{for(const p of await pluginPages(app))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===second.viewId)return p;});
 await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')));mainHandle=originalMain;
 await commitAt('zhongguo','workspace-main');
 const child=await until(async()=>{for(const p of await pluginPages(app))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===view.viewId)return p;});
 assert.ok(!(await child.locator('.xterm-rows').innerText()).includes('中国'));
 await rpc(app.page,'windows.merge',{sourceWindowId:moved.targetWindowId,operationId:crypto.randomUUID()});
 checks.push('whole-workspace migration rejects active composition; after migration both native windows commit Chinese exactly once to their own PTYs, with the original PTY unchanged');
 await stop(app);assert.equal(app.process.exitCode,0);app=null;
 await writeFile(join(output,'result.json'),JSON.stringify({passed:true,scope:'Windows Microsoft Pinyin + loopback SSH; no synthetic composition or Unicode injection',checks,attempts,original,restored,limitations:['candidate popup position not established','single monitor only; no Linux/OpenSSH']},null,2));
 console.log(JSON.stringify({passed:true,output,checks}));
} catch(error) {
 if(terminal){try{await writeFile(join(output,'events.json'),JSON.stringify(await events(),null,2));await terminal.screenshot({path:join(output,'failed-terminal.png'),timeout:2000});}catch{}}
 await writeFile(join(output,'result.json'),JSON.stringify({passed:false,checks,attempts,original,error:{message:error.message,code:error.code,stack:error.stack},fixtureErrors},null,2));console.error(error);process.exitCode=1;
} finally {
 if(app&&shiftChanged){try{await ownFocus();if(await terminal.evaluate(()=>window.__imeComposing))await nativeIme('keys',{Keys:'ESC'});await nativeIme('keys',{Keys:'SHIFT'});shiftChanged=false;await writeFile(join(output,'cleanup-restored-state.json'),JSON.stringify(await nativeIme('state'),null,2));}catch(error){await writeFile(join(output,'cleanup-restore-error.txt'),error.message);}}
 await stop(app);fixture.kill();await writeFile(join(output,'fixture-stderr.log'),fixtureErrors);
 for(const key of ['client-key','client-key-encrypted','client-openssh','client-openssh-encrypted','client-ed25519','client-ed25519-encrypted'])await rm(join(output,'fixture',key),{force:true});
}
