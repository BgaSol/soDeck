// Actual file plugin + independent OpenSSH SFTP, exercised through its shipped iframe UI.
import assert from 'node:assert/strict';
import {fileDocument as editorDocument} from './file-editor-fixture.mjs';
import {transferWorkbench} from './workbench-transfer-fixture.mjs';
import {recordTrace} from './ui-trace.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'vite';import {chromium} from 'playwright';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error('Fixture build failed');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-files-ui-'));
const remote=path.join(data,'remote 中文');fs.mkdirSync(remote);const filename=path.join(remote,'设置 file.txt');fs.writeFileSync(filename,'\uFEFFbefore\r\n中文\r\n');const local=path.join(data,'download.bin');fs.writeFileSync(path.join(remote,'large.bin'),Buffer.alloc(2*1024*1024,42));
for(const directory of ['etc','home','empty'])fs.mkdirSync(path.join(remote,directory));
fs.mkdirSync(path.join(remote,'etc','nginx'));fs.writeFileSync(path.join(remote,'etc','nginx','nginx.conf'),'worker_processes auto;\n');
fs.writeFileSync(path.join(remote,'home','.env'),'TEST=1');
fs.writeFileSync(path.join(remote,'gone.txt'),'temporary');
for(const directory of ['many-a','many-b']){fs.mkdirSync(path.join(remote,directory));for(let i=0;i<215;i++)fs.writeFileSync(path.join(remote,directory,`item-${String(i).padStart(3,'0')}.txt`),'test');}
const upload=path.join(data,'上传 中文'),downloads=path.join(data,'downloads');fs.mkdirSync(upload);fs.mkdirSync(downloads);fs.mkdirSync(path.join(upload,'空目录'));fs.writeFileSync(path.join(upload,'.hidden'),'ui upload');fs.writeFileSync(path.join(upload,'zero.bin'),Buffer.alloc(0));
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[path.join(data,'app')],{stdio:['pipe','pipe','inherit'],windowsHide:true,env:{...process.env,SODECK_TEST_UPLOAD_PATH:upload,SODECK_TEST_DOWNLOAD_PATH:local,SODECK_TEST_DOWNLOAD_DIRECTORY:downloads}});
const pending=[],pages=[],errors=[],requests=[],traces=[];child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Core fixture exited'));});
let clipboard='',pauseRead=false,readRelease;
readline.createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line);const p=pending.shift();if(!p)throw new Error('Unexpected fixture response');r.error?p.reject(new Error(r.error)):p.resolve(r.value);});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];const browser=await chromium.launch({headless:true});
async function pageFor(window){
 const page=await browser.newPage({viewport:{width:1180,height:760}});pages.push(page);traces.push(await recordTrace(page.context(),'files-'+window));page.on('pageerror',e=>errors.push(String(e)));
 await page.exposeBinding('testInvoke',async(_,command,args={})=>{requests.push({command,args});if(command==='component_data'&&args.request?.action==='input.revision')return {revision:'file-ui-fixture',hasFiles:false};if(command==='clipboard_text'){if(args.operation==='read')return clipboard;clipboard=args.text;return null;}if(command==='component_data'&&args.request?.action==='chunk'&&pauseRead){pauseRead=false;const result=await call(command,{...args,window});await new Promise(resolve=>readRelease=resolve);return result;}return call(command,{...args,window});});
 await page.route(url+'__plugin__/**',route=>{const relative=new URL(route.request().url()).pathname.split('/').slice(3).join('/');const filename=path.join(process.env.SODECK_TEST_FILES_PLUGIN,relative);return route.fulfill({contentType:{'.js':'text/javascript','.css':'text/css','.html':'text/html'}[path.extname(filename)]||'application/octet-stream',body:fs.readFileSync(filename)});});
 await page.route(url+'__fixture_events.js',route=>route.fulfill({contentType:'text/javascript',body:fs.readFileSync('scripts/fixtures/events.js')}));
await page.route(url,route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN" data-theme="dopamine"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab">服务器管理</button><div id="server-tabs" class="server-tabs"></div></header><main class="workspace"><div id="server-catalog"></div><div id="workspace-content"></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';import {installFixtureEvents} from '/__fixture_events.js';window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:()=>1};window.adapter=(command,args={})=>{if(args.channel){args={...args};delete args.channel;}return window.testInvoke(command,args);};installFixtureEvents();window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),m=>document.querySelector('#notice').textContent=m,window.adapter);await window.manager.start();window.ready=true;</script></body></html>`}));
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
 const locate=async target=>{const normalized=target.replaceAll('\\','/');await sidebar.getByRole('textbox',{name:'远端路径'}).fill(normalized);await sidebar.getByRole('textbox',{name:'远端路径'}).press('Enter');await wait(()=>sidebar.locator('.tree-row[aria-selected="true"]').evaluateAll((rows,target)=>rows.some(row=>row.dataset.path.endsWith(target)),normalized));};
 // The expanded ancestor tree also contains real /etc and /home on Linux.
 // Match names only inside this test's remote directory, including its root.
 let remotePath=remote.replaceAll('\\','/');
 const testRows=()=>sidebar.locator(`[data-path=${JSON.stringify(remotePath)}],[data-path^=${JSON.stringify(remotePath+'/')}]`);
 const row=name=>sidebar.getByRole('treeitem',{name,exact:true}).and(testRows());
 const expanded=async name=>await row(name).getAttribute('aria-expanded')==='true';
 const tasksIdle=async()=>{await wait(()=>sidebar.locator('.transfer-panel > summary').textContent().then(text=>!text.includes('进行中')));await wait(()=>sidebar.locator('.tree-row[aria-busy="true"]').count().then(count=>count===0));};
 const lists=()=>requests.filter(r=>r.command==='component_data'&&r.args.request?.action==='list').length;
 await wait(()=>sidebar.getByRole('textbox',{name:'远端路径'}).isEnabled());await locate(remote);remotePath=await sidebar.getByRole('treeitem',{name:path.basename(remote),exact:true}).getAttribute('data-path');await row('设置 file.txt').waitFor();
 for(const density of ['compact','comfortable'])for(const fontSize of [11,12,18]){
   const preference=await call('settings_page',{pluginId:'sodeck.files',pageId:'browser'});
   const snapshot=(await call('save_settings',{pluginId:'sodeck.files',pageId:'browser',input:{definitionToken:preference.definitionToken,baseRevision:preference.revision,requestId:`tree-${density}-${fontSize}`,values:{...preference.values,density,fontSize}}})).snapshot;
   await main.evaluate(snapshot=>{const frame=document.querySelector('iframe[title^="文件 ·"]');frame.contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:frame.dataset.surfaceId,type:'settingsChanged',settings:snapshot},new URL(frame.src).origin);},snapshot);
   await wait(()=>row('设置 file.txt').locator('.tree-name').evaluate((el,size)=>parseFloat(getComputedStyle(el).fontSize)===size,fontSize));
   assert(await row('设置 file.txt').evaluate(el=>el.querySelector('.tree-name').getBoundingClientRect().height<=el.getBoundingClientRect().height),`Tree text fits ${fontSize}/${density}`);
 }
 // More than one directory stays expanded; folding cached branches does not read remotely.
 await row('etc').locator('.tree-arrow').click();await row('nginx').waitFor();
 await row('home').locator('.tree-arrow').click();await row('.env').waitFor();assert(await expanded('etc'));
 const reads=lists();await row('etc').locator('.tree-arrow').click();await row('etc').locator('.tree-arrow').click();assert.equal(lists(),reads);assert(await expanded('home'));
 await locate(path.join(remote,'etc','nginx','nginx.conf'));await wait(()=>row('nginx.conf').getAttribute('aria-selected').then(v=>v==='true'));
 assert(await expanded('etc'));assert(await expanded('nginx'));assert(await expanded('home'));
 await sidebar.getByRole('tree').press('ArrowLeft');assert.equal(await row('nginx').getAttribute('aria-selected'),'true');
 // Independently paginated sibling branches can be loaded in alternating order.
 for(const name of ['many-a','many-b'])await row(name).locator('.tree-arrow').click();
 await sidebar.locator('button[data-directory$="/many-a"]').waitFor();await sidebar.locator('button[data-directory$="/many-b"]').waitFor();
 await sidebar.locator('button[data-directory$="/many-a"]').click();
 await wait(()=>sidebar.locator('.tree-row[data-path*="/many-a/"]').count().then(n=>n===200));
 await sidebar.locator('button[data-directory$="/many-b"]').click();
 await wait(()=>sidebar.locator('.tree-row[data-path*="/many-b/"]').count().then(n=>n===200));
 for(const name of ['many-a','many-b']){await sidebar.locator(`button[data-directory$="/${name}"]`).click();await wait(()=>sidebar.locator(`.tree-row[data-path*="/${name}/"]`).count().then(n=>n===215));}
 await locate(path.join(remote,'many-a','item-214.txt'));await wait(()=>sidebar.locator('.tree-row[data-path$="/many-a/item-214.txt"]').getAttribute('aria-selected').then(v=>v==='true'));
 await main.evaluate(()=>window.manager.flush());const pagedScroll=await sidebar.getByRole('tree').evaluate(el=>el.scrollTop);await main.reload();await main.waitForFunction(()=>window.ready);await wait(()=>sidebar.getByRole('textbox',{name:'远端路径'}).isEnabled());
 for(const name of ['many-a','many-b'])assert.equal(await sidebar.locator(`.tree-row[data-path*="/${name}/"]`).count(),215);
 assert.equal(await sidebar.getByRole('tree').evaluate(el=>el.scrollTop),pagedScroll);
 // A failed branch retries locally after the remote directory becomes readable again.
 fs.rmdirSync(path.join(remote,'empty'));await row('empty').locator('.tree-arrow').click();await sidebar.getByRole('button',{name:'读取失败 · 重试'}).waitFor();
 fs.mkdirSync(path.join(remote,'empty'));await sidebar.getByRole('button',{name:'读取失败 · 重试'}).click();await wait(()=>sidebar.getByRole('button',{name:'读取失败 · 重试'}).count().then(n=>n===0));
 for(const name of ['many-a','many-b','empty'])await row(name).locator('.tree-arrow').click();
 await row('gone.txt').click();fs.unlinkSync(path.join(remote,'gone.txt'));await sidebar.getByRole('button',{name:'刷新选中目录',exact:true}).click();await row('gone.txt').waitFor({state:'detached'});assert.equal(await row('remote 中文').getAttribute('aria-selected'),'true');
 await row('设置 file.txt').click({button:'right'});await sidebar.getByRole('menuitem',{name:'复制路径',exact:true}).click();
 await sidebar.getByRole('status').filter({hasText:'路径已复制'}).waitFor();
 assert(clipboard.endsWith('/设置 file.txt'));
 await row('设置 file.txt').click();await sidebar.getByRole('tree').press('Shift+F10');await sidebar.getByRole('menu').waitFor();await main.keyboard.press('Escape');await sidebar.getByRole('menu').waitFor({state:'detached'});
 // A selection spanning destinations confirms the remote target before opening a native picker.
 await row('etc').click({modifiers:[process.platform==='darwin'?'Meta':'Control']});
 assert.equal(await sidebar.locator('.tree-row[aria-selected="true"]').count(),2);
 const choices=()=>requests.filter(r=>r.command==='component_data'&&r.args.request?.action==='input.chooseUpload').length;
 const beforeChoice=choices();await sidebar.getByRole('button',{name:'上传文件',exact:true}).click();
 await sidebar.getByRole('dialog').getByRole('heading',{name:'确认目标目录',exact:true}).waitFor();assert.equal(choices(),beforeChoice);
 await sidebar.getByRole('dialog').getByRole('button',{name:'取消',exact:true}).click();assert.equal(choices(),beforeChoice);
 // Actual plugin task UI: folders, conflicts, remote clipboard, rename and deletion.
 await row('remote 中文').click({button:'right'});await sidebar.getByRole('menuitem',{name:'上传',exact:true}).click();await sidebar.getByRole('menu',{name:'上传',exact:true}).getByRole('menuitem',{name:'文件夹…',exact:true}).click();
 await wait(()=>fs.existsSync(path.join(remote,'上传 中文','空目录'))&&fs.existsSync(path.join(remote,'上传 中文','zero.bin')));
 await sidebar.getByRole('status').filter({hasText:'上传完成'}).waitFor();assert.equal(fs.readFileSync(path.join(remote,'上传 中文','.hidden'),'utf8'),'ui upload');
 const completed=sidebar.locator('.transfer-group').filter({has:sidebar.locator('summary')});
 assert.equal(await completed.getAttribute('open'),null,'Successful tasks default to collapsed summary');
 await completed.locator('summary').click();await new Promise(resolve=>setTimeout(resolve,800));assert.equal(await completed.getAttribute('open'),'','Manual task expansion survives polling');
 await row('remote 中文').click({button:'right'});await sidebar.getByRole('menuitem',{name:'上传',exact:true}).click();await sidebar.getByRole('menu',{name:'上传',exact:true}).getByRole('menuitem',{name:'文件夹…',exact:true}).click();
 await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('button',{name:'跳过',exact:true}).waitFor();
 await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('checkbox').check();
 await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('checkbox').focus();
 await new Promise(resolve=>setTimeout(resolve,500));assert(await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('checkbox').isChecked());
 assert(await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('checkbox').evaluate(el=>el===document.activeElement));
 await sidebar.locator('.transfer-item').filter({hasText:'等待处理同名'}).getByRole('button',{name:'跳过',exact:true}).click();
 await sidebar.getByRole('status').filter({hasText:'上传完成'}).waitFor();
 await locate(path.join(remote,'上传 中文'));await sidebar.locator('.tree-row[data-path$="/remote 中文/上传 中文"]').click({button:'right'});await sidebar.getByRole('menuitem',{name:'下载',exact:true}).click();
 await sidebar.getByRole('status').filter({hasText:'下载完成'}).waitFor();
 assert(fs.existsSync(path.join(downloads,'上传 中文','zero.bin')));assert(fs.statSync(path.join(downloads,'上传 中文','空目录')).isDirectory());
 await row('remote 中文').click({button:'right'});await sidebar.getByRole('menuitem',{name:'新建',exact:true}).click();await sidebar.getByRole('menu',{name:'新建',exact:true}).getByRole('menuitem',{name:'文件夹…',exact:true}).click();
 await sidebar.getByRole('dialog').getByRole('textbox').fill('ui-target');await sidebar.getByRole('dialog').getByRole('button',{name:'确定',exact:true}).click();await wait(()=>fs.existsSync(path.join(remote,'ui-target')));await sidebar.getByRole('status').filter({hasText:'文件任务完成'}).waitFor();
 await locate(path.join(remote,'上传 中文','.hidden'));await sidebar.getByRole('tree').press(process.platform==='darwin'?'Meta+c':'Control+c');await sidebar.getByRole('status').filter({hasText:'已复制'}).waitFor();
 await locate(path.join(remote,'ui-target'));await sidebar.getByRole('tree').focus();await sidebar.getByRole('tree').press(process.platform==='darwin'?'Meta+v':'Control+v');await wait(()=>fs.existsSync(path.join(remote,'ui-target','.hidden')));
 await locate(path.join(remote,'ui-target','.hidden'));await sidebar.getByRole('tree').press('F2');await sidebar.getByRole('dialog').getByRole('textbox').fill('renamed.txt');await sidebar.getByRole('dialog').getByRole('button',{name:'确定',exact:true}).click();await wait(()=>fs.existsSync(path.join(remote,'ui-target','renamed.txt'))&&!fs.existsSync(path.join(remote,'ui-target','.hidden')));
 // Pointer gestures remain inside the file surface; Ctrl/Option requests a copy.
 await sidebar.getByRole('status').filter({hasText:'文件任务完成'}).waitFor();
 await tasksIdle();fs.writeFileSync(path.join(remote,'drag-source.txt'),'pointer transfer');await locate(remote);await sidebar.getByRole('button',{name:'刷新选中目录',exact:true}).click();await row('drag-source.txt').waitFor();await tasksIdle();
 await sidebar.locator('.transfer-panel').evaluate(el=>el.open=false);
 const pointerDrag=async(source,target,copy=false)=>{await tasksIdle();await sidebar.locator('.transfer-panel').evaluate(el=>el.open=false);await source.scrollIntoViewIfNeeded();const a=await source.boundingBox();const modifier=process.platform==='darwin'?'Alt':'Control';if(copy)await main.keyboard.down(modifier);await main.mouse.move(a.x+a.width/2,a.y+a.height/2);await main.mouse.down();await main.mouse.move(a.x+a.width/2+10,a.y+a.height/2,{steps:2});await target.scrollIntoViewIfNeeded();await target.evaluate(el=>{const tree=el.closest('.file-tree');tree.scrollTop=Math.max(0,el.offsetTop-tree.offsetTop-tree.clientHeight/2);});const b=await target.boundingBox();await main.mouse.move(b.x+b.width/2,b.y+b.height/2,{steps:6});await main.mouse.up();if(copy)await main.keyboard.up(modifier);};
 await pointerDrag(row('drag-source.txt'),sidebar.locator('.tree-row[data-path$="/remote 中文/ui-target"]'));
 await wait(()=>!fs.existsSync(path.join(remote,'drag-source.txt'))&&fs.existsSync(path.join(remote,'ui-target','drag-source.txt')));
 await sidebar.getByRole('status').filter({hasText:'文件任务完成'}).waitFor();await locate(path.join(remote,'ui-target','drag-source.txt'));
 await pointerDrag(sidebar.locator('.tree-row[data-path$="/ui-target/drag-source.txt"]'),row('remote 中文'),true);
 await wait(()=>fs.existsSync(path.join(remote,'drag-source.txt'))&&fs.existsSync(path.join(remote,'ui-target','drag-source.txt')));
 await sidebar.getByRole('status').filter({hasText:'文件任务完成'}).waitFor();
 await locate(path.join(remote,'ui-target'));await sidebar.getByRole('tree').press('Delete');await sidebar.getByRole('dialog').getByRole('button',{name:'确定',exact:true}).click();await wait(()=>!fs.existsSync(path.join(remote,'ui-target')));
 await locate(filename);
 await row('设置 file.txt').click();assert.equal(await main.locator('iframe[title^="设置 file.txt"]').count(),0);
 pauseRead=true;await row('设置 file.txt').dblclick();
 let editor=main.frameLocator('iframe[title^="设置 file.txt"]');let text=editorDocument(editor);await text.waitFor();await wait(()=>typeof readRelease==='function');
 assert.equal(await text.isEnabled(),false);
 const loadingSettings=await call('settings_page',{pluginId:'sodeck.files',pageId:'editor'});
 assert.equal(loadingSettings.applications.filter(a=>a.target==='surface').length,0,'Reading a descriptor must not report an editor with applied preferences');
 readRelease();readRelease=undefined;await wait(()=>text.isEnabled());assert.equal(await text.inputValue(),'before\n中文\n');
 await text.fill('after\n中文\n');await editor.getByRole('button',{name:'保存',exact:true}).click();await editor.getByRole('status').filter({hasText:'已保存到服务器'}).waitFor();assert.equal(fs.readFileSync(filename,'utf8'),'\uFEFFafter\r\n中文\r\n');
 fs.mkdirSync('artifacts/files-ui',{recursive:true});
 await row('remote 中文').evaluate(el=>{const tree=el.closest('.file-tree');tree.scrollTop=el.offsetTop-tree.offsetTop-16;});
 await main.screenshot({path:'artifacts/files-ui/tree-workbench.png'});
 // Close must flush the iframe, wait for the remote save, then release the view.
 await text.fill('saved by close\n中文\n');
 const viewId=await main.locator('iframe[title^="设置 file.txt"]').evaluate(el=>el.closest('.component-surface').dataset.viewId);
 await main.locator(`[data-component-action="close"][data-key="${viewId}"]`).click();
 await main.getByRole('button',{name:'保存并关闭',exact:true}).click();
 await main.locator(`.component-surface[data-view-id="${viewId}"]`).waitFor({state:'detached'});
 assert.equal(fs.readFileSync(filename,'utf8'),'\uFEFFsaved by close\r\n中文\r\n');
 await row('设置 file.txt').dblclick();
 editor=main.frameLocator('iframe[title^="设置 file.txt"]');text=editorDocument(editor);await wait(()=>text.isEnabled());assert.equal(await text.inputValue(),'saved by close\n中文\n');
 await text.fill('draft survives move\n中文');await main.evaluate(()=>window.manager.flush());
 await editor.getByRole('button',{name:'下载远端',exact:true}).click();await editor.getByRole('status').filter({hasText:'下载完成'}).waitFor();
 assert.equal(fs.readFileSync(local,'utf8'),'\uFEFFsaved by close\r\n中文\r\n','Remote download excludes current draft');
 const remoteBeforeSettings=fs.readFileSync(filename);
 await text.evaluate(el=>{window.settingsEditor=el.sodeckEditor;el.sodeckEditor.setSelection(9,2);});
 const preference=await call('settings_page',{pluginId:'sodeck.files',pageId:'editor'});
 const savedPreference=(await call('save_settings',{pluginId:'sodeck.files',pageId:'editor',input:{definitionToken:preference.definitionToken,baseRevision:preference.revision,requestId:'editor-display',values:{...preference.values,fontSize:18,lineHeight:1.6,wordWrap:true,tabSize:8,font:{source:'preset',id:'source-code-pro'}}}})).snapshot;
 await main.evaluate(snapshot=>{const frame=document.querySelector('iframe[title^="设置 file.txt"]');frame.contentWindow.postMessage({channel:'sodeck-ui-v2',surfaceId:frame.dataset.surfaceId,type:'settingsChanged',settings:snapshot},new URL(frame.src).origin);},savedPreference);
 await wait(()=>text.evaluate(el=>el.sodeckEditor.editor.getRawOptions().fontSize===18&&el.sodeckEditor.editor.getRawOptions().fontFamily.includes('SoDeckFont')));
 assert.equal(await text.inputValue(),'draft survives move\n中文');assert.deepEqual(fs.readFileSync(filename),remoteBeforeSettings);
 assert(await text.evaluate(el=>el.sodeckEditor===window.settingsEditor));assert.deepEqual(await text.evaluate(el=>el.sodeckEditor.selection()),{anchor:9,active:2});
 await main.evaluate(()=>window.manager.flush());assert.deepEqual(fs.readFileSync(filename),remoteBeforeSettings);
 const stagedBeforeSearch=requests.filter(r=>r.command==='component_data'&&r.args.request?.action==='stage').length;
 await editor.getByLabel('文件语言').selectOption('ini');await editor.getByRole('button',{name:'搜索',exact:true}).click();await editor.getByRole('textbox',{name:'查找当前文件'}).fill('draft');await wait(()=>editor.locator('.editor-find-count').textContent().then(text=>text.endsWith('1 处')));await main.evaluate(()=>window.manager.flush());assert.equal(requests.filter(r=>r.command==='component_data'&&r.args.request?.action==='stage').length,stagedBeforeSearch);
 const savedScroll=await sidebar.getByRole('tree').evaluate(el=>el.scrollTop);
 // Destruction/remount must recover the staged draft, with no remote read replacing it.
 await main.reload();await main.waitForFunction(()=>window.ready);editor=main.frameLocator('iframe[title^="设置 file.txt"]');text=editorDocument(editor);await wait(async()=>await text.inputValue()==='draft survives move\n中文');
 assert.equal(await editor.getByRole('textbox',{name:'查找当前文件'}).inputValue(),'draft');assert.equal(await editor.getByLabel('文件语言').inputValue(),'ini');
 fs.writeFileSync(filename,'external');await editor.getByRole('button',{name:'保存',exact:true}).click();await editor.getByRole('status').filter({hasText:'已被修改'}).waitFor();assert.equal(fs.readFileSync(filename,'utf8'),'external');
 sidebar=main.frameLocator('iframe[title^="文件 ·"]');await wait(()=>sidebar.getByRole('textbox',{name:'远端路径'}).isEnabled());
 assert(await expanded('home'));assert(await expanded('nginx'));assert.equal(await row('设置 file.txt').getAttribute('aria-selected'),'true');
 // Ancestor directories can lose temporary entries while the view is closed.
 // Browsers clamp a restored scroll offset to the remaining content's range.
 const restoredScroll=await sidebar.getByRole('tree').evaluate(el=>({top:el.scrollTop,max:Math.max(0,el.scrollHeight-el.clientHeight)}));
 assert.equal(restoredScroll.top,Math.min(savedScroll,restoredScroll.max));
 // Theme colors reach the real iframe; narrow sidebars stay inside their bounds.
 fs.mkdirSync('artifacts/files-ui',{recursive:true});
 await main.mouse.move(500,20);
 for(const theme of ['light','dark','dopamine']){
  await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
  await wait(async()=>{const host=await main.locator('html').evaluate(el=>getComputedStyle(el).getPropertyValue('--file-tree-bg').trim());return sidebar.locator('html').evaluate(el=>getComputedStyle(el).getPropertyValue('--file-tree-bg').trim()).then(value=>value===host);});
  const expected=await main.locator('html').evaluate(el=>({panel:getComputedStyle(el).getPropertyValue('--panel').trim(),primary:getComputedStyle(el).getPropertyValue('--sd-primary').trim()}));
  await wait(()=>editor.locator('html').evaluate((el,panel)=>getComputedStyle(el).getPropertyValue('--panel').trim()===panel,expected.panel));
  const rgb=hex=>{let digits=hex.slice(1);if(digits.length===3)digits=[...digits].map(v=>v+v).join('');return 'rgb('+digits.match(/../g).map(v=>parseInt(v,16)).join(', ')+')';};
  await wait(()=>editor.locator('.editor-toolbar button').evaluateAll((bs,colors)=>bs[0]&&getComputedStyle(bs[0]).backgroundColor===colors[0]&&getComputedStyle(bs[1]).backgroundColor===colors[1],[rgb(expected.primary),rgb(expected.panel)]));
  const buttons=await editor.locator('.editor-toolbar button').evaluateAll(bs=>bs.map(b=>getComputedStyle(b).backgroundColor));
  assert.equal(buttons[0],rgb(expected.primary));assert.equal(buttons[1],rgb(expected.panel));
  const strokes=await editor.locator('.editor-toolbar .primary').evaluate(b=>({border:getComputedStyle(b).borderTopColor,stroke:getComputedStyle(b,'::before').borderTopWidth,weight:getComputedStyle(b).fontWeight}));
  assert.equal(strokes.border,'rgba(0, 0, 0, 0)');assert.equal(strokes.stroke,theme==='dopamine'?'2px':'1px');assert.equal(strokes.weight,theme==='dopamine'?'600':'500');
  await main.screenshot({path:`artifacts/files-ui/tree-${theme}.png`});
 }
 const board=await main.locator('.server-workbench').getAttribute('data-id');
 const handle=main.getByRole('separator',{name:'左侧面板宽度'}),rect=await handle.boundingBox();
 await main.mouse.move(rect.x+3,rect.y+100);await main.mouse.down();await main.mouse.move(rect.x-97,rect.y+100);await main.mouse.up();
 await wait(()=>sidebar.locator('body').evaluate(el=>innerWidth<=180));
 assert(await sidebar.locator('body').evaluate(el=>el.scrollWidth<=innerWidth&&document.querySelector('.file-tree').scrollWidth<=innerWidth));
 await main.screenshot({path:'artifacts/files-ui/tree-narrow.png'});
 const narrow=await handle.boundingBox();await main.mouse.move(narrow.x+3,narrow.y+100);await main.mouse.down();await main.mouse.move(narrow.x+123,narrow.y+100);await main.mouse.up();
 // Moving the entire connected workspace restores file navigation and the editor draft.
 const moved=await transferWorkbench(call,{window:'main',id:board,target:'workspace-browser'},{sourcePage:main,openTarget:()=>pageFor('workspace-browser')});
 const movedSidebar=moved.frameLocator('iframe[title^="文件 ·"]');await wait(()=>movedSidebar.getByRole('textbox',{name:'远端路径'}).isEnabled());
 assert.equal(await movedSidebar.getByRole('treeitem',{name:'nginx',exact:true}).getAttribute('aria-expanded'),'true');
 assert.equal(await movedSidebar.getByRole('treeitem',{name:'设置 file.txt',exact:true}).getAttribute('aria-selected'),'true');
 assert.equal(await editorDocument(moved.frameLocator('iframe[title^="设置 file.txt"]')).inputValue(),'draft survives move\n中文');
 await transferWorkbench(call,{window:'workspace-browser',id:board,target:'main'},{sourcePage:moved,openTarget:async()=>main});
 sidebar=main.frameLocator('iframe[title^="文件 ·"]');await wait(()=>sidebar.getByRole('textbox',{name:'远端路径'}).isEnabled());editor=main.frameLocator('iframe[title^="设置 file.txt"]');text=editorDocument(editor);await wait(()=>text.isEnabled());
 await row('large.bin').click({button:'right'});await sidebar.getByRole('menuitem',{name:'下载',exact:true}).click();
 const replaceDownload=sidebar.locator('.transfer-item').filter({hasText:'等待处理同名 · 下载 · large.bin'}).getByRole('button',{name:'覆盖',exact:true});
 const downloadId=(await replaceDownload.getAttribute('data-control')).slice(0,-'/覆盖'.length);
 await replaceDownload.click();await wait(()=>fs.existsSync(local)&&fs.statSync(local).size===2*1024*1024);assert.deepEqual(fs.readFileSync(local),Buffer.alloc(2*1024*1024,42));
 // File writes finish before both surfaces receive the task's completion notification.
 await tasksIdle();await editor.locator(`[data-control="${downloadId}/查看结果"]`).waitFor({state:'attached'});
 await editor.getByRole('status').filter({hasText:'下载完成'}).waitFor();
 const [v]=await call('connection',{command:{op:'snapshot'}});await call('connection',{command:{op:'disconnect',context:v.context}});await main.evaluate(()=>window.manager.refresh());assert.equal(await text.inputValue(),'draft survives move\n中文');
 await editor.getByRole('button',{name:'导出草稿',exact:true}).click();await editor.getByRole('status').filter({hasText:'草稿已导出'}).waitFor();assert.equal(fs.readFileSync(local,'utf8'),'\uFEFFdraft survives move\r\n中文');
 assert.deepEqual(errors,[]);fs.mkdirSync('artifacts/files-ui',{recursive:true});await main.screenshot({path:'artifacts/files-ui/real-files.png'});
 console.log('Actual files UI passed: folder upload/download, conflict apply-all, remote clipboard, create/rename/delete, pointer move/copy, multi-branch tree, pagination, themes/narrow layout, cross-window draft, BOM/CRLF save, save-and-close, conflict, streamed download and offline export');
}catch(error){for(const finish of traces)await finish(true);for(const page of pages){console.error(await page.locator('#notice').textContent());for(const frame of page.frames().slice(1)){console.error(await frame.locator('[role="status"]').allTextContents());console.error(await frame.locator('.transfer-item').allTextContents());}fs.mkdirSync('artifacts/files-ui',{recursive:true});await page.screenshot({path:'artifacts/files-ui/failure.png'});}throw error;
}finally{for(const finish of traces)await finish(false);await browser.close();await vite.close();if(child.exitCode===null){child.stdin.end();await new Promise(resolve=>child.once('exit',resolve));}if(path.dirname(path.resolve(data))!==path.resolve(os.tmpdir())||!path.basename(data).startsWith('sodeck-files-ui-'))throw new Error('Unexpected cleanup');fs.rmSync(data,{recursive:true,force:true});}
