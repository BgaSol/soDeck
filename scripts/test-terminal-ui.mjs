// Actual terminal package + xterm UI + core PTY + independent OpenSSH.
import assert from 'node:assert/strict';
import {transferWorkbench} from './workbench-transfer-fixture.mjs';
import {recordTrace} from './ui-trace.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error('Fixture build failed');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-terminal-ui-'));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[data],{stdio:['pipe','pipe','inherit'],windowsHide:true});
const pending=[],routes=new Map(),pages=[],errors=[],traces=[];
let delayOwnership=false;
child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Core fixture exited'));});
readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line);if(r.event==='data'){const route=routes.get(r.surface);if(route)void route.page.evaluate(({callback,index,frame})=>window.callbacks.get(callback)?.({index,message:frame}),{callback:route.callback,index:route.index++,frame:r.frame}).catch(()=>{});return;}const p=pending.shift();r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true});
let clipboard=process.platform==='win32'?'echo sodeck-ui-command-ok':'printf "sodeck-ui-command-ok\\n"';
async function pageFor(window){
 const page=await browser.newPage({viewport:{width:1100,height:720}});pages.push(page);traces.push(await recordTrace(page.context(),'terminal-'+window));page.on('pageerror',e=>errors.push(String(e)));
 await page.addInitScript(()=>addEventListener('sodeck:surfaceReady',()=>{document.documentElement.dataset.testInputReady='true';},{once:true}));
 await page.exposeBinding('testInvoke',async(_,command,args={})=>{if(command==='clipboard_text'){if(args.operation==='read')return clipboard;clipboard=args.text;return null;}if(command==='component_data'&&args.request.op==='attach')routes.set(args.surfaceId,{page,callback:args.callback,index:0});return call(command,{...args,window});});
 await page.route(url+'__plugin__/**',route=>{const relative=new URL(route.request().url()).pathname.split('/').slice(3).join('/');const filename=path.join(process.env.SODECK_TEST_TERMINAL_PLUGIN,relative);const contentType={'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(filename)]||'application/octet-stream';
  let body=fs.readFileSync(filename);
  if(delayOwnership&&window==='main'&&relative.endsWith('/bridge.js')){
   // A restored xterm must wait for the ownership event, even when its delivery is delayed.
   const dispatch="else dispatchEvent(new CustomEvent('sodeck:'+message.type,{detail:message}));";
   assert(body.toString().includes(dispatch),'Terminal bridge still exposes the event dispatcher');
   body=Buffer.from(body.toString().replace(dispatch,"else {const notify=()=>dispatchEvent(new CustomEvent('sodeck:'+message.type,{detail:message}));if(message.type==='surfaceReady')setTimeout(notify,800);else notify();}"));
  }
  return route.fulfill({contentType,body});
 });
 await page.route(url+'__fixture_events.js',route=>route.fulfill({contentType:'text/javascript',body:fs.readFileSync('scripts/fixtures/events.js')}));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab">服务器管理</button><div id="server-tabs" class="server-tabs"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';import {installFixtureEvents} from '/__fixture_events.js';window.callbacks=new Map();let serial=Number(sessionStorage.getItem('sodeckTestCallbackSerial')||0);window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:callback=>{const id=++serial;sessionStorage.setItem('sodeckTestCallbackSerial',String(id));window.callbacks.set(id,callback);return id;}};window.adapter=(command,args={})=>{if(args.channel){args={...args,callback:args.channel.id};delete args.channel;}return window.testInvoke(command,args);};installFixtureEvents();window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),m=>document.querySelector('#notice').textContent=m,window.adapter);await window.manager.start();window.ready=true;</script></body></html>`}));
 await page.goto(url);
 try {await page.waitForFunction(()=>window.ready);} catch(error) {console.error({pageErrors:errors,notice:await page.locator('#notice').textContent()});throw error;}
 return page;
}
const wait=async fn=>{const deadline=Date.now()+15000;while(!await fn()){if(Date.now()>deadline)throw new Error('Core condition timed out');await new Promise(r=>setTimeout(r,30));}};
try{
 await call('fixture_load_plugin',{directory:process.env.SODECK_TEST_TERMINAL_PLUGIN});
 const catalog=await call('server_catalog');const changed=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'真实终端验收',host:'127.0.0.1',port:Number(process.env.SODECK_TEST_SSH_PORT),username:process.env.SODECK_TEST_SSH_USER,authentication:{type:'privateKey',privateKeyPath:process.env.SODECK_TEST_SSH_KEY,secret:{action:'clear'}},route:[]}}});
 await call('connection',{command:{op:'start',serverId:changed.selectedId}});
 await wait(async()=>{const [v]=await call('connection',{command:{op:'snapshot'}});if(v.question)await call('connection',{command:{op:'answer',attemptId:v.attemptId,questionId:v.question.id,answer:{kind:'hostKey',accept:true}}});if(v.state==='failed')throw new Error(JSON.stringify(v.error));return v.state==='connected';});
 const main=await pageFor('main');const board=main.locator('.server-workbench:visible');const frame=main.frameLocator('.component-surface:not([hidden]) iframe');
 await frame.locator('body[data-ready="true"]').waitFor();
 await frame.locator('html[data-test-input-ready="true"]').waitFor();
 await frame.locator('.xterm-helper-textarea').focus();await main.keyboard.press(process.platform==='darwin'?'Meta+v':process.platform==='linux'?'Control+Shift+v':'Control+v');
 await frame.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();await main.keyboard.press('Enter');
 await frame.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();
 await board.locator('[data-component-action="create"]').click();await board.locator('.component-tab').nth(1).waitFor();
 const state=await call('component_ui',{window:'main',operation:'snapshot',args:{}});const workspace=Object.values(state.workspaces)[0];assert.equal(workspace.views.length,2);
 const first=workspace.views[0].id;await board.locator(`[data-component-action="select"][data-key="${first}"]`).click();
 // Callback IDs must survive reload so abandoned surface messages cannot reach a new Channel.
 const previousCallbacks=await main.evaluate(()=>[...window.callbacks.keys()]);
 // Reload simulates a destroyed WebView: same plugin view and PTY, snapshot plus subsequent output.
 await main.waitForTimeout(600);await main.reload();await main.waitForFunction(()=>window.ready);
 const currentCallbacks=await main.evaluate(()=>[...window.callbacks.keys()]);
 assert(previousCallbacks.every(id=>!currentCallbacks.includes(id)),'Reload must not reuse old surface callback IDs');
 await main.frameLocator(`.component-surface[data-view-id="${first}"] iframe`).locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();
 assert.deepEqual(errors,[]);
 fs.mkdirSync('artifacts/terminal-ui',{recursive:true});await main.screenshot({path:'artifacts/terminal-ui/real-terminal.png'});
 for(const theme of ['light','dark','dopamine']){
  await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
  const selected=main.frameLocator(`.component-surface[data-view-id="${first}"] iframe`);
  const expected=await main.locator('html').evaluate(el=>getComputedStyle(el).getPropertyValue('--terminal-background').trim());
  await wait(()=>selected.locator('html').evaluate((el,color)=>getComputedStyle(el).getPropertyValue('--terminal-background').trim()===color,expected));
  await selected.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();
  await main.screenshot({path:`artifacts/terminal-ui/real-terminal-${theme}.png`});
 }
 const moved=await transferWorkbench(call,{window:'main',id:workspace.views[0].context.workspaceId,target:'terminal-transfer',scope:{kind:'view',id:first}},{sourcePage:main,openTarget:()=>pageFor('terminal-transfer')});
 const movedFrame=moved.frameLocator(`.component-surface[data-view-id="${first}"] iframe`);
 await movedFrame.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();
 await movedFrame.locator('html[data-test-input-ready="true"]').waitFor();
 await movedFrame.locator('.xterm-helper-textarea').focus();await moved.keyboard.type(process.platform==='win32'?'echo sodeck-ui-transfer-ok':'printf "sodeck-ui-transfer-ok\\n"');await moved.keyboard.press('Enter');
 await movedFrame.locator('.xterm-rows').filter({hasText:'sodeck-ui-transfer-ok'}).waitFor();
 delayOwnership=true;
 await transferWorkbench(call,{window:'terminal-transfer',id:workspace.views[0].context.workspaceId,target:'main',scope:{kind:'view',id:first}},{sourcePage:moved,openTarget:async()=>main});
 const oldFrame=main.frameLocator(`.component-surface[data-view-id="${first}"] iframe`);
 await oldFrame.locator('html[data-test-input-ready="true"]').waitFor();delayOwnership=false;
 await oldFrame.locator('.xterm-rows').filter({hasText:'sodeck-ui-transfer-ok'}).waitFor();
 await oldFrame.locator('.xterm-helper-textarea').focus();await main.keyboard.type('exit');await main.keyboard.press('Enter');
 const newTerminal=oldFrame.getByRole('button',{name:'新建终端',exact:true});await newTerminal.waitFor();
 await newTerminal.evaluate(el=>{el.click();el.click();});await board.locator('.component-tab').nth(2).waitFor();
 assert.equal(Object.values((await call('component_ui',{window:'main',operation:'snapshot',args:{}})).workspaces)[0].views.length,3,'Repeated exit recovery creates one new PTY');
 assert.match(await oldFrame.locator('.xterm-rows').textContent(),/sodeck-ui-command-ok/,'Exited terminal retains output');
 const context=(await call('connection',{command:{op:'snapshot'}}))[0].context;
 const command=process.platform==='win32'?`powershell.exe -NoLogo -NoProfile -Command "Write-Output 'sodeck-ui-launch-ready'; $line = [Console]::ReadLine(); Write-Output ('sodeck-ui-launch-input:' + $line); exit 7"`:`sh -c 'printf "sodeck-ui-launch-ready\\n"; read line; printf "sodeck-ui-launch-input:%s\\n" "$line"; exit 7'`;
 const config={version:1,title:'指定命令 UI',launch:{kind:'command',command},directoryIntegration:'off'};
 const action=await call('component_ui',{window:'main',operation:'invokeAction',args:{context,requestId:'ui-command',actionKey:'sodeck.terminal:new-terminal',data:config}});assert.equal(action.status,'completed');
 await main.evaluate(()=>window.manager.refresh());
 const commandFrame=main.frameLocator(`.component-surface[data-view-id="${action.viewId}"] iframe`);
 await commandFrame.locator('.xterm-rows').filter({hasText:'sodeck-ui-launch-ready'}).waitFor();
 await commandFrame.locator('html[data-test-input-ready="true"]').waitFor();
 await commandFrame.locator('.xterm-helper-textarea').focus();await main.keyboard.type('value');await main.keyboard.press('Enter');
 await commandFrame.locator('.xterm-rows').filter({hasText:'sodeck-ui-launch-input:value'}).waitFor();
 await commandFrame.getByRole('button',{name:'重新打开终端',exact:true}).click();await board.locator('.component-tab').nth(4).waitFor();
 const final=Object.values((await call('component_ui',{window:'main',operation:'snapshot',args:{}})).workspaces)[0];
 assert.deepEqual(final.views.find(v=>v.id===final.editor).params,config,'Command recovery preserves launch, title and directory strategy');
 await main.frameLocator(`.component-surface[data-view-id="${final.editor}"] iframe`).locator('.xterm-rows').filter({hasText:'sodeck-ui-launch-ready'}).waitFor();
 const connection=(await call('connection',{command:{op:'snapshot'}}))[0];await call('connection',{command:{op:'disconnect',context:connection.context}});await main.evaluate(()=>window.manager.refresh());
  await wait(()=>oldFrame.locator('#status + button').evaluate(el=>el.hidden));await oldFrame.locator('#status + button').evaluate(el=>el.click());
 assert.equal(Object.values((await call('component_ui',{window:'main',operation:'snapshot',args:{}})).workspaces)[0].views.length,5,'Disconnected recovery sends no new create');
 assert.deepEqual(errors,[]);
 console.log('Actual terminal UI passed: OpenSSH PTY, xterm input/output, multiple tabs, WebView remount replay, no page errors');
}catch(error){for(const finish of traces)await finish(true);throw error;}finally{for(const finish of traces)await finish(false);await browser.close();await vite.close();if(child.exitCode===null){child.stdin.end();await new Promise(resolve=>child.once('exit',resolve));}if(path.dirname(path.resolve(data))!==path.resolve(os.tmpdir())||!path.basename(data).startsWith('sodeck-terminal-ui-'))throw new Error('Unexpected cleanup');fs.rmSync(data,{recursive:true,force:true});}
