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
async function makePage(window) {
  const page = await browser.newPage({ viewport: {width:1100,height:720} }); pages.push(page);
  page.on("pageerror", error=>errors.push(String(error)));
  await page.exposeBinding("workspaceInvoke",async (_,command,args={})=>{
    if(command==="workspace_drag_start") { dragCalls.push({command,args}); drags.set(window,args.id);return `browser-${window}`; }
    if(command==="workspace_drag_sample") return dropTargets.get(window)??{kind:"invalid"};
    if(command==="workspace_drag_finish") { dragCalls.push({command,args}); const target=dropTargets.get(window); const id=drags.get(window);drags.delete(window); if(!args.cancel&&target?.kind==="strip")return call("workspace_move",{window,id,target:target.windowId,index:target.index});return null; }
    return call(command,{...args,window});
  });
  await page.route(url, route=>route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><head><meta charset="utf-8"></head><body><header class="topbar"><button id="catalog-tab" role="tab">服务器管理</button><div id="server-tabs" class="server-tabs" role="tablist"></div><div class="drag-region"></div><nav class="global-tools"><button>状态</button><button>消息</button><button>设置</button></nav><div class="window-controls"><button>—</button><button>□</button><button>×</button></div></header><main class="workspace"><div id="server-catalog"><input id="catalog-draft" value="draft retained"></div><div id="workspace-content" hidden></div></main><div id="notice"></div><script type="module">import '/style.css';import {Workspaces} from '/workspaces.ts';window.manager=new Workspaces(document.querySelector('#server-tabs'),document.querySelector('#workspace-content'),document.querySelector('#server-catalog'),message=>document.querySelector('#notice').textContent=message,window.workspaceInvoke);await window.manager.start();window.ready=true;</script></body></html>`}));
  await page.goto(url); await page.waitForFunction(()=>window.ready); return page;
}
async function refresh() { for (const page of pages) await page.evaluate(()=>window.manager.refresh()); }
const select = async (page,id)=>{ await page.locator(`.server-tab-select[data-id="${id}"]`).click(); await page.waitForFunction(id=>document.querySelector(`.server-tab-select[data-id="${id}"]`)?.getAttribute("aria-selected")==="true",id); };
const menu = (page,id)=>page.locator(`.tab-menu-button[data-id="${id}"]`).click();
const panel = (page,side)=>page.locator(`[data-panel="${side}"]`);
async function open(name,index) {
  const changed=await call("change_catalog",{revision:(await call("server_catalog")).revision,change:{op:"saveServer",draft:{name,host:"same.invalid",port:22,username:"ops",authentication:{type:"password",secret:{action:"clear"}},route:[]}}});
  return call("workspace_fixture_open",{serverId:changed.selectedId,connectionId:`fixture-${index}`,endpoint:`ops@${index}.invalid:22`});
}
try {
  const main=await makePage("main");
  assert.equal(await main.locator(".server-tab").count(),0); assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  checks.push("普通启动只显示目录，不伪造连接");
  const a=await open("生产 Web",1),b=await open("测试环境",2),c=await open("长名称服务器-".repeat(12),3);
  const aid=a.context.workspaceId,bid=b.context.workspaceId,cid=c.context.workspaceId; await refresh();
  await select(main,aid); await panel(main,"left").click(); await main.waitForFunction(()=>document.querySelector('[data-panel="left"]').getAttribute('aria-expanded')==='false'); await panel(main,"right").click(); await main.waitForFunction(()=>document.querySelector('[data-panel="right"]').getAttribute('aria-expanded')==='true');
  await select(main,bid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"true"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"false");
  await select(main,aid); assert.equal(await panel(main,"left").getAttribute("aria-expanded"),"false"); assert.equal(await panel(main,"right").getAttribute("aria-expanded"),"true");
  assert((await main.locator(".workspace-context").innerText()).includes("1.invalid"));
  checks.push("固定上下文、服务器切换与侧栏状态各自保持");
  await main.locator("#catalog-tab").click(); assert.equal(await main.locator("#catalog-draft").inputValue(),"draft retained");
  await select(main,aid); await menu(main,aid); await main.locator('[data-choice="detach"]').click();
  await main.waitForFunction(id=>!document.querySelector(`.server-tab[data-id="${id}"]`),aid);
  const child=await makePage("workspace-browser"); assert.equal(await child.locator("#catalog-tab").isVisible(),false);
  let p=await projection("workspace-browser"); assert.deepEqual(p.workspaces[0].context,a.context); assert.equal(p.workspaces[0].layout.rightOpen,true);
  await menu(child,aid); await child.locator('[data-choice="return"]').click(); await refresh();
  assert.deepEqual((await projection("main")).workspaces.find(w=>w.context.workspaceId===aid).context,a.context);
  assert.equal((await projection("workspace-browser")).workspaces.length,0);
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
  dropTargets.set("main",{kind:"invalid"});tabBox=await main.locator(`.server-tab-select[data-id="${bid}"]`).boundingBox();
  await main.mouse.move(tabBox.x+20,tabBox.y+15);await main.mouse.down();await main.mouse.move(tabBox.x+65,200);await main.locator(".workspace-drag-ghost").waitFor();await main.mouse.up();await main.waitForFunction(()=>!document.querySelector('.workspace-drag-ghost'));
  assert.deepEqual((await projection("main")).window.workspaceIds,[cid,bid]);
  checks.push("指针门槛、拖动排序、Esc 和无效落点；原生命中由独立测试验证");
  await select(main,bid); await panel(main,"left").click(); await panel(main,"left").click();
  const separator=main.locator('[data-resize="left"]'); await separator.focus(); await main.keyboard.press("ArrowRight");
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
  assert.equal(await main.locator("#catalog-draft").isVisible(),true); assert.equal((await projection("main")).window.activeWorkspace,null);
  checks.push("最后一个服务器关闭后返回固定管理页");
  assert.deepEqual(errors,[]); console.log(JSON.stringify({passed:checks.length,checks,pageErrors:errors,screenshot:output},null,2));
} finally {
  await browser.close();await vite.close();await bridge.stop();fs.rmSync(directory,{recursive:true,force:true});
}
