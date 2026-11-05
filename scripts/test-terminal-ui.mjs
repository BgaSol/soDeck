// Actual terminal package + xterm UI + core PTY + independent OpenSSH.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error('Fixture build failed');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-terminal-ui-'));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[data],{stdio:['pipe','pipe','inherit'],windowsHide:true});
const pending=[],routes=new Map(),pages=[],errors=[];
child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Core fixture exited'));});
readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line);if(r.event==='data'){const route=routes.get(r.surface);if(route)void route.page.evaluate(({callback,index,frame})=>window.callbacks.get(callback)?.({index,message:frame}),{callback:route.callback,index:route.index++,frame:r.frame}).catch(()=>{});return;}const p=pending.shift();r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true});
let clipboard=process.platform==='win32'?'echo sodeck-ui-command-ok':'printf "sodeck-ui-command-ok\\n"';
async function pageFor(window){
 const page=await browser.newPage({viewport:{width:1100,height:720}});pages.push(page);page.on('pageerror',e=>errors.push(String(e)));
 await page.exposeBinding('testInvoke',async(_,command,args={})=>{if(command==='clipboard_text'){if(args.operation==='read')return clipboard;clipboard=args.text;return null;}if(command==='component_data'&&args.request.op==='attach')routes.set(args.surfaceId,{page,callback:args.callback,index:0});return call(command,{...args,window});});
 await page.route(url+'__plugin__/**',route=>{const relative=new URL(route.request().url()).pathname.split('/').slice(3).join('/');const filename=path.join(process.env.SODECK_TEST_TERMINAL_PLUGIN,relative);const contentType={'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(filename)]||'application/octet-stream';return route.fulfill({contentType,body:fs.readFileSync(filename)});});
 await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab">服务器管理</button><div id="server-tabs" class="server-tabs"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';window.callbacks=new Map();let serial=0;window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:callback=>{const id=++serial;window.callbacks.set(id,callback);return id;}};window.adapter=(command,args={})=>{if(args.channel){args={...args,callback:args.channel.id};delete args.channel;}return window.testInvoke(command,args);};window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),m=>document.querySelector('#notice').textContent=m,window.adapter);await window.manager.start();window.ready=true;</script></body></html>`}));
 await page.goto(url);await page.waitForFunction(()=>window.ready);return page;
}
const wait=async fn=>{const deadline=Date.now()+15000;while(!await fn()){if(Date.now()>deadline)throw new Error('Core condition timed out');await new Promise(r=>setTimeout(r,30));}};
try{
 await call('fixture_load_plugin',{directory:process.env.SODECK_TEST_TERMINAL_PLUGIN});
 const catalog=await call('server_catalog');const changed=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'真实终端验收',host:'127.0.0.1',port:Number(process.env.SODECK_TEST_SSH_PORT),username:process.env.SODECK_TEST_SSH_USER,authentication:{type:'privateKey',privateKeyPath:process.env.SODECK_TEST_SSH_KEY,secret:{action:'clear'}},route:[]}}});
 await call('connection',{command:{op:'start',serverId:changed.selectedId}});
 await wait(async()=>{const [v]=await call('connection',{command:{op:'snapshot'}});if(v.question)await call('connection',{command:{op:'answer',attemptId:v.attemptId,questionId:v.question.id,answer:{kind:'hostKey',accept:true}}});if(v.state==='failed')throw new Error(JSON.stringify(v.error));return v.state==='connected';});
 const main=await pageFor('main');const board=main.locator('.server-workbench:visible');const frame=main.frameLocator('.server-workbench:visible iframe');
 await frame.locator('body[data-ready="true"]').waitFor();
 await frame.locator('.xterm-helper-textarea').focus();await main.keyboard.press(process.platform==='darwin'?'Meta+v':process.platform==='linux'?'Control+Shift+v':'Control+v');
 await frame.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();await main.keyboard.press('Enter');
 await frame.locator('.xterm-rows').filter({hasText:'sodeck-ui-command-ok'}).waitFor();
 await board.locator('[data-component-action="create"]').click();await board.locator('.component-tab').nth(1).waitFor();
 const state=await call('component_ui',{window:'main',operation:'snapshot',args:{}});const workspace=Object.values(state.workspaces)[0];assert.equal(workspace.views.length,2);
 const first=workspace.views[0].id;await board.locator(`[data-component-action="select"][data-key="${first}"]`).click();
 // Reload simulates a destroyed WebView: same plugin view and PTY, snapshot plus subsequent output.
 await main.waitForTimeout(600);await main.reload();await main.waitForFunction(()=>window.ready);
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
 console.log('Actual terminal UI passed: OpenSSH PTY, xterm input/output, multiple tabs, WebView remount replay, no page errors');
}finally{await browser.close();await vite.close();if(child.exitCode===null){child.stdin.end();await new Promise(resolve=>child.once('exit',resolve));}if(path.dirname(path.resolve(data))!==path.resolve(os.tmpdir())||!path.basename(data).startsWith('sodeck-terminal-ui-'))throw new Error('Unexpected cleanup');fs.rmSync(data,{recursive:true,force:true});}
