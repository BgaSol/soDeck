import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages,pluginRpc,nativeWindow,delay} from './harness.mjs';
import {openTerminal} from './workbench-helpers.mjs';
const output=join(root,'.qa/t006-native',String(Date.now()));await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python'),SODECK_TEST_CAPTURE_INPUT:'1'}});
let stdout='',stderr='',app;fixture.stdout.on('data',d=>stdout+=d);fixture.stderr.on('data',d=>stderr+=d);const checks=[];
const record=s=>{checks.push(s);console.log(s)};
const findSurface=async id=>until(async()=>{for(const p of await pluginPages(app))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===id)return p;});
const events=async()=>readFile(join(output,'fixture/events.jsonl'),'utf8').then(t=>t.trim().split('\n').map(JSON.parse));
const input=async()=>Buffer.concat((await events()).filter(e=>e.event==='input-bytes').map(e=>Buffer.from(e.bytes,'base64')));
try{
 await until(()=>stdout.includes('\n'));const config=JSON.parse(stdout.split('\n')[0]);
 app=await launch({data:join(output,'app'),shellPort:9563,pluginPort:9564});
 const win=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck'));
 const server=await rpc(app.page,'servers.create',{name:'T006 隔离 SSH · Graphite',host:'127.0.0.1',port:config.port,username:'test-user',authType:'password'});
 let challenge;try{await openTerminal(app.page,server.id);}catch(e){challenge=e;}assert.equal(challenge?.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...challenge.details});await rpc(app.page,'auth.ssh.set',{serverId:server.id,password:'isolated-test-password'});
 const first=await openTerminal(app.page,server.id);let terminal=await findSurface(first.viewId);await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('fixture-ready')));
 assert.equal((await rpc(app.page,'workbench.snapshot')).schemaVersion,3);assert.equal(await app.page.locator('.sessions-panel').count(),0);assert.equal(await terminal.locator('.terminal-toolbar').count(),0);
 const theme=await rpc(app.page,'ui.theme.get');assert.deepEqual(await terminal.evaluate(()=>window.__SODECK_CONTEXT__.theme),theme);record('UX-01/03: native single central group, no session sidebar or repeated terminal toolbar; same theme snapshot');
 await nativeWindow(app,'show',{WindowHandle:win.handle});await terminal.locator('.xterm-helper-textarea').focus();
 for(const [index,key] of ['Control+v','Control+Shift+v','Shift+Insert'].entries()){
   const text=`paste-${index}-中文🙂\r\nline-${index}`;await rpc(app.page,'clipboard.writeText',{text});await terminal.keyboard.press(key);await until(async()=>(await input()).includes(Buffer.from(text.replaceAll('\r\n','\r'))));
 }
 let bytes=await input();for(let i=0;i<3;i++)assert.equal(bytes.toString().split(`paste-${i}-`).length-1,1);record('IN-02: native clipboard and all three paste shortcuts submit UTF-8 exactly once');
 await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.press('Control+c');await until(async()=>(await input()).includes(3));const interrupts=(await input()).filter(b=>b===3).length;
 const row=terminal.locator('.xterm-rows > div').first();const r=await row.boundingBox();await terminal.mouse.click(r.x+35,r.y+r.height/2,{clickCount:3});await terminal.keyboard.press('Control+c');await until(async()=>(await rpc(app.page,'clipboard.readText')).text.includes('fixture-ready'));
 assert.equal((await input()).filter(b=>b===3).length,interrupts);await terminal.keyboard.press('Control+c');await until(async()=>(await input()).filter(b=>b===3).length===interrupts+1);record('IN-01: selected Ctrl+C copies without ETX, clears selection; following Ctrl+C interrupts once');
 const menuId=crypto.randomUUID();const before=Date.now();await pluginRpc(terminal,'ui.menu.open',{menuId,anchor:{x:80,y:65},items:[{id:'copy',label:'复制',enabled:true,shortcut:'Ctrl+C'},{id:'paste',label:'粘贴',enabled:true,shortcut:'Ctrl+V'}]});assert.ok(Date.now()-before<2000);assert.equal((await pluginRpc(terminal,'ui.menu.result',{menuId})).status,'open');
 await nativeWindow(app,'screen',{WindowHandle:win.handle,OutputPath:join(output,'native-menu.png')});
 const beforeEsc=await input();await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:27});await until(async()=>(await pluginRpc(terminal,'ui.menu.result',{menuId})).status==='closed');assert.deepEqual(await input(),beforeEsc);assert.ok(await terminal.locator('.xterm-rows').isVisible());record('UX-05/IN-03: actual Windows owner-drawn popup returns receipt immediately, Esc cancels without terminal Esc, terminal remains painted');
 const selected=crypto.randomUUID();await pluginRpc(terminal,'ui.menu.open',{menuId:selected,anchor:{x:80,y:65},items:[{id:'disabled',label:'不可用',enabled:false},{id:'chosen',label:'可选操作',enabled:true}]});await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:40});await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:40});await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:13});await until(async()=>(await pluginRpc(terminal,'ui.menu.result',{menuId:selected})).status==='closed');assert.equal((await pluginRpc(terminal,'ui.menu.result',{menuId:selected})).selectedId,'chosen');record('UX-04/09: native keyboard menu selects enabled item and returns only the chosen stable ID');
 const outside=crypto.randomUUID();await rpc(app.page,'ui.menu.open',{menuId:outside,anchor:{x:100,y:44},items:[{id:'one',label:'关闭测试',enabled:true}]});await nativeWindow(app,'click',{WindowHandle:win.handle,X:win.clientX+600,Y:win.clientY+300});await until(async()=>(await rpc(app.page,'ui.menu.result',{menuId:outside})).status==='closed');record('UX-05: shell popup closes on a physical click over the native terminal');
 for(const action of ['replace','minimize','switch']){
   const id=crypto.randomUUID();await pluginRpc(terminal,'ui.menu.open',{menuId:id,anchor:{x:100,y:60},items:[{id:'one',label:'生命周期测试',enabled:true}]});
   if(action==='replace'){const next=crypto.randomUUID();await rpc(app.page,'ui.menu.open',{menuId:next,anchor:{x:100,y:44},items:[{id:'next',label:'另一个菜单',enabled:true}]});await rpc(app.page,'ui.menu.close',{menuId:next});}
   if(action==='minimize'){await nativeWindow(app,'minimize',{WindowHandle:win.handle});await nativeWindow(app,'show',{WindowHandle:win.handle});}
   if(action==='switch')await rpc(app.page,'views.activate',{viewId:first.viewId});
   await until(async()=>(await pluginRpc(terminal,'ui.menu.result',{menuId:id})).status==='closed');
 }record('UX-05: cross-surface replacement, minimize and view activation close the old menu');
 await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.type('after-menu');await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('after-menu')));
 await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'native-composite.png')});
 await rpc(app.page,'clipboard.writeText',{text:'menu-paste-once'});await terminal.locator('.xterm-helper-textarea').focus();await terminal.keyboard.press('Shift+F10');await delay(200);
 await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:40});await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:40});await nativeWindow(app,'menuKey',{WindowHandle:win.handle,KeyCode:13});
 await until(async()=>(await input()).toString().includes('menu-paste-once'));assert.equal((await input()).toString().split('menu-paste-once').length-1,1);record('IN-02/07: actual plugin menu paste returns focus and reads/submits once');
 const resources=await rpc(app.page,'resources.list');
 for(let i=0;i<10;i++){await rpc(app.page,'views.move',{viewId:first.viewId,placement:'detached'});await rpc(app.page,'views.move',{viewId:first.viewId,placement:'center'});}assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),resources.ptys.map(p=>p.id));terminal=await findSurface(first.viewId);await until(()=>terminal.locator('.xterm-rows').innerText().then(t=>t.includes('after-menu')));record('UX-07: ten native detach/return cycles preserve PTY identity and displayed output');
 const savedId=first.viewId;await stop(app);app=await launch({data:join(output,'app'),shellPort:9563,pluginPort:9564});assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);const ended=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===savedId);assert.equal(ended.status,'ended');assert.ok(ended.actions.includes('terminal.reconnect'));await rpc(app.page,'commands.execute',{commandId:'terminal.rename',viewId:savedId,args:{title:'重启后保留名称'}});record('UX-08: restart retains ended placeholder and rename action without restoring any old SSH handle');
 assert.deepEqual(app.errors,[]);await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,output},null,2));console.log(JSON.stringify({output,checks},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);fixture.kill();await writeFile(join(output,'fixture.log'),stderr);}
