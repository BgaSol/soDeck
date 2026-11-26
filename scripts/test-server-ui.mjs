import {blockTarget,restoreTarget} from './write-failure-fixture.mjs';
import {fixtureBinary} from './cargo-fixture.mjs';
// Actual DOM -> desktop API -> Rust catalog -> isolated local files. No server connections.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { createServer } from "vite";
import { chromium } from "playwright";
import { recordTrace } from './ui-trace.mjs';

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--locked"], { stdio: "inherit" });
if (build.status !== 0) throw new Error("Catalog test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-ui-"));
const executable = fixtureBinary('catalog-fixture');
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
let bridge = startBridge();
const vite = await createServer({ server: { host: "127.0.0.1", port: 0, strictPort: false } });
await vite.listen();
const url = vite.resolvedUrls.local[0];
const browser = await chromium.launch({ headless: true, ...(process.env.SODECK_BROWSER_CHANNEL ? { channel: process.env.SODECK_BROWSER_CHANNEL } : {}) });
const page = await browser.newPage({ viewport: { width: 1100, height: 720 } });
const finishTrace = await recordTrace(page.context(), 'server');
const errors = [];
let clipboardText = "";
let failClipboard = false, holdExport = false, exportHeld = false, releaseExport;
let holdImport=false,importHeld=false,releaseImport;
let failCatalogRead = false, holdCatalogRead = false, catalogReadHeld = false, releaseCatalogRead;
page.on("pageerror", error => errors.push(String(error)));
page.on("request", request => { assert.equal(new URL(request.url()).hostname, "127.0.0.1", "Unexpected network request"); });
await page.exposeBinding("catalogInvoke", async (_, command, args) => {
  if(command === "clipboard_text"){if(args.operation === "write"){if(failClipboard)throw new Error('隔离测试：剪贴板不可写');clipboardText=args.text;}return args.operation === "read"?clipboardText:null;}
  if (command === "server_catalog" && failCatalogRead) throw new Error("隔离测试：目录暂时不可读");
  const result = await bridge.call(command, args);
  if(command === "export_server_share"&&holdExport){holdExport=false;exportHeld=true;await new Promise(resolve=>releaseExport=resolve);}
  if (command === "server_catalog" && holdCatalogRead) {
    holdCatalogRead = false; catalogReadHeld = true;
    await new Promise(resolve => releaseCatalogRead = resolve);
  }
  if(command === "import_server_share"&&holdImport){holdImport=false;importHeld=true;await new Promise(resolve=>releaseImport=resolve);}
  // Disk completion may precede the UI callback. Keep this race reproducible.
  if (["deleteTag", "deleteServer", "deleteProxy"].includes(args?.change?.op)) await new Promise(resolve => setTimeout(resolve, 150));
  return result;
});
await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.catalogInvoke(command, args) }; });
await page.route(url, route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><head><meta charset="UTF-8"></head><body><header class="topbar"><div class="app-tab">SoDeck · 服务器</div></header><main class="workspace" id="test-workspace"></main><div id="test-notice"></div><script type="module">import '/style.css'; import {ServerManager} from '/servers.ts'; window.manager=new ServerManager(document.querySelector('main'),text=>document.querySelector('#test-notice').textContent=text); await window.manager.start(); window.testReady=true;</script></body></html>` }));
const dialogs = () => page.locator("dialog[open]").last();
const action = (name, scope = dialogs()) => scope.locator(`[data-server-action="${name}"]`).first();
const work = () => page.locator("#test-workspace");
async function deleteSelected() {
  const id = await action('delete', work()).getAttribute('data-id');
  await action('delete', work()).click(); await action('confirm-yes').click(); await expectDialogs(0);
  await work().locator(`[data-server-action="select"][data-id="${id}"]`).waitFor({ state: 'detached' });
  await work().locator('.catalog-detail [data-server-action="edit"]').waitFor({ state: 'detached' });
}
const field = key => dialogs().locator(`[data-field="${key}"]`);
const read = () => bridge.call("server_catalog");
async function expectDialogs(n) { try {await page.waitForFunction(n => document.querySelectorAll("dialog[open]").length === n, n);} catch(error){console.log(await page.locator("dialog[open]").allTextContents());throw error;} }
async function save() { await dialogs().locator('button[type="submit"]').click(); }
async function closeDirty() { await action("cancel").first().click(); await action("confirm-yes").click(); await expectDialogs(0); }
async function load() { await page.goto(url); await page.waitForFunction(() => window.testReady === true); }
const checks = [];
try {
  failCatalogRead=true;await load();await work().locator('.error-box').waitFor();
  failCatalogRead=false;await action('reload',work()).click();
  await work().locator('.error-box').waitFor({state:'detached'});
  assert.equal(await work().locator('.catalog-page').count(),1,'An initial read failure can be retried without a catalog snapshot');
  await load();
  assert.equal(await action('copy-all',work()).isDisabled(),true,'Empty catalogs cannot be copied');
  assert.equal(await action('share-menu',work()).count(),0);
  const search=work().locator('#catalog-query');await search.focus();
  await search.evaluate(input=>{window.searchInput=input;window.searchDisconnects=0;const ancestors=new Set();for(let n=input;n&&n!==document.body;n=n.parentNode)ancestors.add(n);new MutationObserver(records=>{for(const r of records)for(const n of r.removedNodes)if(ancestors.has(n))searchDisconnects++;}).observe(document.body,{childList:true,subtree:true});});
  const ime=await page.context().newCDPSession(page);
  await ime.send('Input.imeSetComposition',{text:'zhong',selectionStart:5,selectionEnd:5});await page.evaluate(()=>manager.start());
  await ime.send('Input.imeSetComposition',{text:'zhongwen',selectionStart:8,selectionEnd:8});
  assert.equal(await search.inputValue(),'zhongwen');assert.equal(await search.evaluate(input=>input===searchInput&&searchDisconnects===0),true);
  await ime.send('Input.insertText',{text:'中文'});assert.equal(await search.inputValue(),'中文');await ime.detach();await search.fill('');
  assert.equal(await work().locator(".catalog-page").evaluate(el => el.getBoundingClientRect().width), 1100);
  await action("new-server", work()).click(); await field("host").fill("gateway.invalid"); await field("username").fill("ops");
  assert.equal(await field("remember").isChecked(), true);
  await field("password").fill("ui-saved-secret"); await field("name").fill("入口跳板");
  await field("routed").selectOption("true"); await field("password").focus();
  await field("password").evaluate(el=>el.setSelectionRange(1,4,'backward')); await field("password").press("Enter");
  await dialogs().locator(".error-box").waitFor();
  assert.deepEqual(await field("password").evaluate(el=>({focused:el===document.activeElement,value:el.value,start:el.selectionStart,end:el.selectionEnd,direction:el.selectionDirection})),
    {focused:true,value:"ui-saved-secret",start:1,end:4,direction:"backward"});
  await field("routed").selectOption("false"); await save(); await expectDialogs(0);
  checks.push("资料验证失败刷新保留密码草稿、焦点、选区和方向");
  let catalog = await read(); const gateway = catalog.servers[0].id;
  assert.equal(catalog.servers[0].authentication.hasSavedSecret, true); assert(!JSON.stringify(catalog).includes("ui-saved-secret"));
  assert(fs.readFileSync(path.join(directory, "servers.json"), "utf8").includes("ui-saved-secret"));
  await action("edit", work()).click(); assert.equal(await field("password").inputValue(), "");
  assert.equal(await field("remember").isChecked(), true);
  await field("name").fill("公网入口"); await save(); await expectDialogs(0);
  checks.push("创建、编辑、秘密不回填和留空保持");

  const beforeCopy = await read();
  assert.equal(await action('record-menu',work()).count(),0);
  for(const name of ['edit','copy','delete'])assert.equal(await action(name,work()).isVisible(),true);
  await action('delete',work()).click();await expectDialogs(1);
  await action('confirm-no').click();await expectDialogs(0);assert.deepEqual(await read(),beforeCopy);
  await action('copy',work()).focus();await page.keyboard.press('Enter');await expectDialogs(1);
  assert.equal(await page.locator('.catalog-action-menu').count(),0);
  assert.equal(await dialogs().locator('h2').innerText(),'复制服务器');
  assert.equal(await field('password').inputValue(),'');
  assert.match(await dialogs().locator('[id$="-secret-status"]').innerText(),/沿用.*已保存密码/);
  assert.deepEqual(await field('host').evaluate(el=>({focused:el===document.activeElement,start:el.selectionStart,end:el.selectionEnd})),{focused:true,start:0,end:'gateway.invalid'.length});
  assert.deepEqual(await read(),beforeCopy);
  await page.keyboard.press('Escape');await expectDialogs(0);assert.deepEqual(await read(),beforeCopy);
  assert.equal(await action('copy',work()).evaluate(el=>el===document.activeElement),true);
  checks.push('详情直接操作、键盘复制与取消焦点恢复、删除取消不修改资料');
  await action('copy',work()).click(); await field('host').fill('copy.invalid'); await save(); await expectDialogs(0);
  let copied=(await read()).servers.find(s=>s.host==='copy.invalid');
  assert(copied.authentication.hasSavedSecret);
  assert.equal(JSON.parse(fs.readFileSync(path.join(directory,'servers.json'),'utf8')).servers.find(s=>s.id===copied.id).authentication.savedPassword,'ui-saved-secret');
  await deleteSelected();
  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).click();
  await action('copy',work()).click();await field('host').fill('retry-copy.invalid');await field('tagInput').fill('复制标签');
  await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'saveTag',id:null,name:'并发标签'}});
  await save();await dialogs().locator('.error-box:not([hidden])').waitFor();
  await action('reload-editor').click();assert.equal(await field('host').inputValue(),'retry-copy.invalid');
  assert.match(await dialogs().innerText(),/输入已保留/);
  blockTarget(path.join(directory,'servers.json'));await save();await dialogs().locator('.error-box:not([hidden])').waitFor();
  assert.equal((await read()).servers.length,1);assert(!(await read()).tags.some(t=>t.name==='复制标签'));
  restoreTarget(path.join(directory,'servers.json'));await save();await expectDialogs(0);
  copied=(await read()).servers.find(s=>s.host==='retry-copy.invalid');assert(copied.authentication.hasSavedSecret);
  await deleteSelected();
  for(const tag of (await read()).tags)await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'deleteTag',id:tag.id}});
  await action('reload',work()).click();await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).click();
  await action('copy',work()).click();await field('password').fill('replacement');assert.match(await dialogs().locator('[id$="-secret-status"]').innerText(),/新输入/);
  await field('password').fill('');await action('clear-secret').click();assert.match(await dialogs().locator('[id$="-secret-status"]').innerText(),/不沿用/);
  await field('password').fill('replacement-after-opt-out');await field('password').fill('');
  assert.match(await dialogs().locator('[id$="-secret-status"]').innerText(),/不沿用/);
  await save();await expectDialogs(0);assert.equal((await read()).servers.at(-1).authentication.hasSavedSecret,false);
  await deleteSelected();
  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).click();
  checks.push('复制服务器预填与地址全选、取消不落盘、密码独立沿用、冲突刷新及写入失败保留草稿');

  await action("edit", work()).click();await field('tagInput').fill('生产环境');await field('tagInput').press('Enter');
  assert.equal((await read()).tags.length,0);await closeDirty();
  assert.equal((await read()).tags.length,0);checks.push('取消服务器表单不留下新标签');
  await action("new-menu",work()).click();
  await page.locator('.catalog-action-menu [data-menu-action="new-proxy"]').click();
  await field("host").fill("127.0.0.1"); await field("authType").selectOption("usernamePassword");
  assert.equal(await field("remember").isChecked(), true);
  await field("username").fill("proxy-ops"); await field("password").fill("ui-proxy-secret"); await field("remember").check();
  await field("name").fill("回环代理"); await save(); await expectDialogs(0);
  catalog = await read(); const proxy = catalog.proxies[0].id;
  for(const name of ['edit','copy','delete'])assert.equal(await action(name,work()).isVisible(),true);
  await action('copy',work()).click(); await page.waitForFunction(() => document.querySelector(".catalog-detail h2")?.textContent?.includes("副本"));
  catalog = await read(); assert.equal(catalog.proxies[1].authentication.hasSavedSecret, false);
  await deleteSelected();
  assert.equal((await read()).proxies.length, 1); checks.push("代理增删改查和复制不带秘密");

  await action("new-server", work()).click();
  await field("host").fill("db.internal"); await field("username").fill("deploy"); await field("name").fill("生产数据库");
  await field("tagInput").fill("生产环境"); await field("routed").selectOption("true");
  await action("add-ssh").focus();await page.keyboard.press('Enter');
  assert.equal(await dialogs().locator('[data-step="0"]').evaluate(el=>el===document.activeElement),true,'New SSH hop focuses its source selector');
  await dialogs().locator('[data-step="0"]').selectOption(gateway);
  await action("add-socks").focus();await page.keyboard.press('Enter');
  assert.equal(await dialogs().locator('[data-step="1"]').evaluate(el=>el===document.activeElement),true,'New SOCKS hop focuses its source selector');
  await dialogs().locator('[data-step="1"]').selectOption(proxy);
  await action("add-socks").click(); await dialogs().locator('[data-step="2"]').selectOption(proxy);
  assert((await dialogs().locator(".route-preview").innerText()).includes("公网入口 的回环地址"));
  assert.equal(await dialogs().locator('.route-editor-step .route-symbol svg').count(),3);
  assert.equal(await dialogs().locator('.route-preview .route-symbol svg').count(),5);
  await dialogs().locator('[data-server-action="step-up"][data-index="2"]').click();
  await field("password").fill("ui-session-secret"); await field("remember").uncheck(); await save(); await expectDialogs(0);
  catalog = await read(); const target = catalog.servers.find(s => s.name === "生产数据库").id;
  assert.deepEqual(catalog.servers.find(s => s.id === target).route.map(r => r.kind), ["ssh", "socks5", "socks5"]);
  const targetBeforeCopy=catalog.servers.find(s=>s.id===target);
  await action('copy',work()).click();assert.equal(await field('remember').isChecked(),false);
  const copySteps=await dialogs().locator('.route-editor-step').evaluateAll(nodes=>nodes.map(n=>n.dataset.domKey));
  assert.equal(new Set(copySteps).size,3);
  await field('host').fill('session-copy.internal');await save();await expectDialogs(0);
  const sessionCopy=(await read()).servers.find(s=>s.host==='session-copy.internal');
  assert.equal(sessionCopy.authentication.hasSessionSecret,true);assert.equal(sessionCopy.authentication.hasSavedSecret,false);
  assert.deepEqual(sessionCopy.tagIds,targetBeforeCopy.tagIds);
  assert(sessionCopy.route.every((step,i)=>step.id!==targetBeforeCopy.route[i].id));
  assert.deepEqual((await read()).servers.find(s=>s.id===target),targetBeforeCopy);
  await deleteSelected();
  await work().locator(`[data-server-action="select"][data-id="${target}"]:not(.path-node)`).click();
  checks.push('临时密码、多标签和多层中转复制，独立路径步骤与原资料保持');
  assert.equal(catalog.servers.find(s => s.id === target).authentication.hasSessionSecret, true);
  checks.push("三层混合路径、重复代理、重排、回环来源与临时秘密");

  await action("edit",work()).click();await field('tagInput').fill('数据库，数据库');await save();await expectDialogs(0);
  catalog=await read();const databaseTag=catalog.tags.find(t=>t.name==='数据库').id;
  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();await action('edit',work()).click();await field('tagInput').fill('数据库');await save();await expectDialogs(0);
  await work().locator(`[data-filter-tag="${databaseTag}"]`).click();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),2);
  await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),2,'AND filters matching routes without changing an existing ancestor target role');
  assert.equal(await work().locator('.path-node').count(),2,'Every explicit ancestor remains visible with its canonical role');
  assert.equal(await work().locator('.path-node[data-connect-server-on-double-click]').count(),0);
  await work().locator(`.connection-node[data-id="${gateway}"]`).first().click();
  assert.equal(await work().locator('.catalog-detail [data-connect-server]').count(),1,'Filtering preserves a shared prefix that is also a saved target');
  await action('clear-tags',work()).click();
  assert.equal(await work().locator('[data-filter-tag][aria-pressed=true]').count(),0);
  assert.equal(await work().locator('.catalog-detail [data-connect-server]').count(),1);
  await work().locator(`[data-filter-tag="${databaseTag}"]`).click();
  await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  await work().locator(`[data-server-action="select"][data-id="${target}"]`).click();
  fs.mkdirSync('artifacts/connection-tree',{recursive:true});
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await page.evaluate(async()=>{await new Promise(requestAnimationFrame);await Promise.allSettled(document.getAnimations().map(animation=>animation.finished));});
    await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
    const guideOffset=await work().locator('.connection-tree').evaluate(tree=>{
      const top=tree.getBoundingClientRect().top,rows=[...tree.querySelectorAll('.node-card')],paths=[...tree.querySelectorAll('.tree-lines path:not(.on-route)')];
      return Math.max(...rows.map((row,i)=>{const box=row.getBoundingClientRect(),end=paths[i].getPointAtLength(paths[i].getTotalLength());return Math.abs(end.y-(box.top-top+box.height/2));}));
    });
    assert(guideOffset<=1,`${theme} live theme switch keeps guides aligned: ${guideOffset}px`);
    await page.screenshot({path:`artifacts/connection-tree/${theme}.png`});
  }
  const routePreview = work().locator('.catalog-detail .route-preview');
  assert.equal(await routePreview.locator('.route-symbol svg').count(),5,'Origin and every SSH/SOCKS5/target node use icons');
  assert((await routePreview.locator('.route-symbol').allTextContents()).every(text=>!text.trim()),'Route circles contain no step numbers');
  const routeView={attemptId:'route-progress',serverId:target,windowId:'main',state:'connecting',stage:'dns',nodeIndex:0,nodeCount:4,endpoint:'db.internal:22',question:null,context:null,error:null,sentBytes:'0',receivedBytes:'0'};
  const showRoute=async patch=>page.evaluate(view=>window.manager.setConnections([view]),{...routeView,...patch});
  for(const [patch,flow,complete] of [
    [{stage:'dns'},0,0], [{stage:'sshTunnel'},1,1], [{stage:'socks5',nodeIndex:1},2,2],
    [{stage:'authentication',nodeIndex:3},3,3],
  ]){
    await showRoute(patch);
    assert.equal(await routePreview.locator('.route-edge-flow').count(),1);
    assert.equal(await routePreview.locator('.route-edge').nth(flow).evaluate(el=>el.classList.contains('route-edge-flow')),true,'Flow follows the actual incoming or forwarded connection segment');
    assert.equal(await routePreview.locator('.route-edge-complete').count(),complete);
  }
  await showRoute({stage:'credentials',nodeIndex:3,question:{kind:'credentials',id:'route-question'}});
  assert.equal(await routePreview.locator('.route-edge-flow,.route-edge-complete').count(),0,'Credentials are collected before network connection; later nodes do not imply prior progress');
  assert.match(await routePreview.locator('.route-waiting').innerText(),/等待确认/);
  await showRoute({stage:'sshHandshake',nodeIndex:3,question:{kind:'hostKey',id:'route-question'}});
  assert.equal(await routePreview.locator('.route-edge-flow').count(),0,'Host confirmation pauses flow');
  assert.equal(await routePreview.locator('.route-edge-complete').count(),3);
  await showRoute({state:'failed',stage:'socks5',nodeIndex:1,error:{stage:'socks5',message:'代理连接失败',code:'timeout'}});
  assert.equal(await routePreview.locator('.route-edge-flow').count(),0);
  assert.equal(await routePreview.locator('.route-edge-complete').count(),2);
  assert.match(await routePreview.locator('.route-failed').innerText(),/SOCKS5.*连接失败/s);
  await showRoute({state:'connected',nodeIndex:3,stage:'connected'});
  assert.equal(await routePreview.locator('.route-edge-flow').count(),4);
  const packet=routePreview.locator('.route-edge-flow > span').first();
  await packet.evaluate(el=>window.routeAnimation=el.getAnimations()[0]);
  await showRoute({state:'connected',nodeIndex:3,stage:'connected'});
  assert(await packet.evaluate(el=>el.getAnimations()[0]===window.routeAnimation),'Repeated snapshots preserve the running line animation');
  await routePreview.evaluate(el=>el.style.marginTop='1000px');
  await page.waitForFunction(()=>document.querySelector('#test-workspace').classList.contains('route-motion-paused'));
  assert.equal(await packet.evaluate(el=>getComputedStyle(el).animationPlayState),'paused');
  await routePreview.evaluate(el=>el.style.removeProperty('margin-top'));
  await page.waitForFunction(()=>!document.querySelector('#test-workspace').classList.contains('route-motion-paused'));
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
  assert.equal(await packet.evaluate(el=>getComputedStyle(el).animationPlayState),'paused');
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});
  await page.emulateMedia({reducedMotion:'reduce'});
  assert.equal(await packet.evaluate(el=>getComputedStyle(el).animationName),'none');
  await page.emulateMedia({reducedMotion:'no-preference'});
  fs.mkdirSync('artifacts/connection-route',{recursive:true});
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await work().locator('.detail-route').screenshot({path:`artifacts/connection-route/${theme}.png`});
  }
  await showRoute({state:'connected',nodeCount:2});
  assert.equal(await routePreview.locator('.route-edge-flow').count(),0,'Changed route lengths do not reuse old connection progress');
  await showRoute({state:'cancelled'});
  assert.equal(await routePreview.locator('.route-edge-flow').count(),0);
  await page.evaluate(()=>window.manager.setConnections([]));
  checks.push('图标访问路径、真实阶段方向、凭据/指纹暂停、失败定位、后台/屏外/减少动态及快照连续性');
  await search.fill('公网');
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),2,'Searching retains its matching target and the canonical ancestor target role');
  await action('clear-tags',work()).click();assert.equal(await search.inputValue(),'公网','All clears labels without clearing search');
  await work().locator(`[data-filter-tag="${databaseTag}"]`).click();
  await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  const selectedPath=await work().locator('.connection-node.selected').getAttribute('data-path');
  await search.evaluate(input=>window.refreshSearch=input);
  await action('reload',work()).click();
  assert.equal(await search.evaluate(input=>input===refreshSearch),true);
  assert.equal(await work().locator('.connection-node.selected').getAttribute('data-path'),selectedPath);
  assert.equal(await work().locator('[data-filter-tag][aria-pressed=true]').count(),2);
  failCatalogRead=true;await action('reload',work()).click();
  await work().locator('.error-box').waitFor();
  assert.equal(await work().locator('.connection-node.selected').getAttribute('data-path'),selectedPath);
  assert.equal(await work().locator('[data-connect-server]').isDisabled(),true);
  assert.equal(await action('new-server',work()).isDisabled(),true);
  for(const name of ['edit','copy','delete'])assert.equal(await action(name,work()).isDisabled(),true);
  failCatalogRead=false;await action('reload',work()).click();
  assert.equal(await work().locator('.error-box').count(),0);assert.equal(await search.inputValue(),'公网');
  await search.fill('没有匹配的资料');
  assert.equal(await work().locator('.catalog-detail h2').innerText(),'生产数据库');
  await action('reveal',work()).click();assert.equal(await search.inputValue(),'');
  assert.equal(await work().locator('.connection-node.selected').getAttribute('data-path'),selectedPath);
  holdCatalogRead=true;await page.evaluate(()=>{window.pendingCatalog=manager.start();});
  while(!catalogReadHeld)await new Promise(resolve=>setTimeout(resolve,10));
  await action('edit',work()).click();await field('name').fill('生产数据库更新');await save();await expectDialogs(0);
  releaseCatalogRead();await page.evaluate(()=>pendingCatalog);
  assert.equal(await work().locator('.catalog-detail h2').innerText(),'生产数据库更新','A delayed refresh cannot overwrite a newer save');
  checks.push('中转搜索、全部保留搜索、刷新保留选择和筛选、失败保留树、隐藏资料恢复及过期读取隔离');
  const ancestor=work().locator(`.connection-node[data-id="${gateway}"]`).first();
  assert.equal(await ancestor.locator('.node-card > .tree-toggle + .node-symbol').count(),1,'The disclosure belongs to its card, before the type icon');
  const selectedBeforeCollapse=await work().locator('.connection-node.selected').getAttribute('data-path');
  assert.equal(await ancestor.locator('.tree-toggle svg path').getAttribute('d'),'m9 6 6 6-6 6');
  assert.equal(await ancestor.locator('.tree-toggle svg').evaluate(svg=>getComputedStyle(svg).transform),'matrix(0, 1, -1, 0, 0, 0)','Expanded branch points down');
  await ancestor.locator('[data-server-action="collapse"]').click();
  assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  assert.equal(await ancestor.locator('.tree-toggle').getAttribute('aria-expanded'),'false');
  assert.equal(await ancestor.evaluate(row=>row===document.activeElement),true,'Collapsing restores focus to the tree row');
  await ancestor.locator('.tree-toggle svg').evaluate(async svg=>{await Promise.all(svg.getAnimations().map(a=>a.finished.catch(()=>{})));});
  assert.equal(await ancestor.locator('.tree-toggle svg').evaluate(svg=>getComputedStyle(svg).transform),'matrix(1, 0, 0, 1, 0, 0)','Collapsed branch points right');
  await action('reload',work()).click();assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  assert.equal(await work().locator('.catalog-detail h2').innerText(),'生产数据库更新');
  await search.fill('公网');assert.equal(await ancestor.getAttribute('aria-expanded'),'true');
  assert.equal(await ancestor.locator('.tree-toggle').isDisabled(),true,'Filtering temporarily expands the complete route');
  assert.equal(await ancestor.locator('.tree-toggle').getAttribute('aria-expanded'),'true');
  await ancestor.locator('.tree-toggle').evaluate(async button=>{await Promise.allSettled(button.getAnimations().map(animation=>animation.finished));});
  assert.equal(await ancestor.locator('.tree-toggle').evaluate(button=>getComputedStyle(button).backgroundColor),'rgba(0, 0, 0, 0)','The disclosure no longer masks tree guides');
  await search.fill('');assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  await ancestor.focus();await page.keyboard.press('ArrowRight');
  assert.equal(await ancestor.getAttribute('aria-expanded'),'true');
  await page.keyboard.press('ArrowRight');
  assert.equal(await work().locator('.connection-node:focus').getAttribute('data-parent'),await ancestor.getAttribute('data-path'),'Right enters the first child of an expanded branch');
  await ancestor.focus();
  assert.equal(await work().locator('.connection-node.selected').getAttribute('data-path'),selectedBeforeCollapse,'Disclosure and arrow keys preserve the selected record');
  checks.push('刷新保留折叠和详情，筛选临时展开后恢复原折叠');
  checks.push('多标签 AND、完整混合祖先路径、上下文节点无连接入口及三主题节点展示');

  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();
  await action('delete',work()).click(); assert((await dialogs().innerText()).includes("生产数据库")); await action("confirm-yes").click();
  await expectDialogs(0); assert.equal((await read()).servers.length, 2);
  await work().locator("#catalog-query").fill("deploy"); assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 2);
  await work().locator("#catalog-query").fill(""); await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 2); await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  checks.push("引用阻止删除，按账号搜索和标签筛选");

  await action("new-server", work()).click(); await field("host").fill("unsaved.internal"); await action("inline-proxy").count().then(async n => {
    // Inline creation is inside the routed section.
    if (!n) await field("routed").selectOption("true");
  });
  await field('tagInput').fill('父表单待保存标签');await field('tagInput').press('Enter');
  await field('tagInput').fill('正在输入的标签');await field('password').fill('nested-draft-secret');
  await action("inline-proxy").click();
  const nestedLabels=await dialogs().locator('[data-field]').evaluateAll(nodes=>nodes.filter(node=>node.type!=='checkbox').map(node=>({field:node.dataset.field,labels:[...node.labels].map(label=>({text:label.textContent,owns:label.control===node&&label.closest('dialog')===node.closest('dialog')}))})));
  assert(nestedLabels.every(item=>item.labels.length===1&&item.labels[0].owns),`Nested proxy fields own their labels: ${JSON.stringify(nestedLabels)}`);
  assert.equal(await page.locator('[id]').evaluateAll(nodes=>new Set(nodes.map(node=>node.id)).size===nodes.length),true,'Nested editors have unique DOM IDs');
  await field("authType").selectOption("usernamePassword");
  await field("username").fill("proxy-ops");
  await field("host").fill("proxy.internal"); await field("name").fill("单独保留代理");
  await save(); await expectDialogs(1); assert.equal(await dialogs().locator("[data-step]").count(), 1);
  assert.equal(await field('tagInput').inputValue(),'正在输入的标签');
  assert.equal(await field('password').inputValue(),'nested-draft-secret');
  assert.match(await dialogs().locator('.catalog-draft-tags').innerText(),/父表单待保存标签/);
  await closeDirty(); assert.equal((await read()).proxies.length, 2);
  assert(!(await read()).tags.some(tag=>tag.name==='父表单待保存标签'||tag.name==='正在输入的标签'));
  checks.push("内联代理保存保留父表单标签、输入和凭据草稿，取消服务器仍保留代理");

  await work().locator(`[data-server-action="select"][data-id="${target}"]`).click(); await action("edit", work()).click(); await field("name").fill("不可覆盖的草稿");
  catalog = await read(); await bridge.call("change_catalog", { revision: catalog.revision, change: { op: "patchServer", id: target, patch: { name: "外部更新的节点" } } });
  await page.evaluate(()=>manager.start());assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await save(); await dialogs().locator(".error-box").waitFor(); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await page.keyboard.press("Escape"); await action("confirm-no").click(); await expectDialogs(1); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await closeDirty(); await action("reload", work()).click();
  checks.push("旧修订拒绝覆盖并保留草稿，Escape 放弃确认");

  await action("edit", work()).click(); await field("name").fill("保存失败后重试");
  blockTarget(path.join(directory, "servers.json")); await save(); await dialogs().locator(".error-box").waitFor();
  assert.equal(await field("name").inputValue(), "保存失败后重试");
  restoreTarget(path.join(directory, "servers.json")); await save(); await expectDialogs(0);
  checks.push("写入失败保留输入并可重试");

  const productionTag=(await read()).tags.find(tag=>tag.name==='生产环境').id;
  assert.equal(await action('tags',work()).count(),0);assert.equal(await action('density',work()).count(),0);
  await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'deleteTag',id:productionTag}});
  await page.evaluate(()=>manager.start());assert.deepEqual((await read()).servers.find(s=>s.id===target).tagIds,[databaseTag]);
  checks.push('已移除管理标签和密度入口，旧标签数据变更仍正确刷新');
  for (const theme of ["system", "light", "dark", "dopamine"]) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    for (const [width, height] of [[1100, 720], [900, 720], [720, 480]]) {
      await page.setViewportSize({ width, height });
      if(width===720&&await action('back',work()).isVisible())await action('back',work()).click();
      const row=work().locator(`[data-server-action="select"][data-id="${target}"]`);
      await row.focus();await page.keyboard.press('Enter');
      assert.equal(await page.evaluate(()=>document.activeElement===document.body),false,`${theme}/${width} selecting a server keeps contextual focus`);
      if(width===1100){await row.evaluate(async node=>{await Promise.allSettled(node.getAnimations({subtree:true}).map(animation=>animation.finished));});const ratios=await row.evaluate(row=>{const l=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};const b=l(getComputedStyle(row.querySelector('.node-card')).backgroundColor);return [...row.querySelectorAll('.node-endpoint,.node-tags,.node-state')].map(node=>{const a=l(getComputedStyle(node).color);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);});});assert(ratios.length&&ratios.every(r=>r>=4.5),`${theme} selected server contrast: ${ratios}`);}
      for(const name of ['edit','copy','delete'])assert.equal(await action(name,work()).isVisible(),true);
      assert(await work().locator('.catalog-detail-actions').evaluate(row=>{
        const bounds=row.getBoundingClientRect(),buttons=[...row.querySelectorAll('button')].map(button=>button.getBoundingClientRect());
        return row.scrollWidth<=row.clientWidth+1&&buttons.every((box,i)=>box.left>=bounds.left&&box.right<=bounds.right+1&&buttons.slice(i+1).every(other=>box.right<=other.left||other.right<=box.left||box.bottom<=other.top||other.bottom<=box.top));
      }),`${theme}/${width} detail actions stay in bounds without overlap`);
      fs.mkdirSync('artifacts/server-actions',{recursive:true});
      await work().locator('.catalog-detail').screenshot({path:`artifacts/server-actions/${theme}-${width}.png`});
      await action('copy',work()).click();
      await dialogs().evaluate(async d=>{await Promise.allSettled(d.getAnimations({subtree:true}).map(a=>a.finished));});
      assert(await dialogs().evaluate(d=>d.scrollWidth<=d.clientWidth+1));
      assert.equal(await field('host').evaluate(el=>el===document.activeElement&&el.selectionStart===0&&el.selectionEnd===el.value.length),true);
      if(process.env.SODECK_COPY_REVIEW_DIR){fs.mkdirSync(process.env.SODECK_COPY_REVIEW_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.SODECK_COPY_REVIEW_DIR,`${theme}-${width}-connection.png`)});}
      await field('password').scrollIntoViewIfNeeded();
      const secretFooter=await dialogs().locator('.catalog-dialog-footer').boundingBox();assert(secretFooter.y+secretFooter.height<=height);
      if(process.env.SODECK_COPY_REVIEW_DIR)await page.screenshot({path:path.join(process.env.SODECK_COPY_REVIEW_DIR,`${theme}-${width}-authentication.png`)});
      await page.keyboard.press('Escape');await expectDialogs(0);
      if(theme==='light'&&width===720){
        const original=(await read()).servers.find(s=>s.id===target);
        const longDraft={...original,name:'W'.repeat(200),authentication:{...original.authentication,secret:{action:'keep',remember:original.authentication.hasSavedSecret}}};
        await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'saveServer',draft:longDraft}});
        await action('back',work()).click();await action('reload',work()).click();await row.click();await action('copy',work()).click();
        await dialogs().evaluate(async d=>{await Promise.allSettled(d.getAnimations({subtree:true}).map(a=>a.finished));});
        assert(await dialogs().evaluate(d=>d.scrollWidth<=d.clientWidth+1));
        assert(await dialogs().locator('.catalog-dialog-heading').evaluate(h=>{const close=h.querySelector('button').getBoundingClientRect(),bounds=h.getBoundingClientRect();return close.right<=bounds.right&&close.left>=bounds.left;}));
        if(process.env.SODECK_COPY_REVIEW_DIR)await page.screenshot({path:path.join(process.env.SODECK_COPY_REVIEW_DIR,'light-720-long-name.png')});
        await field('password').scrollIntoViewIfNeeded();assert(await dialogs().evaluate(d=>d.scrollWidth<=d.clientWidth+1));
        if(process.env.SODECK_COPY_REVIEW_DIR)await page.screenshot({path:path.join(process.env.SODECK_COPY_REVIEW_DIR,'light-720-long-credential.png')});
        await page.keyboard.press('Escape');await expectDialogs(0);
        longDraft.name=original.name;await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'saveServer',draft:longDraft}});
        await action('back',work()).click();await action('reload',work()).click();await row.click();await action('copy',work()).waitFor({state:'visible'});
      }
      if(width===720){await action('back',work()).focus();await page.keyboard.press('Enter');assert.equal(await row.evaluate(el=>el===document.activeElement),true,'Returning to the list focuses the original server');}
      await action("new-server", work()).click();
      await field("routed").selectOption("true"); await action("add-socks").click();
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert(await dialogs().evaluate(d => d.scrollWidth <= d.clientWidth + 1));
      const footer = await dialogs().locator(".catalog-dialog-footer").boundingBox(); assert(footer.y + footer.height <= height);
      await page.keyboard.press("Tab"); assert(await page.evaluate(() => document.activeElement.closest("dialog") !== null));
      await closeDirty();
    }
  }
  await page.setViewportSize({ width: 1100, height: 720 });
  if (process.env.SODECK_UI_SCREENSHOT) await page.screenshot({ path: process.env.SODECK_UI_SCREENSHOT });
  checks.push("系统、浅色、深色、多巴胺主题与 1100/900/720px 操作换行、表单、焦点和固定页脚");

  await page.setViewportSize({width:1100,height:720});
  await action('clear-filters',work()).count()&&await action('clear-filters',work()).click();
  await page.evaluate(()=>{manager.state.query='';manager.state.tags=[];manager.render();});
  const menu=page.locator('.catalog-action-menu');
  const sourceRow=work().locator(`.connection-node[data-id="${target}"]`).first();
  await sourceRow.scrollIntoViewIfNeeded();await sourceRow.click({button:'right'});
  await menu.waitFor({state:'visible'});assert.equal(await menu.locator('[data-menu-action="collapse"]').count(),0);
  await sourceRow.locator('strong').click();await menu.waitFor({state:'detached'});
  assert.equal(await page.evaluate(()=>manager.menu.source),undefined);
  await sourceRow.click({button:'right'});await menu.locator('.catalog-menu-title').click();await page.keyboard.press('Escape');await menu.waitFor({state:'detached'});
  assert(await sourceRow.evaluate(node=>node===document.activeElement));
  await sourceRow.click({button:'right'});await menu.locator('[data-menu-action="edit"]').click();await expectDialogs(1);
  assert.equal(await page.locator('body > [inert][aria-hidden="true"]').filter({hasText:'经此节点新增服务器'}).count(),0,'The actual editor handoff clears the menu closing picture');
  await page.keyboard.press('Escape');await expectDialogs(0);
  assert(await sourceRow.evaluate(node=>node===document.activeElement));
  await sourceRow.click({button:'right'});await menu.locator('[data-menu-action="copy"]').click();await expectDialogs(1);await page.keyboard.press('Escape');await expectDialogs(0);
  assert(await sourceRow.evaluate(node=>node===document.activeElement));
  await sourceRow.click({button:'right'});
  assert.match(await menu.innerText(),/经此节点新增服务器/);
  const beforeVia=await read();await menu.locator('[data-menu-action="new-via"]').click();await expectDialogs(1);
  assert.equal(await field('host').inputValue(),'');assert.equal(await field('username').inputValue(),'');assert.equal(await field('password').inputValue(),'');
  assert.equal(await dialogs().locator('.route-editor-step').count(),beforeVia.servers.find(s=>s.id===target).route.length+1);
  const expectedVia=[...beforeVia.servers.find(s=>s.id===target).route.map(r=>r.kind==='ssh'?r.serverId:r.proxyId),target];
  assert.deepEqual(await dialogs().locator('[data-step]').evaluateAll(nodes=>nodes.map(n=>n.value)),expectedVia);
  await page.keyboard.press('Escape');await expectDialogs(0);assert.equal((await read()).servers.length,beforeVia.servers.length);
  assert.equal(await sourceRow.evaluate(row=>row===document.activeElement),true);
  await sourceRow.focus();await page.keyboard.press('Shift+F10');await menu.waitFor({state:'visible'});
  await menu.locator('[data-menu-action="new-via"]').click();await field('host').fill('via-context.internal');await field('username').fill('ops');
  await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'saveTag',id:null,name:'右键并发标签'}});
  await save();assert.match(await dialogs().locator('.error-box').innerText(),/目录已更新/);
  await action('reload-editor').click();assert.equal(await field('host').inputValue(),'via-context.internal');
  assert.deepEqual(await dialogs().locator('[data-step]').evaluateAll(nodes=>nodes.map(n=>n.value)),expectedVia);
  await save();await expectDialogs(0);
  const via=(await read()).servers.find(r=>r.host==='via-context.internal');assert(via);assert(!via.authentication.hasSavedSecret);assert(!via.authentication.hasSessionSecret);
  assert.deepEqual(via.route.map(r=>r.kind==='ssh'?r.serverId:r.proxyId),expectedVia);
  const viaRow=work().locator(`.connection-node[data-id="${via.id}"]`);assert.equal(await viaRow.getAttribute('aria-selected'),'true');
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await viaRow.click({button:'right'});await menu.waitFor({state:'visible'});
    const bounds=await menu.boundingBox();assert(bounds.x>=0&&bounds.y>=0&&bounds.x+bounds.width<=1101&&bounds.y+bounds.height<=721);
    await page.evaluate(()=>Promise.allSettled(document.getAnimations().map(a=>a.finished)));
    fs.mkdirSync('artifacts/server-context-menu',{recursive:true});await page.screenshot({path:`artifacts/server-context-menu/${theme}.png`});
    await page.keyboard.press('Escape');
  }
  await page.setViewportSize({width:720,height:540});await action('back',work()).click();
  await viaRow.click({button:'right'});await menu.waitFor({state:'visible'});
  assert.equal(await work().locator('.catalog-layout').evaluate(node=>node.classList.contains('has-detail')),false,'Narrow context selection stays in the tree');
  await page.keyboard.press('Escape');await viaRow.focus();await page.keyboard.press('Shift+F10');await page.keyboard.press('End');
  assert.equal(await menu.locator('[data-menu-action="delete"]').evaluate(node=>node===document.activeElement),true);
  await page.keyboard.press('Escape');assert(await viaRow.evaluate(node=>node===document.activeElement));
  await viaRow.click({button:'right'});await menu.locator('[data-menu-action="delete"]').click();await action('confirm-no').click();await expectDialogs(0);
  assert((await read()).servers.some(s=>s.id===via.id));
  await viaRow.click({button:'right'});await menu.locator('[data-menu-action="delete"]').click();await action('confirm-yes').click();await expectDialogs(0);
  assert(!(await read()).servers.some(s=>s.id===via.id));await viaRow.waitFor({state:'detached'});
  await work().locator('#tree-origin').focus();await page.keyboard.press('Shift+F10');await menu.waitFor({state:'visible'});
  assert.deepEqual(await menu.locator('[data-menu-action]').evaluateAll(nodes=>nodes.map(n=>n.dataset.menuAction)),['new-server','new-proxy','reload']);
  await page.keyboard.press('Escape');
  await page.setViewportSize({width:1100,height:720});
  await sourceRow.click({button:'right'});await menu.locator('[data-menu-action="new-via"]').click();await field('host').fill('cancel-via.internal');
  await closeDirty();assert(!(await read()).servers.some(s=>s.host==='cancel-via.internal'));
  await page.evaluate(async()=>{manager.dispose();const {ServerManager}=await import('/servers.ts');window.manager=new ServerManager(document.querySelector('main'),text=>document.querySelector('#test-notice').textContent=text);await manager.start();});
  await sourceRow.click({button:'right'});await menu.locator('[data-menu-action="new-via"]').click();await expectDialogs(1);await page.keyboard.press('Escape');await expectDialogs(0);
  checks.push('右键与键盘菜单、完整路径继承、无凭据复制、冲突保留、取消无残留、三主题边缘定位、窄窗、删除确认和销毁重建');


  await page.setViewportSize({width:1100,height:720});
  await search.fill('没有匹配的服务器');
  holdExport=true;await action('copy-all',work()).click();
  await page.waitForFunction(()=>document.querySelector('[data-server-action="copy-all"]')?.disabled);
  while(!exportHeld)await new Promise(r=>setTimeout(r,20));
  assert.match(await action('copy-all',work()).innerText(),/正在复制/);
  assert.equal(await page.locator('.catalog-action-menu,dialog[open]').count(),0);
  releaseExport();await page.waitForFunction(()=>document.querySelector('#test-notice').textContent.startsWith('已复制全部'));
  assert(clipboardText.startsWith('SODECK1:'));const shareCode=clipboardText;
  const shared=await bridge.call('preview_server_share',{code:shareCode});assert.equal(shared.targetCount,(await read()).servers.length);
  assert.equal(await search.inputValue(),'没有匹配的服务器');
  assert.equal(await page.locator('.catalog-action-menu,dialog[open]').count(),0);
  await search.fill('');
  failClipboard=true;await action('copy-all',work()).click();await expectDialogs(1);
  assert.match(await dialogs().locator('.error-box').innerText(),/剪贴板不可写/);
  assert((await dialogs().locator('textarea').inputValue()).startsWith('SODECK1:'));
  failClipboard=false;await dialogs().locator('[data-share-action="copy"]').click();
  await page.waitForFunction(()=>document.querySelector('[data-share-action="copy"]')?.textContent==='已复制');
  await dialogs().locator('[data-share-action="close"]').first().click();await expectDialogs(0);
  checks.push('工具栏一键复制全部、空目录禁用、生成中禁用、筛选不缩减范围、剪贴板失败保留分享码并可重试');
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    for(const width of [1100,720,420]){
      await page.setViewportSize({width,height:720});
      if(await action('back',work()).isVisible())await action('back',work()).click();
      assert.equal(await action('copy-all',work()).isVisible(),true);
      assert(await work().locator('.catalog-toolbar').evaluate(toolbar=>{
        const bounds=toolbar.getBoundingClientRect();return toolbar.scrollWidth<=toolbar.clientWidth+1&&[...toolbar.querySelectorAll('button')].every(button=>{const box=button.getBoundingClientRect();return box.left>=bounds.left&&box.right<=bounds.right+1;});
      }),`${theme}/${width} copy all toolbar stays in bounds`);
      fs.mkdirSync('artifacts/server-copy-all',{recursive:true});await work().locator('.catalog-toolbar').screenshot({path:`artifacts/server-copy-all/${theme}-${width}.png`});
    }
  }
  await page.setViewportSize({width:1100,height:720});
  await action('import-share',work()).click();await dialogs().locator('textarea').fill('broken');await dialogs().locator('.error-box').waitFor();
  assert.equal(await dialogs().locator('[data-share-action="import"]').isDisabled(),true);
  await dialogs().locator('textarea').fill(shareCode);await dialogs().locator('[data-share-action="import"]:not([disabled])').waitFor();
  const preImport=await read();assert.equal((await read()).revision,preImport.revision);
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    for(const width of [1100,420]){
      await page.setViewportSize({width,height:720});
      assert.equal(await dialogs().evaluate(d=>d.scrollWidth<=d.clientWidth+1&&d.querySelector('.catalog-dialog-footer').getBoundingClientRect().bottom<=d.getBoundingClientRect().bottom+1),true);
      fs.mkdirSync('artifacts/server-sharing',{recursive:true});await dialogs().screenshot({path:'artifacts/server-sharing/'+theme+'-'+width+'.png'});
    }
  }
  await page.setViewportSize({width:1100,height:720});
  await dialogs().locator('[data-share-action="import"]').click();await expectDialogs(0);await work().locator('.catalog-import-results').waitFor();
  const afterImport=await read();assert.equal(afterImport.revision,preImport.revision+1);assert.equal(afterImport.servers.length,preImport.servers.length*2);
  await action('import-share',work()).click();await dialogs().locator('textarea').fill(shareCode);await dialogs().locator('[data-share-action="import"]:not([disabled])').waitFor();
  await dialogs().locator('[data-share-action="close"]').first().click();assert.equal((await read()).revision,afterImport.revision);
  await action('import-share',work()).click();await dialogs().locator('textarea').fill(shareCode);await dialogs().locator('[data-share-action="import"]:not([disabled])').waitFor();
  holdImport=true;await dialogs().locator('[data-share-action="import"]').click();
  while(!importHeld)await new Promise(r=>setTimeout(r,20));
  assert.equal(await dialogs().locator('[data-share-action="close"]').first().isDisabled(),true);
  assert((await page.evaluate(()=>manager.closeInputs('collect'))).some(d=>d.busy));
  await assert.rejects(page.evaluate(()=>manager.closeInputs('discard')),/正在保存/);releaseImport();await expectDialogs(0);
  checks.push('完整分享码复制、非法输入、预览不落盘、整批导入单次修订、取消、三主题与窄窗');
  await bridge.stop(); bridge = startBridge(); await load(); catalog = await read();
  assert.equal(catalog.servers.find(s => s.id === gateway).authentication.hasSavedSecret, true);
  assert(catalog.servers.every(s => !s.authentication.hasSessionSecret));
  checks.push("真实核心进程重开，持久资料保留、临时秘密清除");

  await bridge.stop(); fs.writeFileSync(path.join(directory, "servers.json"), "{bad-json"); bridge = startBridge(); await load();
  await work().locator(".error-box").waitFor(); assert.equal(await action("new-server", work()).isDisabled(), true);
  assert.equal(await action('copy-all',work()).isDisabled(),true);
  await action("recover", work()).click(); await action("confirm-yes").click(); await expectDialogs(0);
  await page.waitForFunction(() => !document.querySelector(".catalog-page > .error-box"));
  assert(fs.readdirSync(directory).some(f => f.startsWith("servers.corrupt-")));
  checks.push("损坏文件阻止覆盖，从备份恢复并保留原文件");
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ passed: checks.length, checks, pageErrors: errors }, null, 2));
} catch (error) {
  await finishTrace(true);
  console.error({ pageErrors: errors });
  throw error;
} finally {
  await finishTrace(false);
  await browser.close(); await vite.close(); await bridge.stop();
  // Only this process's mkdtemp output, never a user supplied directory.
  fs.rmSync(directory, { recursive: true, force: true });
}
