// Real workspace DOM -> test bridge -> actual Rust ownership. Browser windows stand in for native surfaces.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { createServer } from "vite";
import { chromium } from "playwright";
import { assertMenuLayout } from "./menu-layout.mjs";
import { recordTrace } from "./ui-trace.mjs";

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--bin", "workspace-ui-fixture", "--locked"], { stdio: "inherit" });
if (build.status !== 0) throw new Error("Catalog test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-ui-"));
const pluginRoot = path.join(directory, "ui-plugin");fs.mkdirSync(pluginRoot);
const suffix = process.platform === "win32" ? ".exe" : "";
fs.copyFileSync(`src-tauri/target/debug/workspace-ui-fixture${suffix}`, path.join(pluginRoot, `workspace-ui-fixture${suffix}`));
if (process.platform !== 'win32') fs.chmodSync(path.join(pluginRoot, `workspace-ui-fixture${suffix}`), 0o755);
fs.cpSync("scripts/fixtures/workspace-ui/ui", path.join(pluginRoot,"ui"), {recursive:true});
const metadata = JSON.parse(fs.readFileSync("scripts/fixtures/workspace-ui/manifest.json","utf8"));
// Exercise the product's default target with a test process, never a distributed plugin.
metadata.id = 'test.workbench';
metadata.ui.views.find(v=>v.id==='forwarding').backgroundOnWorkspace=true;
metadata.ui.views.push({id:'alternative',title:'其他工具',icon:'plug',container:'editor',instancePolicy:'multiple',createAction:'新建其他工具'});
for(let index=0;index<20;index++)metadata.ui.views.push({id:`extra${index}`,title:`扩展测试组件 ${index}`,icon:'plug',container:'sidebar',instancePolicy:'perWorkspace',defaultSide:'left'});
const target = spawnSync("rustc",["-vV"],{encoding:"utf8"}).stdout.match(/^host: (.+)$/m)[1];
fs.writeFileSync(path.join(pluginRoot,"plugin.json"),JSON.stringify({...metadata,target,entry:`workspace-ui-fixture${suffix}`}));
const executable = path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform === "win32" ? ".exe" : ""}`);
function startBridge() {
  const child = spawn(executable, [directory], { stdio: ["pipe", "pipe", "inherit"], windowsHide: true });
  const queue = [];
  readline.createInterface({ input: child.stdout }).on("line", line => {
    const response = JSON.parse(line); const request = queue.shift();
    if (response.error) request.reject(new Error(response.error)); else request.resolve(response.value);
  });
  child.on("exit", () => { for (const request of queue) request.reject(new Error("Test bridge exited")); });
  return {
    call(command, args = {}) { return new Promise((resolve, reject) => { queue.push({ resolve, reject }); child.stdin.write(JSON.stringify({ command, args }) + "\n"); }); },
    stop() { return child.exitCode !== null ? Promise.resolve() : new Promise(resolve => { child.once("exit", resolve); child.stdin.end(); }); },
  };
}
const bridge = startBridge();
const vite = await createServer({ server: { host: "127.0.0.1", port: 0 } }); await vite.listen();
const url = vite.resolvedUrls.local[0];
const browser = await chromium.launch({ headless: true, ...(process.env.SODECK_BROWSER_CHANNEL ? { channel: process.env.SODECK_BROWSER_CHANNEL } : {}) });
const traces = [];
const pages = [], errors = [], checks = [], dragCalls = [], dropTargets = new Map(), drags = new Map();
const surfaces = new Map();
const call = (command, args={}) => bridge.call(command,args);
let createGate,createData,openEditorGate,layoutGate,lostSubmission;
const actionCalls=[];
const layoutCalls=[];
const projection = window => call("workspace_snapshot",{window});
const componentState = window => call("component_ui",{window,operation:"snapshot",args:{}});
async function makePage(window) {
  const page = await browser.newPage({ viewport: {width:1100,height:720} }); pages.push(page);
  traces.push(await recordTrace(page.context(), window));
  page.on("pageerror", error=>errors.push(String(error)));
  await page.exposeBinding("workspaceInvoke",async (_,command,args={})=>{
    if(command==='workspace_surface')surfaces.set(window,args.surface);
    if(command==='component_ui'&&args.operation==='command'){
      actionCalls.push(args.args.op);
      if(args.args.op==='submit'&&args.args.actionKey==='test.workbench:new-page'){
        if(createGate){const rejection=await createGate();if(rejection)return rejection;}
        if(createData)args.args.payload=createData;
        if(lostSubmission){
          const loss=lostSubmission;lostSubmission=undefined;
          if(loss==='after')await call(command,{...args,window});
          throw new Error('隔离测试：提交回执丢失');
        }
      }
    }
    if(command==='component_ui'&&args.operation==='action'&&args.args?.action?.op==='open'&&args.args.action.key==='test.workbench:editor'&&openEditorGate) await openEditorGate();
    if(command==='workspace_layout'){layoutCalls.push(args);if(layoutGate)await layoutGate();}
    if(command==="workspace_drag_start") { dragCalls.push({command,args}); drags.set(window,args.id);return `browser-${window}`; }
    if(command==="workspace_drag_sample") return dropTargets.get(window)??{kind:"invalid"};
    if(command==="workspace_drag_release") { dragCalls.push({command,args});return null; }
    if(command==="workspace_drag_finish") { dragCalls.push({command,args}); const target=dropTargets.get(window); const id=drags.get(window);drags.delete(window); if(!args.cancel&&target?.kind==="strip")return call("workspace_move",{window,id,target:target.windowId,index:target.index});return null; }
    return call(command,{...args,window});
  });
  await page.route(url+'__plugin__/**',route=>{
    const relative=decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/');
    const file=path.join(pluginRoot,relative);
    const types={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
    return route.fulfill({contentType:types[path.extname(file)]||'application/octet-stream',body:fs.readFileSync(file)});
  });
  await page.route(url, route=>route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><head><meta charset="utf-8"></head><body><div id="app"></div><div id="notice"></div><script type="module">
import '/style.css';import {Workspaces} from '/workspaces.ts';import {Shell} from '/shell.ts';
const callbacks=new Map(),listeners=new Map();let nativeId=0;
window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:(_event,id)=>{callbacks.delete(listeners.get(id));listeners.delete(id);}};
window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href,transformCallback:callback=>{const id=++nativeId;callbacks.set(id,callback);return id;},invoke:async(command,args)=>{if(command==='plugin:event|listen'){const id=++nativeId;listeners.set(id,args.handler);return id;}if(command==='plugin:event|unlisten')return;return window.workspaceInvoke(command,args);}};
document.body.classList.toggle('secondary-window','${window}'!=='main');
window.shell=new Shell(document.querySelector('#app'),'${window}'==='main');
shell.catalog.innerHTML='<input id="catalog-draft" value="draft retained">';
window.manager=new Workspaces(shell.tabs,shell.content,message=>document.querySelector('#notice').textContent=message,window.workspaceInvoke);
shell.attach(manager,{setNavigation(){}});await manager.start();window.ready=true;
</script></body></html>`}));
  await page.goto(url); await page.waitForFunction(()=>window.ready); return page;
}
async function refresh() { for (const page of pages) await page.evaluate(()=>window.manager.refresh()); }
const select = async (page,id)=>{ await page.locator(`.server-tab-select[data-id="${id}"]`).click(); await page.waitForFunction(id=>document.querySelector(`.server-tab-select[data-id="${id}"]`)?.getAttribute("aria-selected")==="true",id); };
const menu = (page,id)=>page.locator(`.server-tab-select[data-id="${id}"]`).click({button:"right"});
const panel = (page,side)=>page.locator(`.server-workbench:visible [data-component-action="toggle"][data-key="test.workbench:${side==='left'?'files':'forwarding'}"]`);
async function open(name,index) {
  const changed=await call("change_catalog",{revision:(await call("server_catalog")).revision,change:{op:"saveServer",draft:{name,host:"same.invalid",port:22,username:"ops",authentication:{type:"password",secret:{action:"clear"}},route:[]}}});
  return call("workspace_fixture_open",{serverId:changed.selectedId,connectionId:`fixture-${index}`,endpoint:`ops@${index}.invalid:22`});
}
const wait=async predicate=>{const deadline=Date.now()+10000;while(!await predicate()){if(Date.now()>deadline)throw new Error('Pressed-control state did not settle');await new Promise(resolve=>setTimeout(resolve,20));}};
try {
  const main=await makePage("main");
  assert.equal(await main.locator(".server-tab").count(),0); assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  const blank = await main.locator('.drag-region').boundingBox();
  assert.deepEqual({ x: surfaces.get('main').x, y: surfaces.get('main').y, width: surfaces.get('main').width, height: surfaces.get('main').height }, blank, 'Empty main window reports its title-bar blank area as the native tab drop target');
  assert(blank.width >= 32, 'Empty-window drop target remains usable without visible tab spacing');
  await main.setViewportSize({ width: 720, height: 480 });
  const narrowBlank = await main.locator('.drag-region').boundingBox();
  await wait(() => surfaces.get('main')?.width === narrowBlank.width);
  assert.deepEqual({ x: surfaces.get('main').x, y: surfaces.get('main').y, width: surfaces.get('main').width, height: surfaces.get('main').height }, narrowBlank, 'Resizing an empty window updates its native drop target');
  await main.setViewportSize({ width: 1100, height: 720 });
  checks.push("普通启动只显示目录，不伪造连接");
  const unavailable=await open('无插件',0);await refresh();await select(main,unavailable.context.workspaceId);
  const unavailableButton=main.locator('.server-workbench:visible [data-component-action="create"]');
  assert(await unavailableButton.isDisabled());assert.match(await unavailableButton.getAttribute('title'),/默认行为|未设置/);
  await main.locator(`.tab-close-button[data-id="${unavailable.context.workspaceId}"]`).click();await main.locator(`.server-tab[data-id="${unavailable.context.workspaceId}"]`).waitFor({state:"detached"});await refresh();
  await call('change_catalog',{revision:(await call('server_catalog')).revision,change:{op:'deleteServer',id:unavailable.context.serverId}});
  await call("fixture_load_plugin",{directory:pluginRoot});
  const a=await open("生产 Web",1),b=await open("测试环境",2),c=await open("长名称服务器-".repeat(12),3);
  const aid=a.context.workspaceId,bid=b.context.workspaceId,cid=c.context.workspaceId; await refresh();
  assert.equal(await main.locator('.component-surface iframe[title^="网络转发"]').count(),0,'Unrevealed background views must not create an iframe');
  await main.waitForFunction(() => document.querySelectorAll('.server-tab').length === 3);
  const strip = await main.locator('#server-tabs').boundingBox();
  await wait(() => surfaces.get('main')?.width === strip.width);
  assert.deepEqual({ x: surfaces.get('main').x, y: surfaces.get('main').y, width: surfaces.get('main').width, height: surfaces.get('main').height }, strip, 'Populated windows keep the actual tab strip as their native drop target');
  await select(main,aid); await panel(main,"left").click(); await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) [data-component-action="toggle"][data-key="test.workbench:files"]').getAttribute('aria-expanded')==='false'); await panel(main,"right").click(); await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) [data-component-action="toggle"][data-key="test.workbench:forwarding"]').getAttribute('aria-expanded')==='true');
  await select(main,bid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"true"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"false");
  await select(main,aid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"false"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"true");
  assert((await main.locator("#shell-status .shell-endpoint").innerText()).includes("1.invalid"));
  checks.push("固定上下文、服务器切换与侧栏状态各自保持");
  const board=main.locator(`.server-workbench[data-id="${aid}"]`);
  const pressRefresh=async locator=>{const rect=await locator.boundingBox();await locator.evaluate(el=>window.pressedChrome=el);await main.mouse.move(rect.x+rect.width/2,rect.y+rect.height/2);await main.mouse.down();await main.evaluate(()=>window.manager.refresh());assert.equal(await locator.evaluate(el=>el===window.pressedChrome&&el.isConnected),true);await main.mouse.up();};
  for(const side of ['left','right']){const toggle=panel(main,side),before=await toggle.getAttribute('aria-expanded');await pressRefresh(toggle);await wait(async()=>await toggle.getAttribute('aria-expanded')!==before);await toggle.click();await wait(async()=>await toggle.getAttribute('aria-expanded')===before);}
  const stableCollapse=board.getByRole('button',{name:'收起网络转发',exact:true});
  await pressRefresh(stableCollapse);await wait(async()=>await panel(main,'right').getAttribute('aria-expanded')==='false');await panel(main,'right').click();
  const serverMenu=main.locator(`.server-tab-select[data-id="${aid}"]`);await serverMenu.click({button:"right"});await main.getByRole("menu").waitFor();await serverMenu.click({button:"right"});assert.equal(await main.getByRole("menu").count(),1);await main.keyboard.press("Escape");assert.equal(await main.locator(".tab-menu-button").count(),0);
  checks.push('真实按压跨工作台刷新仍触发一次，两侧入口节点稳定，右键再次打开标签菜单');
  fs.mkdirSync('artifacts/menu-layout',{recursive:true});
  for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[1100,720],[720,480]]){
    await main.setViewportSize({width,height});
    await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    const tabMenu=main.locator(`.server-tab-select[data-id="${aid}"]`);
    const tabBounds=await tabMenu.boundingBox();
    const point={x:Math.round(tabBounds.x+tabBounds.width/2),y:Math.round(tabBounds.y+tabBounds.height/2),width:0,height:0};await main.mouse.click(point.x,point.y,{button:"right"});await assertMenuLayout(main,point,{point:true});await main.screenshot({path:`artifacts/menu-layout/server-${theme}-${width}.png`});await main.keyboard.press("Escape");
    assert.equal(await board.locator('[data-component-action="panel-menu"]').count(),0);
    assert.equal(await board.getByRole('button',{name:'收起网络转发',exact:true}).count(),1);
    if(theme==='dopamine'&&width===1100)await main.screenshot({path:'artifacts/menu-layout/panel-header.png'});
    const more=board.getByRole('button',{name:'左侧更多组件',exact:true});
    const moreBounds=await more.boundingBox();
    await more.click();const popup=await assertMenuLayout(main,moreBounds,{long:true});
    await popup.getByRole('textbox').fill('扩展测试组件 19');
    assert.equal(await popup.locator('.component-menu-row:visible').count(),1);
    assert.equal(await popup.locator('.component-menu-list').evaluate(el=>el.scrollHeight>el.clientHeight),false);
    if(theme==='dopamine'&&width===1100)await main.screenshot({path:'artifacts/menu-layout/filtered-menu.png'});
    await main.keyboard.press('Escape');assert(await more.evaluate(el=>el===document.activeElement));
    const rightMore=board.getByRole('button',{name:'右侧更多组件',exact:true});
    const rightBounds=await rightMore.boundingBox();
    await rightMore.click();await assertMenuLayout(main,rightBounds,{long:true});
    assert.equal(await main.getByRole('menu').locator('.component-menu-row').count(),metadata.ui.views.filter(v=>v.container==='sidebar').length,'Both sides expose the same complete tool list');
    await board.locator('.left-rail').click({position:{x:10,y:300}});assert.equal(await main.getByRole('menu').count(),0);
  }
  await main.setViewportSize({width:1100,height:720});
  checks.push('三主题及窄窗口：面板仅保留名称和收起按钮，页签右键与更多组件菜单靠近入口；短菜单无滚动条，长列表仅纵向滚动，搜索后消除滚动，Esc 返回焦点及外部关闭');
  const viewFrame=(page,wid,vid)=>page.frameLocator(`.component-surface[data-view-id="${vid}"] iframe`);
  const flush=page=>page.evaluate(()=>window.manager.flush());
  let aw=(await componentState("main")).workspaces[aid];
  assert.equal(aw.views.filter(v=>v.definitionKey.endsWith(':terminal')).length,1);
  const firstTerminal=aw.views.find(v=>v.definitionKey.endsWith(':terminal')).id;
  const terminalOne=viewFrame(main,aid,firstTerminal).getByRole('textbox',{name:'终端输入草稿'});
  await terminalOne.fill('terminal A one');await flush(main);
  const failedFrame=await main.locator(`.component-surface[data-view-id="${firstTerminal}"] iframe`).elementHandle();
  await main.evaluate(({aid,id})=>{const components=manager.components,surface=components.shells.get(aid).surfaces.get(id);components.surfaceError(surface,'测试界面加载失败');},{aid,id:firstTerminal});
  await main.getByRole('button',{name:'重试显示',exact:true}).click();
  await terminalOne.waitFor();assert.equal(await terminalOne.inputValue(),'terminal A one');
  assert.equal(await failedFrame.evaluate(el=>el.isConnected),false,'Error retry remounts only the failed surface');
  await board.locator('[data-component-action="create"]').click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .component-tab-list`).childElementCount===2,aid);
  aw=(await componentState("main")).workspaces[aid];
  const secondTerminal=aw.views.filter(v=>v.definitionKey.endsWith(':terminal')).find(v=>v.id!==firstTerminal).id;
  await viewFrame(main,aid,secondTerminal).getByRole('textbox').fill('terminal A two');await flush(main);
  await board.locator(`[data-component-action="select"][data-key="${firstTerminal}"]`).click();
  assert.equal(await board.locator('.workbench-rail [data-key$=":terminal"]').count(),0,'Legacy terminal locateEntry is ignored by the tool rail');
  await board.getByRole('button',{name:'左侧更多组件',exact:true}).click();
  assert.equal(await main.getByRole('menuitem').filter({hasText:'终端'}).count(),0,'Editor views stay out of more tools');
  await main.keyboard.press('Escape');
  assert.equal((await componentState("main")).workspaces[aid].editor,firstTerminal);
  assert.equal(await terminalOne.inputValue(),'terminal A one');
  await panel(main,'left').click();
  aw=(await componentState("main")).workspaces[aid];
  const browserView=aw.views.find(v=>v.definitionKey.endsWith(':files')).id;
  const fileBrowser=viewFrame(main,aid,browserView);
  await fileBrowser.getByRole('button',{name:'nginx.conf',exact:true}).dblclick();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .component-tab-list`).textContent.includes('nginx.conf'),aid);
  aw=(await componentState("main")).workspaces[aid];const editorId=aw.views.find(v=>v.definitionKey.endsWith(':editor')).id;
  const editorFrame=viewFrame(main,aid,editorId),editor=editorFrame.getByRole('textbox',{name:'文件编辑草稿'});
  await editor.fill('draft for server A');await flush(main);
  let releaseEditor;
  openEditorGate=()=>new Promise(resolve=>{releaseEditor=resolve;});
  await fileBrowser.locator('body').evaluate(()=>{const request=sodeck.request.bind(sodeck);sodeck.request=(method,params)=>{const response=request(method,params);if(method==='openView')window.editorOpenResponse=response;return response;};});
  await fileBrowser.getByRole('button',{name:'nginx.conf',exact:true}).dblclick();await wait(()=>Boolean(releaseEditor));
  await select(main,bid);releaseEditor();openEditorGate=undefined;await fileBrowser.locator('body').evaluate(()=>window.editorOpenResponse);
  await main.evaluate(()=>window.manager.refresh());assert.equal((await projection('main')).window.activeWorkspace,bid);
  assert.equal(await editorFrame.locator('textarea:focus').count(),0);await select(main,aid);
  checks.push('插件请求打开文件期间切换工作区，迟到的打开结果不抢焦点');
  const beforeCreate=(await componentState('main')).workspaces[aid].views.length;
  await board.locator('[data-component-action="create"]').click();
  await main.waitForFunction(({id,count})=>document.querySelector(`.server-workbench[data-id="${id}"] .component-tab-list`).childElementCount===count,{id:aid,count:4});
  aw=(await componentState('main')).workspaces[aid];
  assert.equal(aw.views.length,beforeCreate+1);
  const added=aw.views.find(v=>v.id===aw.editor);assert.equal(added.definitionKey,'test.workbench:terminal');
  assert.equal(await main.getByRole('menu').count(),0);
  assert.equal(await editorFrame.locator('textarea').inputValue(),'draft for server A');
  await viewFrame(main,aid,added.id).locator('textarea:focus').waitFor({timeout:5000});
  await board.locator(`[data-component-action="close"][data-key="${added.id}"]`).click();
  await board.locator(`[data-component-action="close"][data-key="${added.id}"]`).waitFor({state:"detached"});
  await board.locator(`[data-component-action="select"][data-key="${editorId}"]`).click();
  checks.push('默认终端未加载时禁用；多个创建类型或正在编辑文件时，加号仍只新建一个独立终端并聚焦，保留文件草稿');
  let releaseCreate,createRequests=0;
  const failedCreate=new Promise(resolve=>{releaseCreate=()=>resolve({error:{code:'resourceLimit',message:'隔离测试：终端创建失败'}});});
  createGate=()=>{createRequests++;return failedCreate;};
  await board.locator('[data-component-action="create"]').click();
  assert(await board.locator('[data-component-action="create"]').isDisabled());
  await board.locator('[data-component-action="create"]').dispatchEvent('click');
  releaseCreate();
  await main.waitForFunction(()=>document.querySelector('#notice').textContent.includes('终端创建失败'));
  assert.equal(createRequests,1);assert.equal((await componentState('main')).workspaces[aid].views.length,beforeCreate);
  await wait(()=>board.locator('[data-component-action="create"]').isEnabled());
  const lateCreate=new Promise(resolve=>{releaseCreate=resolve;});createGate=()=>lateCreate;
  await board.locator('[data-component-action="create"]').click();
  await select(main,bid);
  releaseCreate();createGate=undefined;
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .component-tab-list`).childElementCount===4,aid);
  const late=(await componentState('main')).workspaces[aid];
  assert.equal((await projection('main')).window.activeWorkspace,bid);
  assert.equal(await main.locator(`.component-surface[data-view-id="${late.editor}"] iframe`).evaluate(el=>el===document.activeElement),false);
  await select(main,aid);await board.locator(`[data-component-action="close"][data-key="${late.editor}"]`).click();await board.locator(`[data-component-action="close"][data-key="${late.editor}"]`).waitFor({state:"detached"});
  await board.locator(`[data-component-action="select"][data-key="${editorId}"]`).click();
  checks.push('创建中禁用且防重复，失败可重试；创建期间切换服务器，迟到结果不抢工作区和焦点');
  const actionBefore=(await componentState('main')).workspaces[aid].views.length;
  createData={none:true};await board.locator('[data-component-action="create"]').click();await wait(()=>board.locator('[data-component-action="create"]').isEnabled());
  assert.equal((await componentState('main')).workspaces[aid].views.length,actionBefore,'A behavior that returns no page must not get a host-created page');
  const sentBefore=actionCalls.filter(op=>op==='submit').length;createData={delayMs:6200};
  await board.locator('[data-component-action="create"]').click();await wait(()=>board.locator('[data-component-action="create"]').getAttribute('title').then(title=>title==='结果尚未确认'));
  await board.locator('[data-component-action="create"]').dispatchEvent('click');await select(main,bid);await main.waitForTimeout(1300);await select(main,aid);
  await wait(()=>board.locator('[data-component-action="create"]').isEnabled());createData=undefined;
  const resolved=(await componentState('main')).workspaces[aid];assert.equal(resolved.views.length,actionBefore+1);assert.equal(actionCalls.filter(op=>op==='submit').length,sentBefore+1);assert(actionCalls.includes('get'));
  await board.locator(`[data-component-action="close"][data-key="${resolved.editor}"]`).click();await board.locator(`[data-component-action="close"][data-key="${resolved.editor}"]`).waitFor({state:"detached"});await board.locator(`[data-component-action="select"][data-key="${editorId}"]`).click();
  checks.push('无页面动作不补建；超过等待期限后查询原回执，切换工作区恢复查询，重复点击不重发');
  createData={none:true};
  for(const loss of ['before','after']){
    const start=actionCalls.filter(op=>op==='submit').length;lostSubmission=loss;
    await board.locator('[data-component-action="create"]').click();
    await wait(()=>board.locator('[data-component-action="create"]').isEnabled());
    assert.equal(actionCalls.filter(op=>op==='submit').length-start,loss==='before'?2:1,'Only an unobserved request may be submitted again with its original intent');
    assert.equal((await componentState('main')).workspaces[aid].views.length,actionBefore);
  }
  createData=undefined;
  checks.push('接纳前后丢失提交回执均恢复：原意图查询，无接纳记录时复用原编号提交，完成后解除等待');
  await fileBrowser.getByRole('button',{name:'nginx.conf',exact:true}).dblclick();await flush(main);
  assert.equal((await componentState("main")).workspaces[aid].views.filter(v=>v.definitionKey.endsWith(':editor')).length,1);
  await editorFrame.locator('body').evaluate(el=>el.dataset.retained='same iframe');
  await select(main,bid);await select(main,aid);
  assert.equal(await editor.inputValue(),'draft for server A');assert.equal(await editorFrame.locator('body').getAttribute('data-retained'),'same iframe');
  checks.push('多终端独立草稿、最近使用定位、混合 Tab、同文件去重、切换不重建 iframe');
  await board.getByRole('button',{name:'左侧更多组件',exact:true}).click();
  await main.getByRole('textbox',{name:'查找组件或页面'}).fill('扩展测试组件 19');
  await main.locator('.component-menu-row:visible').getByRole('menuitem',{name:'固定入口',exact:true}).click();
  assert.equal((await componentState('main')).workspaces[aid].preferences['test.workbench:extra19'].pinned,true);
  await board.getByRole('button',{name:'左侧更多组件',exact:true}).click();await main.getByRole('textbox',{name:'查找组件或页面'}).fill('扩展测试组件 19');
  await main.locator('.component-menu-row:visible').getByRole('menuitem',{name:'取消固定',exact:true}).click();
  checks.push('20 个扩展视图通过更多入口查找、固定和取消固定');
  await board.getByRole('button',{name:'网络转发',exact:true}).click({button:'right'});
  await main.getByRole('menuitem',{name:'移到左侧',exact:true}).click();
  await main.waitForFunction(id=>!document.querySelector(`.server-workbench[data-id="${id}"]`).classList.contains('right-open'),aid);
  assert((await componentState("main")).workspaces[aid].left);
  await board.getByRole('button',{name:'文件',exact:true}).click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .left-rail [data-key="test.workbench:files"]`)?.getAttribute('aria-expanded')==='true',aid);
  assert.equal(await editor.inputValue(),'draft for server A');
  // Move it back so the existing ownership/layout checks keep their initial shape.
  await board.getByRole('button',{name:'网络转发',exact:true}).click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .left-rail [data-key="test.workbench:forwarding"]`)?.getAttribute('aria-expanded')==='true',aid);
  await flush(main);await refresh();
  await board.getByRole('button',{name:'网络转发',exact:true}).click({button:'right'});await main.getByRole('menuitem',{name:'移到右侧',exact:true}).click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .right-rail [data-key="test.workbench:forwarding"]`)?.getAttribute('aria-expanded')==='true',aid);
  await board.getByRole('button',{name:'文件',exact:true}).click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .left-rail [data-key="test.workbench:files"]`)?.getAttribute('aria-expanded')==='true',aid);
  checks.push('同侧单面板、左右移动保留 viewId 和编辑草稿');
  const closeEditor=()=>board.locator(`[data-component-action="close"][data-key="${editorId}"]`).click();
  await closeEditor();await main.locator('.component-close-dialog').waitFor();await main.getByRole('button',{name:'取消',exact:true}).click();
  await main.locator('.component-close-dialog').waitFor({state:'detached'});
  assert.equal(await board.evaluate(el=>el.inert),false);
  assert.equal(await editor.inputValue(),'draft for server A');
  await editor.fill('SODECK_TEST_SAVE_FAILURE');await flush(main);await closeEditor();await main.getByRole('button',{name:'保存并关闭',exact:true}).click();
  await main.locator('.component-close-dialog').getByText(/保存失败/).waitFor();
  await main.getByRole('button',{name:'取消',exact:true}).click();
  await main.locator('.component-close-dialog').waitFor({state:'detached'});
  assert.equal(await board.evaluate(el=>el.inert),false);
  assert.equal(await editor.inputValue(),'SODECK_TEST_SAVE_FAILURE');
  await editor.fill('draft for server A');await flush(main);
  checks.push('关闭取消及保存失败保留草稿，不提前释放页面');
  const oldSurface=await main.locator(`.component-surface[data-view-id="${editorId}"] iframe`).getAttribute('data-surface-id');
  await main.evaluate(()=>{window.hostFrames=[...document.querySelectorAll('.component-surface iframe')];manager.setHostVisible(false);});
  await refresh();await main.evaluate(()=>document.documentElement.dataset.theme='dark');
  assert.equal(await main.evaluate(()=>hostFrames.every(frame=>frame.isConnected&&[...document.querySelectorAll('.component-surface iframe')].includes(frame))),true,'Management navigation and theme changes retain iframe identity');
  assert.equal(await main.locator('.component-surface:not([hidden]) iframe').count(),0,'Hidden workbench surfaces cannot appear during refresh');
  assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  await main.evaluate(()=>manager.setHostVisible(true));await select(main,aid); await menu(main,aid); await main.locator('[data-choice="detach"]').click();
  await main.waitForFunction(id=>!document.querySelector(`.server-tab[data-id="${id}"]`),aid);
  const child=await makePage("workspace-browser"); assert.equal(await child.locator("#catalog-tab").isVisible(),false);
  assert.equal(await viewFrame(child,aid,editorId).getByRole('textbox').inputValue(),'draft for server A');
  await assert.rejects(()=>call('component_ui',{window:'main',operation:'event',args:{surfaceId:oldSurface,event:{action:'edit',text:'stale write'}}}),/移动|失效|关闭|输入权/);
  let p=await projection("workspace-browser"); assert.deepEqual(p.workspaces[0].context,a.context); assert.equal(Boolean((await componentState("workspace-browser")).workspaces[aid].right),true);
  await menu(child,aid); await child.locator('[data-choice="return"]').click();
  await wait(async()=>Boolean((await projection('main')).workspaces.find(w=>w.context.workspaceId===aid)));
  await refresh();
  assert.deepEqual((await projection("main")).workspaces.find(w=>w.context.workspaceId===aid).context,a.context);
  assert.equal((await projection("workspace-browser")).workspaces.length,0);
  assert.equal(await editor.inputValue(),'draft for server A');
  await closeEditor();await main.getByRole('button',{name:'保存并关闭',exact:true}).click();await main.locator(`.component-surface[data-view-id="${editorId}"]`).waitFor({state:'detached'});
  checks.push("菜单拆出和返回保留会话及布局，主窗口目录草稿不丢");
  await select(main,bid); await main.locator(`.server-tab[data-id="${aid}"]`).click({button:"right"});
  assert.equal((await projection('main')).window.activeWorkspace,bid,'Right-click does not activate a server');
  assert.deepEqual(await main.getByRole('menuitem').allTextContents(),['移到新窗口','关闭标签']);
  await main.keyboard.press('Escape');
  const inactive=main.locator(`.server-tab-select[data-id="${aid}"]`);assert(await inactive.evaluate(el=>el===document.activeElement));
  await main.keyboard.press('Shift+F10');assert.deepEqual(await main.getByRole('menuitem').allTextContents(),['移到新窗口','关闭标签']);
  await main.keyboard.press('Tab');assert.equal(await main.getByRole('menu').count(),0);assert(await inactive.evaluate(el=>el===document.activeElement));
  await inactive.click({button:'right'});
  await main.locator('[data-choice="close"]').click(); await main.waitForFunction(id=>!document.querySelector(`.server-tab[data-id="${id}"]`),aid);
  assert.equal((await projection("main")).window.activeWorkspace,bid); assert.equal((await call("server_catalog")).servers.length,3);
  checks.push("右键菜单锁定点击目标，关闭工作区保留服务器档案");
  await main.locator('.server-tab-select[data-id="'+bid+'"]').focus(); await main.keyboard.press("ArrowRight"); await main.waitForFunction(id=>document.querySelector(`.server-tab-select[data-id="${id}"]`)?.getAttribute("aria-selected")==="true",cid);
  await main.keyboard.press("Home"); await main.waitForFunction(()=>document.querySelector("#catalog-tab").getAttribute("aria-selected")==="true");
  checks.push("键盘箭头和 Home 切换固定页及服务器页");
  await select(main,bid);
  const beforeDrag=(await projection("main")).window.workspaceIds;
  let tabBox=await main.locator(`.server-tab-select[data-id="${bid}"]`).boundingBox();
  await main.mouse.move(tabBox.x+20,tabBox.y+15);await main.mouse.down();await main.mouse.move(tabBox.x+24,tabBox.y+15);await main.mouse.up();
  assert.equal(dragCalls.length,0,"Below threshold remains an ordinary tab click");
  dropTargets.set("main",{kind:"strip",windowId:"main",index:1});
  await main.mouse.move(tabBox.x+20,tabBox.y+15);await main.mouse.down();await main.mouse.move(tabBox.x+65,tabBox.y+15);await main.locator(".workspace-drag-ghost").waitFor();
  await main.keyboard.press("Escape");await main.mouse.up();await main.waitForFunction(()=>!document.querySelector('.workspace-drag-ghost'));
  assert.deepEqual((await projection("main")).window.workspaceIds,beforeDrag);assert.equal(dragCalls.at(-1).args.cancel,true);
  await main.mouse.move(tabBox.x+20,tabBox.y+15);await main.mouse.down();await main.mouse.move(tabBox.x+65,tabBox.y+15);await main.locator(".workspace-drag-ghost").waitFor();await main.mouse.up();await main.waitForFunction(()=>!document.querySelector('.workspace-drag-ghost'));
  assert.deepEqual((await projection("main")).window.workspaceIds,[cid,bid]);
  dropTargets.set("main",{kind:"invalid"});await main.evaluate(()=>window.manager.refresh());await main.locator(`.server-tab-select[data-id="${bid}"]`).waitFor();tabBox=await main.locator(`.server-tab-select[data-id="${bid}"]`).boundingBox();
  await main.mouse.move(tabBox.x+20,tabBox.y+15);await main.mouse.down();await main.mouse.move(tabBox.x+65,200);await main.locator(".workspace-drag-ghost").waitFor();await main.mouse.up();await main.waitForFunction(()=>!document.querySelector('.workspace-drag-ghost'));
  assert.deepEqual((await projection("main")).window.workspaceIds,[cid,bid]);
  checks.push("指针门槛、拖动排序、Esc 和无效落点；原生命中由独立测试验证");
  await select(main,bid); await panel(main,"left").click();
  await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) .left-rail [data-key="test.workbench:files"]').getAttribute('aria-expanded')==='false');
  await panel(main,"left").click();
  await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) .left-rail [data-key="test.workbench:files"]').getAttribute('aria-expanded')==='true');
  const separator=main.locator('.server-workbench:visible [data-resize="left"]');
  const initialWidth=await main.locator('.server-workbench:visible').evaluate(el=>parseFloat(el.style.getPropertyValue('--left-width')));
  await separator.focus(); await main.keyboard.press("ArrowRight");
  await main.waitForFunction(width=>document.querySelector(".server-workbench:not([hidden])").style.getPropertyValue("--left-width")===`${width+16}px`,initialWidth);
  const box=await separator.boundingBox(); await main.mouse.move(box.x+3,box.y+30); await main.mouse.down(); await main.mouse.move(box.x+53,box.y+30);
  // Live drag updates the effective iframe width before pointerup persists the preference.
  await main.waitForFunction(width=>document.querySelector('.server-workbench:not([hidden])').style.getPropertyValue('--effective-left-width')===`${width+66}px`,initialWidth);
  await main.evaluate(()=>window.manager.refresh());
  await main.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  assert.equal(await main.locator('.server-workbench:visible').evaluate(el=>parseFloat(el.style.getPropertyValue('--effective-left-width'))),initialWidth+66,'A snapshot during a captured resize must preserve the preview');
  await main.mouse.up();
  await main.waitForFunction(width=>document.querySelector(".server-workbench:not([hidden])").style.getPropertyValue("--left-width")===`${width+66}px`,initialWidth);
  checks.push("面板宽度支持拖动和键盘，写回核心布局");
  const settleLayout=()=>main.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  const boardWidth=side=>main.locator('.server-workbench:visible').evaluate((el,side)=>parseFloat(el.style.getPropertyValue(`--effective-${side}-width`)),side);
  const dragPanel=async(side,delta)=>{const handle=main.locator(`.server-workbench:visible [data-resize="${side}"]`);const box=await handle.boundingBox();await main.mouse.move(box.x+4,box.y+80);await main.mouse.down();await main.mouse.move(box.x+4+delta,box.y+80);await settleLayout();};
  await call('workspace_layout',{window:'main',id:bid,side:'left',width:280});
  await call('workspace_layout',{window:'main',id:bid,side:'right',width:280});await refresh();
  if(await panel(main,'right').getAttribute('aria-expanded')!=='true')await panel(main,'right').click();
  await main.setViewportSize({width:1000,height:720});await settleLayout();
  // The click awaits pointer delivery; the asynchronous core layout must settle before measuring a drag.
  await main.waitForFunction(()=>{const board=document.querySelector('.server-workbench:not([hidden])');return board.classList.contains('left-open')&&board.classList.contains('right-open')&&board.style.getPropertyValue('--effective-left-width')==='280px'&&board.style.getPropertyValue('--effective-right-width')==='280px';});
  await main.evaluate(()=>window.resizeFrames=[...document.querySelectorAll('.component-surface:not([hidden]) iframe')]);assert((await main.evaluate(()=>window.resizeFrames.length))>0,'The resize retention check must include visible surfaces');
  await dragPanel('left',70);assert.equal(await boardWidth('left'),316);assert.equal(await main.locator('.server-workbench:visible').evaluate(el=>el.classList.contains('left-open')&&el.classList.contains('right-open')),true);
  await main.mouse.up();await main.evaluate(()=>window.manager.flush());
  assert.equal((await projection('main')).workspaces.find(w=>w.context.workspaceId===bid).layout.leftWidth,316);
  // A new preview remains authoritative while the previous commit is held.
  let releaseLayout;layoutGate=()=>new Promise(resolve=>releaseLayout=resolve);
  const callsBefore=layoutCalls.length;await dragPanel('left',-20);await main.mouse.up();await wait(()=>layoutCalls.length===callsBefore+1);
  await main.evaluate(()=>window.manager.refresh());await settleLayout();assert.equal(await boardWidth('left'),296);
  await dragPanel('left',-20);await main.mouse.up();await settleLayout();assert.equal(await boardWidth('left'),276);
  layoutGate=undefined;releaseLayout();await main.evaluate(()=>window.manager.flush());await settleLayout();
  assert.equal(await boardWidth('left'),276);assert.equal(layoutCalls.length,callsBefore+2);
  // Key repeats derive from the latest displayed width, not the last core snapshot.
  layoutGate=()=>new Promise(resolve=>releaseLayout=resolve);const keyCalls=layoutCalls.length;
  await separator.focus();for(let i=0;i<3;i++)await main.keyboard.press('ArrowRight');await settleLayout();assert.equal(await boardWidth('left'),316);
  layoutGate=undefined;releaseLayout();await main.evaluate(()=>window.manager.flush());assert.equal(layoutCalls.length,keyCalls+2);
  layoutGate=()=>{throw new Error('隔离测试：宽度保存失败');};await dragPanel('left',-20);await main.mouse.up();await main.evaluate(()=>window.manager.flush());layoutGate=undefined;await settleLayout();
  assert.equal(await boardWidth('left'),316);assert.match(await main.locator('#notice').textContent(),/面板宽度未保存/);
  // All interruption paths cancel without a save or a leftover capture/cursor.
  for(const ending of ['escape','blur','capture','cancel']){
    if(ending==='escape')await main.locator('.component-surface:not([hidden]) iframe').first().focus();
    const before=await boardWidth('left'),count=layoutCalls.length;await dragPanel('left',-30);assert.equal(await boardWidth('left'),before-30);
    if(ending==='escape')await main.keyboard.press('Escape');
    if(ending==='blur')await main.evaluate(()=>window.dispatchEvent(new Event('blur')));
    if(ending==='capture'){await separator.evaluate(el=>el.releasePointerCapture(1));await main.mouse.move(500,100);}
    if(ending==='cancel')await separator.dispatchEvent('pointercancel',{pointerId:1,isPrimary:true});
    await main.mouse.up();await settleLayout();assert.equal(await boardWidth('left'),before);assert.equal(layoutCalls.length,count);
    assert.equal(await main.evaluate(()=>document.body.classList.contains('panel-resizing')),false);
  }
  // The other side uses the same preview, cancellation and persistence path.
  await dragPanel('right',20);await main.evaluate(()=>window.manager.refresh());await settleLayout();assert.equal(await boardWidth('right'),260);
  await main.mouse.up();await main.evaluate(()=>window.manager.flush());assert.equal((await projection('main')).workspaces.find(w=>w.context.workspaceId===bid).layout.rightWidth,260);
  await dragPanel('left',-20);await main.setViewportSize({width:720,height:480});await settleLayout();await main.mouse.up();
  assert.equal(await main.evaluate(()=>document.body.classList.contains('panel-resizing')),false);
  await main.setViewportSize({width:1000,height:720});
  // ResizeObserver may deliver the narrow viewport's geometry after two animation frames.
  await wait(async()=>await boardWidth('left')===316);assert.equal(await boardWidth('left'),316);
  assert.equal(await main.evaluate(()=>window.resizeFrames.every(frame=>frame.isConnected)),true,'Resize must retain mounted iframe nodes');
  await dragPanel('left',-20);await call('workspace_activate',{window:'main',id:cid});await refresh();await main.mouse.up();
  assert.equal(await main.evaluate(()=>document.body.classList.contains('panel-resizing')),false);await select(main,bid);
  await main.setViewportSize({width:1100,height:720});await settleLayout();
  checks.push('两侧预览抗刷新、窄窗口约束、在途保存与重复键盘合并、所有手势取消及 iframe 保留');
  await main.evaluate(()=>{const d=document.createElement('dialog');d.id='blocking';document.body.append(d);d.showModal();});
  // Modal captures input: fixed catalog and server tabs cannot steal the draft's focus.
  assert.equal(await main.locator("dialog").evaluate(el=>el.open),true); await main.evaluate(()=>document.querySelector('dialog').remove());
  for (const theme of ["light","dark","dopamine"]) for (const width of [1100,720]) {
    await main.setViewportSize({width,height:480}); await main.emulateMedia({reducedMotion:"reduce"});
    await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await main.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
    const chrome=await main.locator('.server-workbench:visible').evaluate(node=>({divider:getComputedStyle(node.querySelector('.left-rail'),'::after').width,tools:getComputedStyle(node.querySelector('.component-tabs')).backgroundColor}));
    assert.equal(chrome.divider,theme==='dopamine'?'2px':'1px');
    if(theme==='dopamine')assert.equal(chrome.tools,'rgb(66, 198, 155)');
    assert.equal(await main.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    assert.equal(await main.locator('.workspace-status').count(),0,'Core status is owned by Shell');
    if(width===1100){fs.mkdirSync('artifacts/workbench-design',{recursive:true});await main.screenshot({path:`artifacts/workbench-design/workbench-${theme}.png`});}
  }
  await main.setViewportSize({width:1100,height:720});
  const display=await main.context().newCDPSession(main);
  for(const scale of [1.25,1.5]){
    await display.send('Emulation.setDeviceMetricsOverride',{width:1100,height:720,deviceScaleFactor:scale,mobile:false});
    assert(await main.locator('.server-workbench:visible').evaluate(el=>{const r=el.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&r.bottom<=innerHeight;}));
    await main.screenshot({path:`artifacts/workbench-design/workbench-dopamine-${scale}.png`});
  }
  await display.send('Emulation.clearDeviceMetricsOverride');await display.detach();
  await main.screenshot({path:path.join(directory,"workspace.png")});
  const output=path.resolve("artifacts/workspace-ui.png");fs.mkdirSync(path.dirname(output),{recursive:true});fs.copyFileSync(path.join(directory,"workspace.png"),output);
  checks.push("长名称、多主题、720×480 和减少动态下布局可用；连接状态仅展示，点击无弹出菜单");

  const crowded=[];for(let i=0;i<20;i++)crowded.push(await open('多标签服务器 '+i,100+i));await refresh();
  assert.equal(await main.locator('#server-tabs .server-tab').count(),22);
  const pickerButton=main.locator('.topbar [data-tab-action="pick"]');
  for(const width of [1100,720]){
    await main.setViewportSize({width,height:720});await pickerButton.click();
    await main.locator('.tab-picker input').fill('100.invalid');
    assert.equal(await main.locator('.tab-picker-row').count(),1);
    await main.locator('[data-tab-picker-action="select"]').click();
    await wait(async()=>(await projection('main')).window.activeWorkspace===crowded[0].context.workspaceId);
    await main.waitForTimeout(300);
    assert.equal(await main.locator('#server-tabs').evaluate(n=>{const t=n.querySelector('[aria-selected="true"]').closest('.server-tab').getBoundingClientRect(),r=n.getBoundingClientRect();return t.left>=r.left-1&&t.right<=r.right+1;}),true);
    await main.locator('.topbar [data-tab-action="forward"]').click();await main.waitForTimeout(300);
    const browsed=await main.locator('#server-tabs').evaluate(n=>n.scrollLeft);await refresh();
    assert.equal(await main.locator('#server-tabs').evaluate(n=>n.scrollLeft),browsed);
    assert.equal((await projection('main')).window.activeWorkspace,crowded[0].context.workspaceId);
    assert.equal(await main.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    fs.mkdirSync('artifacts/tab-overflow',{recursive:true});await main.screenshot({path:'artifacts/tab-overflow/servers-'+width+'.png'});
  }
  for(const w of crowded)await main.evaluate(async id=>{await manager.closer.request({kind:'host',id});await manager.refresh();},w.context.workspaceId);
  await refresh();await select(main,bid);
  checks.push('20 个服务器标签：1100/720 宽度固定操作区、endpoint 搜索、选择露出、手动浏览与刷新不抢活动页');
  for (const id of [bid,cid]) { await main.locator(`.tab-close-button[data-id="${id}"]`).click();await main.locator(`.server-tab[data-id="${id}"]`).waitFor({state:"detached"}); await refresh(); }
  await main.locator("#catalog-draft").waitFor({state:"visible"}); assert.equal((await projection("main")).window.activeWorkspace,null);
  checks.push("最后一个服务器关闭后返回固定管理页");
  const disposal=await open('手势销毁',4);await refresh();await select(main,disposal.context.workspaceId);
  if(await panel(main,'left').getAttribute('aria-expanded')!=='true')await panel(main,'left').click();
  await settleLayout();const disposeCalls=layoutCalls.length;await dragPanel('left',-20);
  await main.evaluate(()=>window.manager.dispose());await main.mouse.up();await settleLayout();
  assert.equal(await main.evaluate(()=>document.body.classList.contains('panel-resizing')),false);
  assert.equal(await main.locator('.server-workbench').count(),0);assert.equal(layoutCalls.length,disposeCalls);
  checks.push('宿主销毁结束捕获、清理工作台与 RAF，不保存未提交的手势');
  assert.deepEqual(errors,[]); console.log(JSON.stringify({passed:checks.length,checks,pageErrors:errors,screenshot:output},null,2));
} catch(error) {
  for (const finish of traces) await finish(true);
  console.error(JSON.stringify(await call('close_operations',{window:'main'}),null,2));
  fs.mkdirSync('artifacts/menu-layout',{recursive:true});await pages[0]?.screenshot({path:'artifacts/menu-layout/failure.png'});
  for(const page of pages) console.error(await page.evaluate(()=>({focus:document.activeElement?.tagName,notice:document.querySelector('#notice')?.textContent,dialog:document.querySelector('.component-close-dialog')?.textContent,content:document.querySelector('#workspace-content')?.style.cssText,board:document.querySelector('.server-workbench:not([hidden])')?.style.cssText})));
  throw error;
} finally {
  for (const finish of traces) await finish(false);
  await browser.close();await vite.close();await bridge.stop();fs.rmSync(directory,{recursive:true,force:true});
}
