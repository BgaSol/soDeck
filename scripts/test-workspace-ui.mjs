// Real workspace DOM -> test bridge -> actual Rust ownership. Browser windows stand in for native surfaces.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { createServer } from "vite";
import { chromium } from "playwright";

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--locked"], { stdio: "inherit" });
if (build.status !== 0) throw new Error("Catalog test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-ui-"));
const exampleBuild = spawnSync("cargo", ["build", "--manifest-path", "plugins/ui-example/Cargo.toml", "--locked"], { stdio: "inherit" });
if (exampleBuild.status !== 0) throw new Error("UI example build failed");
const pluginRoot = path.join(directory, "ui-plugin");fs.mkdirSync(pluginRoot);
const suffix = process.platform === "win32" ? ".exe" : "";
fs.copyFileSync(`plugins/ui-example/target/debug/sodeck-ui-example${suffix}`, path.join(pluginRoot, `sodeck-ui-example${suffix}`));
fs.cpSync("plugins/ui-example/ui", path.join(pluginRoot,"ui"), {recursive:true});
const metadata = JSON.parse(fs.readFileSync("plugins/ui-example/release.json","utf8"));
for(let index=0;index<20;index++)metadata.ui.views.push({id:`extra${index}`,title:`扩展示例组件 ${index}`,icon:'plug',container:'sidebar',instancePolicy:'perWorkspace',defaultSide:'left'});
const target = spawnSync("rustc",["-vV"],{encoding:"utf8"}).stdout.match(/^host: (.+)$/m)[1];
fs.writeFileSync(path.join(pluginRoot,"plugin.json"),JSON.stringify({...metadata,version:"0.1.0",target,entry:`sodeck-ui-example${suffix}`}));
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
const pages = [], errors = [], checks = [], dragCalls = [], dropTargets = new Map(), drags = new Map();
const call = (command, args={}) => bridge.call(command,args);
const projection = window => call("workspace_snapshot",{window});
const componentState = window => call("component_ui",{window,operation:"snapshot",args:{}});
async function makePage(window) {
  const page = await browser.newPage({ viewport: {width:1100,height:720} }); pages.push(page);
  page.on("pageerror", error=>errors.push(String(error)));
  await page.exposeBinding("workspaceInvoke",async (_,command,args={})=>{
    if(command==="workspace_drag_start") { dragCalls.push({command,args}); drags.set(window,args.id);return `browser-${window}`; }
    if(command==="workspace_drag_sample") return dropTargets.get(window)??{kind:"invalid"};
    if(command==="workspace_drag_finish") { dragCalls.push({command,args}); const target=dropTargets.get(window); const id=drags.get(window);drags.delete(window); if(!args.cancel&&target?.kind==="strip")return call("workspace_move",{window,id,target:target.windowId,index:target.index});return null; }
    return call(command,{...args,window});
  });
  await page.route(url+'__plugin__/**',route=>{
    const relative=decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/');
    const file=path.join(pluginRoot,relative);
    const types={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
    return route.fulfill({contentType:types[path.extname(file)]||'application/octet-stream',body:fs.readFileSync(file)});
  });
  await page.route(url, route=>route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab" role="tab">服务器管理</button><div id="server-tabs" class="server-tabs" role="tablist"></div><div class="drag-region"></div><nav class="global-tools"><button>状态</button><button>消息</button><button>设置</button></nav><div class="window-controls"><button>—</button><button>□</button><button>×</button></div></header><main class="workspace"><div id="server-catalog"><input id="catalog-draft" value="draft retained"></div><div id="workspace-content" hidden></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';window.__TAURI_INTERNALS__={convertFileSrc:p=>new URL('/__plugin__/'+p,location.href).href};window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),message=>document.querySelector('#notice').textContent=message,window.workspaceInvoke);await window.manager.start();window.ready=true;</script></body></html>`}));
  await page.goto(url); await page.waitForFunction(()=>window.ready); return page;
}
async function refresh() { for (const page of pages) await page.evaluate(()=>window.manager.refresh()); }
const select = async (page,id)=>{ await page.locator(`.server-tab-select[data-id="${id}"]`).click(); await page.waitForFunction(id=>document.querySelector(`.server-tab-select[data-id="${id}"]`)?.getAttribute("aria-selected")==="true",id); };
const menu = (page,id)=>page.locator(`.tab-menu-button[data-id="${id}"]`).click();
const panel = (page,side)=>page.locator(`.server-workbench:visible [data-component-action="toggle"][data-key="sodeck.ui-example:${side==='left'?'files':'forwarding'}"]`);
async function open(name,index) {
  const changed=await call("change_catalog",{revision:(await call("server_catalog")).revision,change:{op:"saveServer",draft:{name,host:"same.invalid",port:22,username:"ops",authentication:{type:"password",secret:{action:"clear"}},route:[]}}});
  return call("workspace_fixture_open",{serverId:changed.selectedId,connectionId:`fixture-${index}`,endpoint:`ops@${index}.invalid:22`});
}
try {
  const main=await makePage("main");
  assert.equal(await main.locator(".server-tab").count(),0); assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  checks.push("普通启动只显示目录，不伪造连接");
  await call("fixture_load_plugin",{directory:pluginRoot});
  const a=await open("生产 Web",1),b=await open("测试环境",2),c=await open("长名称服务器-".repeat(12),3);
  const aid=a.context.workspaceId,bid=b.context.workspaceId,cid=c.context.workspaceId; await refresh();
  await select(main,aid); await panel(main,"left").click(); await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) [data-component-action="toggle"][data-key="sodeck.ui-example:files"]').getAttribute('aria-expanded')==='false'); await panel(main,"right").click(); await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) [data-component-action="toggle"][data-key="sodeck.ui-example:forwarding"]').getAttribute('aria-expanded')==='true');
  await select(main,bid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"true"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"false");
  await select(main,aid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"false"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"true");
  assert((await main.locator(".server-workbench:visible .workspace-endpoint").innerText()).includes("1.invalid"));
  checks.push("固定上下文、服务器切换与侧栏状态各自保持");
  const board=main.locator(`.server-workbench[data-id="${aid}"]`);
  const viewFrame=(page,wid,vid)=>page.frameLocator(`.server-workbench[data-id="${wid}"] .component-surface[data-view-id="${vid}"] iframe`);
  const flush=page=>page.evaluate(()=>window.manager.flush());
  let aw=(await componentState("main")).workspaces[aid];
  assert.equal(aw.views.filter(v=>v.definitionKey.endsWith(':terminal')).length,1);
  const firstTerminal=aw.views.find(v=>v.definitionKey.endsWith(':terminal')).id;
  const terminalOne=viewFrame(main,aid,firstTerminal).getByRole('textbox',{name:'终端输入草稿'});
  await terminalOne.fill('terminal A one');await flush(main);
  await board.locator('[data-component-action="create"]').click();
  await main.waitForFunction(id=>document.querySelector(`.server-workbench[data-id="${id}"] .component-tab-list`).childElementCount===2,aid);
  aw=(await componentState("main")).workspaces[aid];
  const secondTerminal=aw.views.filter(v=>v.definitionKey.endsWith(':terminal')).find(v=>v.id!==firstTerminal).id;
  await viewFrame(main,aid,secondTerminal).getByRole('textbox').fill('terminal A two');await flush(main);
  await board.locator(`[data-component-action="select"][data-key="${firstTerminal}"]`).click();
  await board.getByRole('button',{name:'定位终端',exact:true}).click();
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
  await fileBrowser.getByRole('button',{name:'nginx.conf',exact:true}).dblclick();await flush(main);
  assert.equal((await componentState("main")).workspaces[aid].views.filter(v=>v.definitionKey.endsWith(':editor')).length,1);
  await editorFrame.locator('body').evaluate(el=>el.dataset.retained='same iframe');
  await select(main,bid);await select(main,aid);
  assert.equal(await editor.inputValue(),'draft for server A');assert.equal(await editorFrame.locator('body').getAttribute('data-retained'),'same iframe');
  checks.push('多终端独立草稿、最近使用定位、混合 Tab、同文件去重、切换不重建 iframe');
  await board.getByRole('button',{name:'左侧更多组件',exact:true}).click();
  await main.getByRole('textbox',{name:'查找组件或页面'}).fill('扩展示例组件 19');
  await main.locator('.component-menu-row:visible').getByRole('button',{name:'固定',exact:true}).click();
  assert.equal((await componentState('main')).workspaces[aid].preferences['sodeck.ui-example:extra19'].pinned,true);
  await board.getByRole('button',{name:'左侧更多组件',exact:true}).click();await main.getByRole('textbox',{name:'查找组件或页面'}).fill('扩展示例组件 19');
  await main.locator('.component-menu-row:visible').getByRole('button',{name:'取消固定',exact:true}).click();
  checks.push('20 个扩展视图通过更多入口查找、固定和取消固定');
  await board.getByRole('button',{name:'网络转发 面板菜单',exact:true}).click();
  await main.getByRole('button',{name:'移到左侧',exact:true}).click();
  await main.waitForFunction(id=>!document.querySelector(`.server-workbench[data-id="${id}"]`).classList.contains('right-open'),aid);
  assert((await componentState("main")).workspaces[aid].left);
  await board.getByRole('button',{name:'文件',exact:true}).click();
  assert.equal(await editor.inputValue(),'draft for server A');
  // Move it back so the existing ownership/layout checks keep their initial shape.
  await board.getByRole('button',{name:'网络转发',exact:true}).click();
  await board.getByRole('button',{name:'网络转发 面板菜单',exact:true}).click();await main.getByRole('button',{name:'移到右侧',exact:true}).click();
  await board.getByRole('button',{name:'文件',exact:true}).click();
  checks.push('同侧单面板、左右移动保留 viewId 和编辑草稿');
  const closeEditor=()=>board.locator(`[data-component-action="close"][data-key="${editorId}"]`).click();
  await closeEditor();await main.locator('.component-close-dialog').waitFor();await main.getByRole('button',{name:'取消',exact:true}).click();
  assert.equal(await editor.inputValue(),'draft for server A');
  await editor.fill('SODECK_TEST_SAVE_FAILURE');await flush(main);await closeEditor();await main.getByRole('button',{name:'保存并关闭',exact:true}).click();
  await main.waitForFunction(()=>document.querySelector('#notice').textContent.includes('保存失败'));
  assert.equal(await editor.inputValue(),'SODECK_TEST_SAVE_FAILURE');
  await editor.fill('draft for server A');await flush(main);
  checks.push('关闭取消及保存失败保留草稿，不提前释放页面');
  const oldSurface=await board.locator(`.component-surface[data-view-id="${editorId}"] iframe`).getAttribute('data-surface-id');
  await main.locator("#catalog-tab").click(); assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  await select(main,aid); await menu(main,aid); await main.locator('[data-choice="detach"]').click();
  await main.waitForFunction(id=>!document.querySelector(`.server-tab[data-id="${id}"]`),aid);
  const child=await makePage("workspace-browser"); assert.equal(await child.locator("#catalog-tab").isVisible(),false);
  assert.equal(await viewFrame(child,aid,editorId).getByRole('textbox').inputValue(),'draft for server A');
  await assert.rejects(()=>call('component_ui',{window:'main',operation:'event',args:{surfaceId:oldSurface,event:{action:'edit',text:'stale write'}}}),/移动|失效|关闭/);
  let p=await projection("workspace-browser"); assert.deepEqual(p.workspaces[0].context,a.context); assert.equal(Boolean((await componentState("workspace-browser")).workspaces[aid].right),true);
  await menu(child,aid); await child.locator('[data-choice="return"]').click(); await refresh();
  assert.deepEqual((await projection("main")).workspaces.find(w=>w.context.workspaceId===aid).context,a.context);
  assert.equal((await projection("workspace-browser")).workspaces.length,0);
  assert.equal(await editor.inputValue(),'draft for server A');
  await closeEditor();await main.getByRole('button',{name:'保存并关闭',exact:true}).click();await board.locator(`.component-surface[data-view-id="${editorId}"]`).waitFor({state:'detached'});
  checks.push("菜单拆出和返回保留会话及布局，主窗口目录草稿不丢");
  await select(main,bid); await main.locator(`.server-tab[data-id="${aid}"]`).click({button:"right"});
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
  await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) .left-rail [data-key="sodeck.ui-example:files"]').getAttribute('aria-expanded')==='false');
  await panel(main,"left").click();
  await main.waitForFunction(()=>document.querySelector('.server-workbench:not([hidden]) .left-rail [data-key="sodeck.ui-example:files"]').getAttribute('aria-expanded')==='true');
  const separator=main.locator('.server-workbench:visible [data-resize="left"]'); await separator.focus(); await main.keyboard.press("ArrowRight");
  await main.waitForFunction(()=>document.querySelector("#workspace-content").style.getPropertyValue("--left-width")==="236px");
  const box=await separator.boundingBox(); await main.mouse.move(box.x+3,box.y+30); await main.mouse.down(); await main.mouse.move(box.x+53,box.y+30); await main.mouse.up();
  await main.waitForFunction(()=>document.querySelector("#workspace-content").style.getPropertyValue("--left-width")==="286px");
  checks.push("面板宽度支持拖动和键盘，写回核心布局");
  await main.evaluate(()=>{const d=document.createElement('dialog');d.id='blocking';document.body.append(d);d.showModal();});
  // Modal captures input: fixed catalog and server tabs cannot steal the draft's focus.
  assert.equal(await main.locator("dialog").evaluate(el=>el.open),true); await main.evaluate(()=>document.querySelector('dialog').remove());
  for (const theme of ["light","dark","dopamine"]) for (const width of [1100,720]) {
    await main.setViewportSize({width,height:480}); await main.emulateMedia({reducedMotion:"reduce"});
    await main.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    assert.equal(await main.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    assert.equal(await main.locator(".window-controls").evaluate(el=>el.getBoundingClientRect().right<=innerWidth),true);
  }
  await main.setViewportSize({width:1100,height:720});
  await main.screenshot({path:path.join(directory,"workspace.png")});
  const output=path.resolve("artifacts/workspace-ui.png");fs.mkdirSync(path.dirname(output),{recursive:true});fs.copyFileSync(path.join(directory,"workspace.png"),output);
  checks.push("长名称、多主题、720×480 和减少动态下布局可用");
  for (const id of [bid,cid]) { await main.locator(`.tab-close-button[data-id="${id}"]`).click(); await refresh(); }
  await main.locator("#catalog-draft").waitFor({state:"visible"}); assert.equal((await projection("main")).window.activeWorkspace,null);
  checks.push("最后一个服务器关闭后返回固定管理页");
  assert.deepEqual(errors,[]); console.log(JSON.stringify({passed:checks.length,checks,pageErrors:errors,screenshot:output},null,2));
} catch(error) {
  for(const page of pages) console.error(await page.evaluate(()=>({focus:document.activeElement?.tagName,notice:document.querySelector('#notice')?.textContent,content:document.querySelector('#workspace-content')?.style.cssText,board:document.querySelector('.server-workbench:not([hidden])')?.style.cssText})));
  throw error;
} finally {
  await browser.close();await vite.close();await bridge.stop();fs.rmSync(directory,{recursive:true,force:true});
}
