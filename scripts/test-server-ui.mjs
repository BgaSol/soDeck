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
page.on("pageerror", error => errors.push(String(error)));
page.on("request", request => { assert.equal(new URL(request.url()).hostname, "127.0.0.1", "Unexpected network request"); });
await page.exposeBinding("catalogInvoke", async (_, command, args) => {
  const result = await bridge.call(command, args);
  // Disk completion may precede the UI callback. Keep this race reproducible.
  if (args?.change?.op === "deleteTag") await new Promise(resolve => setTimeout(resolve, 150));
  return result;
});
await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.catalogInvoke(command, args) }; });
await page.route(url, route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><head><meta charset="UTF-8"></head><body><header class="topbar"><div class="app-tab">SoDeck · 服务器</div></header><main class="workspace" id="test-workspace"></main><div id="test-notice"></div><script type="module">import '/style.css'; import {ServerManager} from '/servers.ts'; window.manager=new ServerManager(document.querySelector('main'),text=>document.querySelector('#test-notice').textContent=text); await window.manager.start(); window.testReady=true;</script></body></html>` }));
const dialogs = () => page.locator("dialog[open]").last();
const action = (name, scope = dialogs()) => scope.locator(`[data-server-action="${name}"]`);
const work = () => page.locator("#test-workspace");
const field = key => dialogs().locator(`[data-field="${key}"]`);
const read = () => bridge.call("server_catalog");
async function expectDialogs(n) { try {await page.waitForFunction(n => document.querySelectorAll("dialog[open]").length === n, n);} catch(error){console.log(await page.locator("dialog[open]").allTextContents());throw error;} }
async function save() { await dialogs().locator('button[type="submit"]').click(); }
async function closeDirty() { await action("cancel").first().click(); await action("confirm-yes").click(); await expectDialogs(0); }
async function load() { await page.goto(url); await page.waitForFunction(() => window.testReady === true); }
const checks = [];
try {
  await load();
  const search=work().locator('#catalog-query');await search.focus();
  await search.evaluate(input=>{window.searchInput=input;window.searchDisconnects=0;const ancestors=new Set();for(let n=input;n&&n!==document.body;n=n.parentNode)ancestors.add(n);new MutationObserver(records=>{for(const r of records)for(const n of r.removedNodes)if(ancestors.has(n))searchDisconnects++;}).observe(document.body,{childList:true,subtree:true});});
  const ime=await page.context().newCDPSession(page);
  await ime.send('Input.imeSetComposition',{text:'zhong',selectionStart:5,selectionEnd:5});await page.evaluate(()=>manager.start());
  await ime.send('Input.imeSetComposition',{text:'zhongwen',selectionStart:8,selectionEnd:8});
  assert.equal(await search.inputValue(),'zhongwen');assert.equal(await search.evaluate(input=>input===searchInput&&searchDisconnects===0),true);
  await ime.send('Input.insertText',{text:'中文'});assert.equal(await search.inputValue(),'中文');await ime.detach();await search.fill('');
  assert.equal(await work().locator(".catalog-page").evaluate(el => el.getBoundingClientRect().width), 1100);
  await action("new", work()).click(); await field("host").fill("gateway.invalid"); await field("username").fill("ops");
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

  await action("edit", work()).click(); await action("new-tag").click();
  await dialogs().locator("#tag-name").fill("生产环境"); await save(); await expectDialogs(1);
  await closeDirty(); catalog = await read(); assert.equal(catalog.tags.length, 1); assert.deepEqual(catalog.servers[0].tagIds, []);
  checks.push("独立保存标签，取消父表单仍保留");

  const proxyTab=work().locator('[data-tab="proxies"]');await proxyTab.focus();await page.keyboard.press('Enter');
  assert.equal(await proxyTab.evaluate(el=>el===document.activeElement),true,'Switching catalog tabs retains keyboard focus');
  await action("new", work()).click();
  await field("host").fill("127.0.0.1"); await field("authType").selectOption("usernamePassword");
  assert.equal(await field("remember").isChecked(), true);
  await field("username").fill("proxy-ops"); await field("password").fill("ui-proxy-secret"); await field("remember").check();
  await field("name").fill("回环代理"); await save(); await expectDialogs(0);
  catalog = await read(); const proxy = catalog.proxies[0].id;
  await action("copy", work()).click(); await page.waitForFunction(() => document.querySelector(".catalog-detail h2")?.textContent?.includes("副本"));
  catalog = await read(); assert.equal(catalog.proxies[1].authentication.hasSavedSecret, false);
  await action("delete", work()).click(); await action("confirm-yes").click(); await expectDialogs(0);
  assert.equal((await read()).proxies.length, 1); checks.push("代理增删改查和复制不带秘密");

  await work().locator('[data-tab="servers"]').click(); await action("new", work()).click();
  await field("host").fill("db.internal"); await field("username").fill("deploy"); await field("name").fill("生产数据库");
  await dialogs().locator(`[data-tag="${catalog.tags[0].id}"]`).check(); await field("routed").selectOption("true");
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

  await action("edit", work()).click();await action("new-tag").click();
  await dialogs().locator("#tag-name").fill("数据库");await save();await expectDialogs(1);
  const databaseTag=(await read()).tags.find(t=>t.name==='数据库').id;
  await dialogs().locator(`[data-tag="${databaseTag}"]`).check();await save();await expectDialogs(0);
  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();await action("edit",work()).click();
  await dialogs().locator(`[data-tag="${databaseTag}"]`).check();await save();await expectDialogs(0);
  await work().locator('.tag-filter summary').click();
  await work().locator(`[data-filter-tag="${databaseTag}"]`).check();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),2);
  await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).check();
  assert.equal(await work().locator('.connection-node:not(.path-node)').count(),1,'AND excludes the server with only one selected tag');
  assert.equal(await work().locator('.path-node').count(),3,'Every explicit ancestor remains visible');
  assert.equal(await work().locator('.path-node[data-connect-server-on-double-click]').count(),0);
  await work().locator(`[data-server-action="select"][data-id="${target}"]`).click();
  fs.mkdirSync('artifacts/connection-tree',{recursive:true});
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await page.evaluate(async()=>{await new Promise(requestAnimationFrame);await Promise.allSettled(document.getAnimations().map(animation=>animation.finished));});
    await page.screenshot({path:`artifacts/connection-tree/${theme}.png`});
  }
  await work().locator(`[data-filter-tag="${databaseTag}"]`).uncheck();await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).uncheck();
  await work().locator('.tag-filter summary').click();
  checks.push('多标签 AND、完整混合祖先路径、上下文节点无连接入口及三主题节点展示');

  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).first().click();
  await action("delete", work()).click(); assert((await dialogs().innerText()).includes("生产数据库")); await action("confirm-yes").click();
  await expectDialogs(0); assert.equal((await read()).servers.length, 2);
  await work().locator("#catalog-query").fill("deploy"); assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 1);
  await work().locator("#catalog-query").fill(""); await work().locator(".tag-filter summary").click();await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).check();
  assert.equal(await work().locator(".connection-node:not(.path-node)").count(), 1); await work().locator(`[data-filter-tag="${catalog.tags[0].id}"]`).uncheck();await work().locator(".tag-filter summary").click();
  checks.push("引用阻止删除，按账号搜索和标签筛选");

  await action("new", work()).click(); await field("host").fill("unsaved.internal"); await action("inline-proxy").count().then(async n => {
    // Inline creation is inside the routed section.
    if (!n) await field("routed").selectOption("true");
  });
  await action("inline-proxy").click();
  const nestedLabels=await dialogs().locator('[data-field]').evaluateAll(nodes=>nodes.filter(node=>node.type!=='checkbox').map(node=>({field:node.dataset.field,labels:[...node.labels].map(label=>({text:label.textContent,owns:label.control===node&&label.closest('dialog')===node.closest('dialog')}))})));
  assert(nestedLabels.every(item=>item.labels.length===1&&item.labels[0].owns),`Nested proxy fields own their labels: ${JSON.stringify(nestedLabels)}`);
  assert.equal(await page.locator('[id]').evaluateAll(nodes=>new Set(nodes.map(node=>node.id)).size===nodes.length),true,'Nested editors have unique DOM IDs');
  await field("authType").selectOption("usernamePassword");
  await field("username").fill("proxy-ops");
  await field("host").fill("proxy.internal"); await field("name").fill("单独保留代理");
  await save(); await expectDialogs(1); assert.equal(await dialogs().locator("[data-step]").count(), 1);
  await closeDirty(); assert.equal((await read()).proxies.length, 2);
  checks.push("内联代理保存后取消服务器仍保留");

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

  const productionTag=(await read()).tags.find(g=>g.name==="生产环境").id;
  await work().locator(".tag-filter summary").click();await action("tags", work()).click(); await dialogs().locator(`[data-server-action="rename-tag"][data-id="${productionTag}"]`).click();
  await dialogs().locator("#tag-name").fill("生产组"); await save(); await expectDialogs(1);
  await dialogs().locator(`[data-server-action="delete-tag"][data-id="${productionTag}"]`).click(); await action("confirm-yes").click(); await expectDialogs(1);
  await dialogs().locator(`[data-server-action="delete-tag"][data-id="${productionTag}"]`).waitFor({ state: "detached" });
  await action("cancel").click(); await expectDialogs(0);
  assert.deepEqual((await read()).servers.find(s => s.id === target).tagIds, [databaseTag]);
  checks.push("标签重命名，删除标签保留服务器");

  for (const theme of ["system", "light", "dark", "dopamine"]) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    for (const [width, height] of [[1100, 720], [720, 480]]) {
      await page.setViewportSize({ width, height });
      if(width===720&&await action('back',work()).isVisible())await action('back',work()).click();
      const row=work().locator(`[data-server-action="select"][data-id="${target}"]`);
      await row.focus();await page.keyboard.press('Enter');
      assert.equal(await page.evaluate(()=>document.activeElement===document.body),false,`${theme}/${width} selecting a server keeps contextual focus`);
      if(width===1100){await row.evaluate(async node=>{await Promise.allSettled(node.getAnimations({subtree:true}).map(animation=>animation.finished));});const ratios=await row.evaluate(row=>{const l=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};const b=l(getComputedStyle(row).backgroundColor);return [...row.querySelectorAll('.node-endpoint,.node-tags,.node-state')].map(node=>{const a=l(getComputedStyle(node).color);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);});});assert(ratios.length&&ratios.every(r=>r>=4.5),`${theme} selected server contrast: ${ratios}`);}
      if(width===720){await action('back',work()).focus();await page.keyboard.press('Enter');assert.equal(await row.evaluate(el=>el===document.activeElement),true,'Returning to the list focuses the original server');}
      await action("new", work()).click();
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
  await work().locator(".error-box").waitFor(); assert.equal(await action("new", work()).isDisabled(), true);
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
