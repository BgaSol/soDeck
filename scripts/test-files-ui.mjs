// Actual file plugin + independent OpenSSH SFTP, exercised through its shipped iframe UI.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error('Fixture build failed');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-files-ui-'));
const remote=path.join(data,'remote 中文');fs.mkdirSync(remote);const filename=path.join(remote,'设置 file.txt');fs.writeFileSync(filename,'\uFEFFbefore\r\n中文\r\n');const local=path.join(data,'download.bin');fs.writeFileSync(path.join(remote,'large.bin'),Buffer.alloc(2*1024*1024,42));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[path.join(data,'app')],{stdio:['pipe','pipe','inherit'],windowsHide:true,env:{...process.env,SODECK_TEST_DOWNLOAD_PATH:local}});
const pending=[],pages=[],errors=[];child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Core fixture exited'));});
readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line);const p=pending.shift();if(!p)throw new Error('Unexpected fixture response');r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];const browser=await chromium.launch({headless:true});
async function pageFor(window){
 const page=await browser.newPage({viewport:{width:1180,height:760}});pages.push(page);page.on('pageerror',e=>errors.push(String(e)));
 await page.exposeBinding('testInvoke',(_,command,args={})=>call(command,{...args,window}));
 await page.route(url+'__plugin__/**',route=>{const relative=new URL(route.request().url()).pathname.split('/').slice(3).join('/');const filename=path.join(process.env.SODECK_TEST_FILES_PLUGIN,relative);return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(filename)]||'application/octet-stream',body:fs.readFileSync(filename)});});
 await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN" data-theme="dopamine"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab">服务器管理</button><div id="server-tabs" class="server-tabs"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:()=>1};window.adapter=(command,args={})=>{if(args.channel){args={...args};delete args.channel;}return window.testInvoke(command,args);};window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),m=>document.querySelector('#notice').textContent=m,window.adapter);await window.manager.start();window.ready=true;</script></body></html>`}));
 await page.goto(url);await page.waitForFunction(()=>window.ready);return page;
}
const wait=async fn=>{const deadline=Date.now()+30000;while(!await fn()){if(Date.now()>deadline)throw new Error('Condition timed out');await new Promise(r=>setTimeout(r,40));}};
try{
 await call('fixture_load_plugin',{directory:process.env.SODECK_TEST_FILES_PLUGIN});
 const catalog=await call('server_catalog');const changed=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'真实文件验收',host:'127.0.0.1',port:Number(process.env.SODECK_TEST_SSH_PORT),username:process.env.SODECK_TEST_SSH_USER,authentication:{type:'privateKey',privateKeyPath:process.env.SODECK_TEST_SSH_KEY,secret:{action:'clear'}},route:[]}}});
 await call('connection',{command:{op:'start',serverId:changed.selectedId}});
 await wait(async()=>{const [v]=await call('connection',{command:{op:'snapshot'}});if(v.question)await call('connection',{command:{op:'answer',attemptId:v.attemptId,questionId:v.question.id,answer:{kind:'hostKey',accept:true}}});if(v.state==='failed')throw new Error(JSON.stringify(v.error));return v.state==='connected';});
 const main=await pageFor('main');let sidebar=main.frameLocator('.component-panel.left iframe');
 // The iframe is hosted in shared surface slots; select by its accessible title.
 sidebar=main.frameLocator('iframe[title^="文件 ·"]');
 await wait(()=>sidebar.locator('input').isEnabled());await sidebar.locator('input[aria-label="远端目录"]').fill(remote.replaceAll('\\','/'));await sidebar.locator('input').press('Enter');
 await sidebar.getByRole('button',{name:'设置 file.txt',exact:false}).click();
 let editor=main.frameLocator('iframe[title^="设置 file.txt"]');let text=editor.locator('textarea');await text.waitFor();await wait(()=>text.isEnabled());assert.equal(await text.inputValue(),'before\n中文\n');
 await text.fill('after\n中文\n');await editor.getByRole('button',{name:'保存',exact:true}).click();await editor.getByRole('status').filter({hasText:'已保存到服务器'}).waitFor();assert.equal(fs.readFileSync(filename,'utf8'),'\uFEFFafter\r\n中文\r\n');
 // Close must flush the iframe, wait for the remote save, then release the view.
 await text.fill('saved by close\n中文\n');
 const viewId=await main.locator('iframe[title^="设置 file.txt"]').evaluate(el=>el.closest('.component-surface').dataset.viewId);
 await main.locator(`[data-component-action="close"][data-key="${viewId}"]`).click();
 await main.getByRole('button',{name:'保存并关闭',exact:true}).click();
 await main.locator(`.component-surface[data-view-id="${viewId}"]`).waitFor({state:'detached'});
 assert.equal(fs.readFileSync(filename,'utf8'),'\uFEFFsaved by close\r\n中文\r\n');
 await sidebar.getByRole('button',{name:'设置 file.txt',exact:false}).click();
 editor=main.frameLocator('iframe[title^="设置 file.txt"]');text=editor.locator('textarea');await wait(()=>text.isEnabled());assert.equal(await text.inputValue(),'saved by close\n中文\n');
 await text.fill('draft survives move\n中文');await main.evaluate(()=>window.manager.flush());
 // Destruction/remount must recover the staged draft, with no remote read replacing it.
 await main.reload();await main.waitForFunction(()=>window.ready);editor=main.frameLocator('iframe[title^="设置 file.txt"]');text=editor.locator('textarea');await wait(async()=>await text.inputValue()==='draft survives move\n中文');
 fs.writeFileSync(filename,'external');await editor.getByRole('button',{name:'保存',exact:true}).click();await editor.getByRole('status').filter({hasText:'已被修改'}).waitFor();assert.equal(fs.readFileSync(filename,'utf8'),'external');
 sidebar=main.frameLocator('iframe[title^="文件 ·"]');await sidebar.locator('.entry').filter({hasText:'large.bin'}).getByRole('button',{name:'↓'}).click();await sidebar.getByRole('status').filter({hasText:'下载完成'}).waitFor();assert.deepEqual(fs.readFileSync(local),Buffer.alloc(2*1024*1024,42));
 const [v]=await call('connection',{command:{op:'snapshot'}});await call('connection',{command:{op:'disconnect',context:v.context}});await main.evaluate(()=>window.manager.refresh());assert.equal(await text.inputValue(),'draft survives move\n中文');
 await editor.getByRole('button',{name:'导出草稿',exact:true}).click();await editor.getByRole('status').filter({hasText:'草稿已导出'}).waitFor();assert.equal(fs.readFileSync(local,'utf8'),'\uFEFFdraft survives move\r\n中文');
 assert.deepEqual(errors,[]);fs.mkdirSync('artifacts/files-ui',{recursive:true});await main.screenshot({path:'artifacts/files-ui/real-files.png'});
 console.log('Actual files UI passed: Chinese paths, BOM/CRLF save, save-and-close, draft remount, external conflict, streamed download, offline draft export');
}catch(error){for(const page of pages){console.error(await page.locator('#notice').textContent());for(const frame of page.frames().slice(1))console.error((await frame.locator('body').innerText()).slice(0,2000));fs.mkdirSync('artifacts/files-ui',{recursive:true});await page.screenshot({path:'artifacts/files-ui/failure.png'});}throw error;
}finally{await browser.close();await vite.close();if(child.exitCode===null){child.stdin.end();await new Promise(resolve=>child.once('exit',resolve));}if(path.dirname(path.resolve(data))!==path.resolve(os.tmpdir())||!path.basename(data).startsWith('sodeck-files-ui-'))throw new Error('Unexpected cleanup');fs.rmSync(data,{recursive:true,force:true});}
