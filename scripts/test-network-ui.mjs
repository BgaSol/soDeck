// Shipped network plugin UI, actual core service, and isolated OpenSSH.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import net from 'node:net';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error('Fixture build failed');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-network-ui-'));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[data],{stdio:['pipe','pipe','inherit'],windowsHide:true});
const pending=[],errors=[],actions=[];let clipboard='';child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Core fixture exited'));});
readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line),p=pending.shift();if(!p)throw new Error('Unexpected fixture response');r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const service=net.createServer(socket=>socket.pipe(socket));await new Promise(r=>service.listen(0,'127.0.0.1',r));
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];const browser=await chromium.launch({headless:true});
const page=await browser.newPage({viewport:{width:1180,height:760}});page.on('pageerror',e=>errors.push(String(e)));
await page.exposeBinding('testInvoke',(_,command,args={})=>{if(command==='clipboard_text'){clipboard=args.text;return null;}if(command==='component_ui'&&args.operation==='event')actions.push(args.args);return call(command,{...args,window:'main'});});
await page.route(url+'__plugin__/**',route=>{const relative=new URL(route.request().url()).pathname.split('/').slice(3).join('/');return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(relative)]||'application/octet-stream',body:fs.readFileSync(path.join(process.env.SODECK_TEST_NETWORK_PLUGIN,relative))});});
await page.route(url+'__fixture_events.js',route=>route.fulfill({contentType:'text/javascript',body:fs.readFileSync('scripts/fixtures/events.js')}));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN" data-theme="dopamine"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab">服务器管理</button><div id="server-tabs" class="server-tabs"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';import {installFixtureEvents} from '/__fixture_events.js';window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:()=>1};window.adapter=(command,args={})=>window.testInvoke(command,args);installFixtureEvents();window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),m=>document.querySelector('#notice').textContent=m,window.adapter);await window.manager.start();window.ready=true;</script></body></html>`}));
const wait=async fn=>{const end=Date.now()+25000;while(!await fn()){if(Date.now()>end)throw new Error('Condition timed out');await new Promise(r=>setTimeout(r,50));}};
try{
 await call('fixture_load_plugin',{directory:process.env.SODECK_TEST_NETWORK_PLUGIN});
 const catalog=await call('server_catalog');const change=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'网络隧道验收',host:'127.0.0.1',port:Number(process.env.SODECK_TEST_SSH_PORT),username:process.env.SODECK_TEST_SSH_USER,authentication:{type:'privateKey',privateKeyPath:process.env.SODECK_TEST_SSH_KEY,secret:{action:'clear'}},route:[]}}});
 await call('connection',{command:{op:'start',serverId:change.selectedId}});
 await wait(async()=>{const [v]=await call('connection',{command:{op:'snapshot'}});if(v.question)await call('connection',{command:{op:'answer',attemptId:v.attemptId,questionId:v.question.id,answer:{kind:'hostKey',accept:true}}});if(v.state==='failed')throw new Error(JSON.stringify(v.error));return v.state==='connected';});
 await page.goto(url);await page.waitForFunction(()=>window.ready);await page.getByRole('button',{name:'网络隧道',exact:true}).click();
 const sidebar=page.frameLocator('iframe[title^="网络隧道 ·"]');await sidebar.getByRole('button',{name:'＋ 新建',exact:true}).click();
 const editor=page.frameLocator('iframe[title^="编辑网络隧道 ·"]');await editor.locator('input[name=name]').fill('应用服务');await editor.locator('input[name=host]').fill('127.0.0.1');await editor.locator('input[name=targetPort]').fill(String(service.address().port));
 // Fixed-port checkbox and small viewport scroll must be usable.
 await editor.locator('input[name=auto]').uncheck();assert.equal(await editor.locator('input[name=port]').isEnabled(),true);await editor.locator('input[name=auto]').check();
 await page.setViewportSize({width:720,height:480});await editor.locator('summary').click();await editor.locator('.fields').evaluate(el=>el.scrollTop=el.scrollHeight);await editor.locator('input[name=bindHost]').waitFor({state:'visible'});
 const scroll=await editor.locator('.fields').evaluate(el=>({scroll:el.scrollHeight,client:el.clientHeight,top:el.scrollTop}));assert(scroll.scroll>scroll.client&&scroll.top>0);assert(await editor.getByRole('button',{name:'保存',exact:true}).isVisible());
 fs.mkdirSync('artifacts/network-ui',{recursive:true});await page.screenshot({path:'artifacts/network-ui/editor-narrow.png'});
 await editor.getByRole('button',{name:'保存',exact:true}).click();await page.locator('.plugin-view-dialog').waitFor({state:'detached'});await page.setViewportSize({width:1180,height:760});
 const instrument=()=>sidebar.locator('#app').evaluate(()=>{window.refreshes=0;const request=sodeck.request;sodeck.request=async(method,params)=>{const result=await request(method,params);if(method==='event'&&params.action==='snapshot')window.refreshes++;return result;};});
 await instrument();
 const cycles=async()=>{const before=await sidebar.locator('#app').evaluate(()=>window.refreshes);try{await wait(()=>sidebar.locator('#app').evaluate((el,n)=>window.refreshes>=n+2,before));}catch(e){console.log({refreshes:await sidebar.locator('#app').evaluate(()=>window.refreshes),error:await sidebar.locator('#error').textContent(),actions:actions.slice(-5)});throw e;}};
 const listRow=sidebar.getByRole('button',{name:'应用服务'});
 await listRow.focus();await listRow.evaluate(el=>window.stableRow=el);await cycles();
 assert(await listRow.evaluate(el=>el===window.stableRow&&el===document.activeElement),'List row identity and focus survive real refreshes');
 assert.equal(await sidebar.getByRole('button',{name:'上一页规则',exact:true}).count(),1);
 await listRow.click();assert.equal(await sidebar.getByRole('button',{name:'启动',exact:true}).isEnabled(),true);await sidebar.getByRole('button',{name:'启动',exact:true}).click();await sidebar.getByRole('button',{name:'停止',exact:true}).waitFor();
 const pathSummary=sidebar.getByText('连接路径',{exact:true});await pathSummary.click();await pathSummary.focus();await pathSummary.evaluate(el=>window.stableSummary=el);await cycles();
 assert(await pathSummary.evaluate(el=>el===window.stableSummary&&el===document.activeElement&&el.parentElement.open));
 const getEntry=()=>sidebar.locator('.entry code').textContent();const entry=await getEntry();assert.match(entry,/127\.0\.0\.1:\d+/);
 await sidebar.getByRole('button',{name:'复制',exact:true}).click();await wait(()=>clipboard===entry);assert.equal(clipboard,entry);await cycles();assert.equal(await sidebar.getByRole('button',{name:'已复制',exact:true}).count(),1);
 const echo=()=>new Promise((resolve,reject)=>{const socket=net.connect(Number(entry.split(':').at(-1)),'127.0.0.1',()=>socket.write('ui-real-relay'));socket.once('data',bytes=>{try{assert.equal(bytes.toString(),'ui-real-relay');socket.destroy();resolve();}catch(e){reject(e)}});socket.on('error',reject);socket.setTimeout(8000,()=>{socket.destroy();reject(new Error('UI tunnel timeout'))});});await echo();
 const networkFrame=await page.locator('iframe[title^="网络隧道 ·"]').elementHandle();
 for(const side of ['left','right']){
  await page.evaluate(()=>manager.refresh());
  const button=await page.getByRole('button',{name:'网络隧道',exact:true}).boundingBox(),rail=await page.locator(`.server-workbench:visible .${side}-rail`).boundingBox();
  await page.mouse.move(button.x+button.width/2,button.y+button.height/2);await page.mouse.down();await page.mouse.move(rail.x+rail.width/2,rail.y+140,{steps:12});await page.mouse.up();
  await wait(()=>page.locator(`.${side}-rail [data-tool-key]`).getAttribute('aria-expanded').then(value=>value==='true'));
  assert(await networkFrame.evaluate(el=>el.isConnected),'Network docking retains the iframe');assert.equal(await getEntry(),entry);await echo();
 }
 const rulesFile=path.join(data,'plugin-data/sodeck.network-proxy/network-rules.json'),rulesBeforeSettings=fs.readFileSync(rulesFile);
 const preference=await call('settings_page',{pluginId:'sodeck.network-proxy',pageId:'defaults'});
 await call('save_settings',{pluginId:'sodeck.network-proxy',pageId:'defaults',input:{definitionToken:preference.definitionToken,baseRevision:preference.revision,requestId:'new-rule-defaults',values:{defaultType:'socks5',portMode:'fixed',fixedPort:2080}}});
 assert.deepEqual(fs.readFileSync(rulesFile),rulesBeforeSettings);assert.equal(await getEntry(),entry);await echo();
 for(const theme of ['dopamine','light','dark']){await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);await page.screenshot({path:`artifacts/network-ui/detail-${theme}.png`});}
 await sidebar.getByRole('button',{name:'编辑',exact:true}).click();await editor.locator('input[name=name]').fill('未保存的修改');await editor.getByRole('button',{name:'取消',exact:true}).click();await page.getByRole('button',{name:'取消',exact:true}).waitFor();await page.getByRole('button',{name:'取消',exact:true}).click();assert.equal(await editor.locator('input[name=name]').inputValue(),'未保存的修改');await editor.getByRole('button',{name:'取消',exact:true}).click();await page.getByRole('button',{name:'放弃并关闭',exact:true}).click();await page.locator('.plugin-view-dialog').waitFor({state:'detached'});await echo();
 // A reload remounts the logical view, retaining the native listener and theme.
 await page.reload();await page.waitForFunction(()=>window.ready);await sidebar.locator('.entry code').waitFor();await instrument();assert.equal(await getEntry(),entry);await echo();
 const stop=sidebar.getByRole('button',{name:'停止',exact:true});await stop.evaluate(el=>window.stableStop=el);const box=await stop.boundingBox();await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await cycles();assert(await stop.evaluate(el=>el===window.stableStop));await page.mouse.up();await sidebar.getByRole('button',{name:'启动',exact:true}).waitFor();
 assert.equal(actions.filter(a=>a.event?.action==='stop').length,1,'Press across refresh stops exactly once');
 const start=sidebar.getByRole('button',{name:'启动',exact:true}),startBox=await start.boundingBox(),startsBefore=actions.filter(a=>a.event?.action==='start').length;
 await page.mouse.move(startBox.x+startBox.width/2,startBox.y+startBox.height/2);await page.mouse.down();await cycles();await page.mouse.move(startBox.x-40,startBox.y+startBox.height+40);await page.mouse.up();await cycles();
 assert.equal(actions.filter(a=>a.event?.action==='start').length,startsBefore,'Dragging out cancels the press');
 await sidebar.getByRole('button',{name:'删除规则',exact:true}).click();await sidebar.getByRole('button',{name:'确认删除',exact:true}).click();await sidebar.getByText('连接远端网络',{exact:true}).waitFor();
 await sidebar.getByRole('button',{name:'＋ 新建',exact:true}).click();await editor.locator('input[name=port]').waitFor();assert.equal(await editor.locator('input[name=port]').inputValue(),'2080');assert(!await editor.locator('input[name=auto]').isChecked());assert.equal(await editor.locator('input[name=host]').count(),0);
 await editor.locator('input[name=name]').fill('SOCKS 检查');await editor.locator('input[name=auto]').check();await editor.getByRole('button',{name:'保存',exact:true}).click();await page.locator('.plugin-view-dialog').waitFor({state:'detached'});
 await sidebar.getByRole('button',{name:'SOCKS 检查'}).click();await sidebar.getByRole('button',{name:'启动',exact:true}).click();await sidebar.getByRole('button',{name:'停止',exact:true}).waitFor();await sidebar.getByText('连通性检查',{exact:true}).click();await sidebar.getByRole('textbox',{name:'检查主机',exact:true}).fill('typed.example');await cycles();
 assert.equal(await sidebar.getByRole('textbox',{name:'检查主机',exact:true}).inputValue(),'typed.example');assert(await sidebar.getByRole('textbox',{name:'检查主机',exact:true}).evaluate(el=>el===document.activeElement));
 assert.deepEqual(errors,[]);console.log('Network UI: real forwarding, unchanged listener/rules after preferences, new SOCKS/fixed-port defaults, form scroll, themes, dirty close, remount and delete passed.');
}finally{await browser.close();await vite.close();child.stdin.end();await new Promise(r=>child.exitCode!==null?r():child.once('exit',r));await new Promise(r=>service.close(r));if(!path.basename(data).startsWith('sodeck-network-ui-'))throw new Error('Unexpected cleanup path');fs.rmSync(data,{recursive:true,force:true});}
