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
let bridge = startBridge();
const vite = await createServer({ server: { host: "127.0.0.1", port: 0, strictPort: false } });
await vite.listen();
const url = vite.resolvedUrls.local[0];
const browser = await chromium.launch({ headless: true, ...(process.env.SODECK_BROWSER_CHANNEL ? { channel: process.env.SODECK_BROWSER_CHANNEL } : {}) });
const page = await browser.newPage({ viewport: { width: 1100, height: 720 } });
const finishTrace = await recordTrace(page.context(), 'server');
const errors = [];
let failCatalogRead = false, holdCatalogRead = false, catalogReadHeld = false, releaseCatalogRead;
page.on("pageerror", error => errors.push(String(error)));
page.on("request", request => { assert.equal(new URL(request.url()).hostname, "127.0.0.1", "Unexpected network request"); });
await page.exposeBinding("catalogInvoke", async (_, command, args) => {
  if (command === "server_catalog" && failCatalogRead) throw new Error("隔离测试：目录暂时不可读");
  const result = await bridge.call(command, args);
  if (command === "server_catalog" && holdCatalogRead) {
    holdCatalogRead = false; catalogReadHeld = true;
    await new Promise(resolve => releaseCatalogRead = resolve);
  }
  // Disk completion may precede the UI callback. Keep this race reproducible.
  if (args?.change?.op === "deleteTag") await new Promise(resolve => setTimeout(resolve, 150));
  return result;
});
await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.catalogInvoke(command, args) }; });
await page.route(url, route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><head><meta charset="UTF-8"></head><body><header class="topbar"><div class="app-tab">SoDeck · 服务器</div></header><main class="workspace" id="test-workspace"></main><div id="test-notice"></div><script type="module">import '/style.css'; import {ServerManager} from '/servers.ts'; window.manager=new ServerManager(document.querySelector('main'),text=>document.querySelector('#test-notice').textContent=text); await window.manager.start(); window.testReady=true;</script></body></html>` }));
const dialogs = () => page.locator("dialog[open]").last();
const action = (name, scope = dialogs()) => scope.locator(`[data-server-action="${name}"]`).first();
const work = () => page.locator("#test-workspace");
async function menuAction(name) { await action("record-menu",work()).click(); await page.locator(".catalog-action-menu:visible").waitFor(); await page.locator(`.catalog-action-menu [data-server-action="${name}"]`).click(); }
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

  await action("edit", work()).click();await field('tagInput').fill('生产环境');await field('tagInput').press('Enter');
  assert.equal((await read()).tags.length,0);await closeDirty();
  assert.equal((await read()).tags.length,0);checks.push('取消服务器表单不留下新标签');
  await action("new-menu",work()).click();
  await page.locator('.catalog-action-menu [data-server-action="new-proxy"]').click();
  await field("host").fill("127.0.0.1"); await field("authType").selectOption("usernamePassword");
  assert.equal(await field("remember").isChecked(), true);
  await field("username").fill("proxy-ops"); await field("password").fill("ui-proxy-secret"); await field("remember").check();
  await field("name").fill("回环代理"); await save(); await expectDialogs(0);
  catalog = await read(); const proxy = catalog.proxies[0].id;
  await menuAction("copy"); await page.waitForFunction(() => document.querySelector(".catalog-detail h2")?.textContent?.includes("副本"));
  catalog = await read(); assert.equal(catalog.proxies[1].authentication.hasSavedSecret, false);
  await menuAction("delete"); await action("confirm-yes").click(); await expectDialogs(0);
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
  await dialogs().locator('[data-server-action="step-up"][data-index="2"]').click();
  await field("password").fill("ui-session-secret"); await field("remember").uncheck(); await save(); await expectDialogs(0);
  catalog = await read(); const target = catalog.servers.find(s => s.name === "生产数据库").id;
  assert.deepEqual(catalog.servers.find(s => s.id === target).route.map(r => r.kind), ["ssh", "socks5", "socks5"]);
  assert.equal(catalog.servers.find(s => s.id === target).authentication.hasSessionSecret, true);
  checks.push("三层混合路径、重复代理、重排、回环来源与临时秘密");

  await action("edit",work()).click();await field('tagInput').fill('数据库，数据库');await save();await expectDialogs(0);
  catalog=await read();const databaseTag=catalog.tags.find(t=>t.name==='数据库').id;
  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();await action('edit',work()).click();await field('tagInput').fill('数据库');await save();await expectDialogs(0);
  await work().locator(`[data-filter-tag="${databaseTag}"]`).click();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),2);
  await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),1,'AND excludes the server with only one selected tag');
  assert.equal(await work().locator('.path-node').count(),3,'Every explicit ancestor remains visible');
  assert.equal(await work().locator('.path-node[data-connect-server-on-double-click]').count(),0);
  await work().locator(`.connection-node[data-id="${gateway}"]`).first().click();
  assert.equal(await work().locator('.catalog-detail [data-connect-server]').count(),0,'A filtered ancestor is context even if its saved target shares this prefix');
  await work().locator('.catalog-detail [data-server-action="locate-target"]').first().click();
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
  await search.fill('公网');
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),1,'Searching a route node retains its matching target');
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
  assert.equal(await work().locator('[data-connect-server]').count(),0);
  assert.equal(await action('new-server',work()).isDisabled(),true);
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
  assert.equal(await ancestor.locator('.tree-toggle svg path').getAttribute('d'),'m9 6 6 6-6 6');
  assert.equal(await ancestor.locator('.tree-toggle svg').evaluate(svg=>getComputedStyle(svg).transform),'matrix(0, 1, -1, 0, 0, 0)','Expanded branch points down');
  await ancestor.locator('[data-server-action="collapse"]').click();
  assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  await ancestor.locator('.tree-toggle svg').evaluate(async svg=>{await Promise.all(svg.getAnimations().map(a=>a.finished.catch(()=>{})));});
  assert.equal(await ancestor.locator('.tree-toggle svg').evaluate(svg=>getComputedStyle(svg).transform),'matrix(1, 0, 0, 1, 0, 0)','Collapsed branch points right');
  await action('reload',work()).click();assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  assert.equal(await work().locator('.catalog-detail h2').innerText(),'生产数据库更新');
  await search.fill('公网');assert.equal(await ancestor.getAttribute('aria-expanded'),'true');
  assert.equal(await ancestor.locator('.tree-toggle').evaluate(button=>getComputedStyle(button).opacity),'1','Filtered branch keeps an opaque guide mask');
  await search.fill('');assert.equal(await ancestor.getAttribute('aria-expanded'),'false');
  await ancestor.locator('[data-server-action="collapse"]').click();
  checks.push('刷新保留折叠和详情，筛选临时展开后恢复原折叠');
  checks.push('多标签 AND、完整混合祖先路径、上下文节点无连接入口及三主题节点展示');

  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();
  await menuAction("delete"); assert((await dialogs().innerText()).includes("生产数据库")); await action("confirm-yes").click();
  await expectDialogs(0); assert.equal((await read()).servers.length, 2);
  await work().locator("#catalog-query").fill("deploy"); assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 1);
  await work().locator("#catalog-query").fill(""); await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
  assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 1); await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).click();
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
  catalog = await read(); await bridge.call("change_catalog", { revision: catalog.revision, change: { op: "saveTag", id: null, name: "外部更新" } });
  await save(); await dialogs().locator(".error-box").waitFor(); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await page.keyboard.press("Escape"); await action("confirm-no").click(); await expectDialogs(1); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await closeDirty(); await action("reload", work()).click();
  checks.push("旧修订拒绝覆盖并保留草稿，Escape 放弃确认");

  await action("edit", work()).click(); await field("name").fill("保存失败后重试");
  fs.mkdirSync(path.join(directory, "servers.json.tmp")); await save(); await dialogs().locator(".error-box").waitFor();
  assert.equal(await field("name").inputValue(), "保存失败后重试");
  fs.rmdirSync(path.join(directory, "servers.json.tmp")); await save(); await expectDialogs(0);
  checks.push("写入失败保留输入并可重试");

  const productionTag=(await read()).tags.find(tag=>tag.name==='生产环境').id;
  assert.equal(await action('tags',work()).count(),0);assert.equal(await action('density',work()).count(),0);
  await bridge.call('change_catalog',{revision:(await read()).revision,change:{op:'deleteTag',id:productionTag}});
  await page.evaluate(()=>manager.start());assert.deepEqual((await read()).servers.find(s=>s.id===target).tagIds,[databaseTag]);
  checks.push('已移除管理标签和密度入口，旧标签数据变更仍正确刷新');
  for (const theme of ["system", "light", "dark", "dopamine"]) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    for (const [width, height] of [[1100, 720], [720, 480]]) {
      await page.setViewportSize({ width, height });
      if(width===720&&await action('back',work()).isVisible())await action('back',work()).click();
      const row=work().locator(`[data-server-action="select"][data-id="${target}"]`);
      await row.focus();await page.keyboard.press('Enter');
      assert.equal(await page.evaluate(()=>document.activeElement===document.body),false,`${theme}/${width} selecting a server keeps contextual focus`);
      if(width===1100){await row.evaluate(async node=>{await Promise.allSettled(node.getAnimations({subtree:true}).map(animation=>animation.finished));});const ratios=await row.evaluate(row=>{const l=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};const b=l(getComputedStyle(row.querySelector('.node-card')).backgroundColor);return [...row.querySelectorAll('.node-endpoint,.node-tags,.node-state')].map(node=>{const a=l(getComputedStyle(node).color);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);});});assert(ratios.length&&ratios.every(r=>r>=4.5),`${theme} selected server contrast: ${ratios}`);}
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
  checks.push("系统、浅色、深色、多巴胺主题与 720×480 表单、焦点和固定页脚");

  await bridge.stop(); bridge = startBridge(); await load(); catalog = await read();
  assert.equal(catalog.servers.find(s => s.id === gateway).authentication.hasSavedSecret, true);
  assert(catalog.servers.every(s => !s.authentication.hasSessionSecret));
  checks.push("真实核心进程重开，持久资料保留、临时秘密清除");

  await bridge.stop(); fs.writeFileSync(path.join(directory, "servers.json"), "{bad-json"); bridge = startBridge(); await load();
  await work().locator(".error-box").waitFor(); assert.equal(await action("new-server", work()).isDisabled(), true);
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
