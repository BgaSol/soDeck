// Actual DOM -> desktop API -> Rust catalog -> isolated local files. No server connections.
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
let bridge = startBridge();
const vite = await createServer({ server: { host: "127.0.0.1", port: 0, strictPort: false } });
await vite.listen();
const url = vite.resolvedUrls.local[0];
const browser = await chromium.launch({ headless: true, ...(process.env.SODECK_BROWSER_CHANNEL ? { channel: process.env.SODECK_BROWSER_CHANNEL } : {}) });
const page = await browser.newPage({ viewport: { width: 1100, height: 720 } });
const errors = [];
page.on("pageerror", error => errors.push(String(error)));
page.on("request", request => { assert.equal(new URL(request.url()).hostname, "127.0.0.1", "Unexpected network request"); });
await page.exposeBinding("catalogInvoke", async (_, command, args) => {
  const result = await bridge.call(command, args);
  // Disk completion may precede the UI callback. Keep this race reproducible.
  if (args?.change?.op === "deleteGroup") await new Promise(resolve => setTimeout(resolve, 150));
  return result;
});
await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.catalogInvoke(command, args) }; });
await page.route(url, route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><head><meta charset="UTF-8"></head><body><header class="topbar"><div class="app-tab">SoDeck · 服务器</div></header><main class="workspace" id="test-workspace"></main><div id="test-notice"></div><script type="module">import '/style.css'; import {ServerManager} from '/servers.ts'; window.manager=new ServerManager(document.querySelector('main'),text=>document.querySelector('#test-notice').textContent=text); await window.manager.start(); window.testReady=true;</script></body></html>` }));
const dialogs = () => page.locator("dialog[open]").last();
const action = (name, scope = dialogs()) => scope.locator(`[data-server-action="${name}"]`);
const work = () => page.locator("#test-workspace");
const field = key => dialogs().locator(`[data-field="${key}"]`);
const read = () => bridge.call("server_catalog");
async function expectDialogs(n) { await page.waitForFunction(n => document.querySelectorAll("dialog[open]").length === n, n); }
async function save() { await dialogs().locator('button[type="submit"]').click(); }
async function closeDirty() { await action("cancel").first().click(); await action("confirm-yes").click(); await expectDialogs(0); }
async function load() { await page.goto(url); await page.waitForFunction(() => window.testReady === true); }
const checks = [];
try {
  await load();
  assert.equal(await work().locator(".catalog-page").evaluate(el => el.getBoundingClientRect().width), 1100);
  await action("new", work()).click(); await field("host").fill("gateway.invalid"); await field("username").fill("ops");
  assert.equal(await field("remember").isChecked(), true);
  await field("password").fill("ui-saved-secret"); await field("name").fill("入口跳板"); await save(); await expectDialogs(0);
  let catalog = await read(); const gateway = catalog.servers[0].id;
  assert.equal(catalog.servers[0].authentication.hasSavedSecret, true); assert(!JSON.stringify(catalog).includes("ui-saved-secret"));
  assert(fs.readFileSync(path.join(directory, "servers.json"), "utf8").includes("ui-saved-secret"));
  await action("edit", work()).click(); assert.equal(await field("password").inputValue(), "");
  assert.equal(await field("remember").isChecked(), true);
  await field("name").fill("公网入口"); await save(); await expectDialogs(0);
  checks.push("创建、编辑、秘密不回填和留空保持");

  await action("edit", work()).click(); await action("new-group").click();
  await dialogs().locator("#group-name").fill("生产环境"); await save(); await expectDialogs(1);
  await closeDirty(); catalog = await read(); assert.equal(catalog.groups.length, 1); assert.equal(catalog.servers[0].groupId, null);
  checks.push("独立保存分组，取消父表单仍保留");

  await work().locator('[data-tab="proxies"]').click(); await action("new", work()).click();
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
  await field("groupId").selectOption(catalog.groups[0].id); await field("routed").selectOption("true");
  await action("add-ssh").click(); await dialogs().locator('[data-step="0"]').selectOption(gateway);
  await action("add-socks").click(); await dialogs().locator('[data-step="1"]').selectOption(proxy);
  await action("add-socks").click(); await dialogs().locator('[data-step="2"]').selectOption(proxy);
  assert((await dialogs().locator(".route-preview").innerText()).includes("公网入口 的回环地址"));
  await dialogs().locator('[data-server-action="step-up"][data-index="2"]').click();
  await field("password").fill("ui-session-secret"); await field("remember").uncheck(); await save(); await expectDialogs(0);
  catalog = await read(); const target = catalog.servers.find(s => s.name === "生产数据库").id;
  assert.deepEqual(catalog.servers.find(s => s.id === target).route.map(r => r.kind), ["ssh", "socks5", "socks5"]);
  assert.equal(catalog.servers.find(s => s.id === target).authentication.hasSessionSecret, true);
  checks.push("三层混合路径、重复代理、重排、回环来源与临时秘密");

  await work().locator(`[data-server-action="select"][data-id="${gateway}"]`).click();
  await action("delete", work()).click(); assert((await dialogs().innerText()).includes("生产数据库")); await action("confirm-yes").click();
  await expectDialogs(0); assert.equal((await read()).servers.length, 2);
  await work().locator("#catalog-query").fill("deploy"); assert.equal(await work().locator(".catalog-row").count(), 1);
  await work().locator("#catalog-query").fill(""); await work().locator("#catalog-group").selectOption(catalog.groups[0].id);
  assert.equal(await work().locator(".catalog-row").count(), 1); await work().locator("#catalog-group").selectOption("all");
  checks.push("引用阻止删除，按账号搜索和分组筛选");

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
  catalog = await read(); await bridge.call("change_catalog", { revision: catalog.revision, change: { op: "saveGroup", id: null, name: "外部更新" } });
  await save(); await dialogs().locator(".error-box").waitFor(); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await page.keyboard.press("Escape"); await action("confirm-no").click(); await expectDialogs(1); assert.equal(await field("name").inputValue(), "不可覆盖的草稿");
  await closeDirty(); await action("reload", work()).click();
  checks.push("旧修订拒绝覆盖并保留草稿，Escape 放弃确认");

  await action("edit", work()).click(); await field("name").fill("保存失败后重试");
  fs.mkdirSync(path.join(directory, "servers.json.tmp")); await save(); await dialogs().locator(".error-box").waitFor();
  assert.equal(await field("name").inputValue(), "保存失败后重试");
  fs.rmdirSync(path.join(directory, "servers.json.tmp")); await save(); await expectDialogs(0);
  checks.push("写入失败保留输入并可重试");

  const productionGroup=(await read()).groups.find(g=>g.name==="生产环境").id;
  await action("groups", work()).click(); await dialogs().locator(`[data-server-action="rename-group"][data-id="${productionGroup}"]`).click();
  await dialogs().locator("#group-name").fill("生产组"); await save(); await expectDialogs(1);
  await dialogs().locator(`[data-server-action="delete-group"][data-id="${productionGroup}"]`).click(); await action("confirm-yes").click(); await expectDialogs(1);
  await dialogs().locator(`[data-server-action="delete-group"][data-id="${productionGroup}"]`).waitFor({ state: "detached" });
  await action("cancel").click(); await expectDialogs(0);
  assert.equal((await read()).servers.find(s => s.id === target).groupId, null);
  checks.push("分组重命名，删除分组保留服务器");

  for (const theme of ["system", "light", "dark", "dopamine"]) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    for (const [width, height] of [[1100, 720], [720, 480]]) {
      await page.setViewportSize({ width, height }); await action("new", work()).click();
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
  console.error({ pageErrors: errors });
  throw error;
} finally {
  await browser.close(); await vite.close(); await bridge.stop();
  // Only this process's mkdtemp output, never a user supplied directory.
  fs.rmSync(directory, { recursive: true, force: true });
}
