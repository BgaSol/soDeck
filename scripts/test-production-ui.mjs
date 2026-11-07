// Use the actual bundled HTML/JS/CSS and an isolated catalog core; only native window/event APIs are replaced.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import http from "node:http";
import { createHash } from "node:crypto";
import { preview } from "vite";
import { chromium } from "playwright";
import { assertMenuLayout } from "./menu-layout.mjs";

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--bin", "plugin-fixture", "--bin", "workspace-ui-fixture", "--locked"], { stdio: "inherit" });
if (build.status !== 0) throw new Error("Catalog test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-production-ui-"));
const suffix = process.platform === "win32" ? ".exe" : "";
const target = spawnSync("rustc", ["-vV"], { encoding: "utf8" }).stdout.match(/^host: (.+)$/m)[1];
// Run the title-bar driver injected by the locked Tauri crate, rather than copying it.
const metadata = spawnSync("cargo", ["metadata", "--manifest-path", "src-tauri/Cargo.toml", "--format-version", "1", "--locked", "--filter-platform", target], { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
assert.equal(metadata.status, 0, metadata.error?.message || metadata.stderr);
const tauri = JSON.parse(metadata.stdout).packages.find(item => item.name === "tauri");
const nativeDragScript = fs.readFileSync(path.join(path.dirname(tauri.manifest_path), "src/window/scripts/drag.js"), "utf8")
  .replace("__TEMPLATE_os_name__", JSON.stringify(process.platform === "darwin" ? "macos" : process.platform === "win32" ? "windows" : "linux"));
const fixture = path.join(directory, "fixture"); fs.mkdirSync(fixture);
const uiFixture=path.join(directory,'workspace-ui');fs.mkdirSync(uiFixture);
fs.copyFileSync(`src-tauri/target/debug/workspace-ui-fixture${suffix}`,path.join(uiFixture,`workspace-ui-fixture${suffix}`));
if(process.platform!=='win32')fs.chmodSync(path.join(uiFixture,`workspace-ui-fixture${suffix}`),0o755);
fs.cpSync('scripts/fixtures/workspace-ui/ui',path.join(uiFixture,'ui'),{recursive:true});
const uiMetadata=JSON.parse(fs.readFileSync('scripts/fixtures/workspace-ui/manifest.json','utf8'));
uiMetadata.settings={apiVersion:'1',schemaVersion:1,pages:[{id:'display',title:'显示偏好验收',viewTypes:['files'],groups:[{id:'text',title:'文字',fields:[{key:'fontSize',type:'number',title:'名称字号',default:12,min:11,max:18,step:1,unit:'px',apply:'live'}]}]}]};
for(let index=0;index<20;index++)uiMetadata.ui.views.push({id:`extra${index}`,title:`扩展测试组件 ${index}`,icon:'plug',container:'sidebar',instancePolicy:'perWorkspace',defaultSide:'left'});
fs.writeFileSync(path.join(uiFixture,'plugin.json'),JSON.stringify({...uiMetadata,target,entry:`workspace-ui-fixture${suffix}`}));
fs.copyFileSync(`src-tauri/target/debug/plugin-fixture${suffix}`, path.join(fixture, `plugin-fixture${suffix}`));
if (process.platform !== "win32") fs.chmodSync(path.join(fixture, `plugin-fixture${suffix}`), 0o755);
fs.writeFileSync(path.join(fixture, "plugin.json"), JSON.stringify({ id: "sodeck.terminal", name: "Loopback terminal fixture", version: "0.1.1", apiVersion: "1", target, entry: `plugin-fixture${suffix}` }));
const archive = path.join(directory, "fixture.tar.gz");
const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32", "tar.exe") : "tar";
if (spawnSync(tar, ["-czf", archive, "-C", directory, "fixture"], { windowsHide: true, stdio: "inherit" }).status !== 0) throw new Error("Failed to package test fixture");
const archiveBytes = fs.readFileSync(archive), requests = [];
let published = false, corrupt = false;
let extraCorrupt = true;
const extraPackages = new Map(), extraManifests = new Map(), extraReleases = [];
const markdownNotes = '# 更新测试\n\n## 改进\n\n- **保留工作**并支持*重试*\n  - 使用 `npm run check` 验证\n- 下载后继续使用\n\n> 完成工作后再重启。\n\n[发行说明](https://github.com/BgaSol/soDeck/releases)\n\n```sh\nnpm run check\n' + 'a'.repeat(180) + '\n```\n\n| 项目 | 结果 |\n| --- | --- |\n| 核心 | 可更新 |\n| 插件 | 独立下载 |';
const downloads = http.createServer((request, response) => {
  requests.push(request.url);
  const base = `http://127.0.0.1:${downloads.address().port}`;
  let data;
  if (request.url.startsWith("/repos/BgaSol/soDeck/releases")) data = JSON.stringify(published ? [{ tag_name: "plugin-terminal-v0.1.1", draft: false, prerelease: false, assets: [{ name: "plugin-update.json", browser_download_url: `${base}/manifest.json` }] }, ...extraReleases] : []);
  else if (extraManifests.has(request.url)) data = JSON.stringify(extraManifests.get(request.url));
  else if (extraPackages.has(request.url)) data = extraCorrupt && request.url.includes('update-two') ? Buffer.from('broken archive') : extraPackages.get(request.url);
  else if (request.url === "/manifest.json") data = JSON.stringify({ id: "sodeck.terminal", version: "0.1.1", apiVersion: "1", platforms: { [target]: { url: `${base}/fixture.tar.gz`, sha256: createHash("sha256").update(archiveBytes).digest("hex") } } });
  else if (request.url === "/fixture.tar.gz") data = corrupt ? Buffer.from("interrupted download") : archiveBytes;
  else { response.writeHead(404).end(); return; }
  response.writeHead(200, { "Content-Length": Buffer.byteLength(data) }); response.end(data);
});
await new Promise(resolve => downloads.listen(0, "127.0.0.1", resolve));
const child = spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${suffix}`), [directory], { windowsHide: true, stdio: ["pipe", "pipe", "inherit"], env: { ...process.env, SODECK_TEST_PLUGIN_API: `http://127.0.0.1:${downloads.address().port}` } });
const pending = [];
readline.createInterface({ input: child.stdout }).on("line", line => {
  const item = JSON.parse(line), request = pending.shift();
  if (item.error) request.reject(new Error(item.error)); else request.resolve(item.value);
});
child.on("exit", () => { for (const request of pending.splice(0)) request.reject(new Error("Test core exited")); });
const call = (command, args = {}) => new Promise((resolve, reject) => {
  pending.push({ resolve, reject }); child.stdin.write(JSON.stringify({ command, args }) + "\n");
});
const server = await preview({ preview: { host: "127.0.0.1", port: 0 } });
const browser = await chromium.launch({ headless: true, ...(process.env.SODECK_BROWSER_CHANNEL ? { channel: process.env.SODECK_BROWSER_CHANNEL } : {}) });
const page = await browser.newPage({ viewport: { width: 1100, height: 720 } });
await page.route('**/__plugin__/**',route=>{
  const relative=decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/');
  const file=path.join(uiFixture,relative),types={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
  return route.fulfill({contentType:types[path.extname(file)]||'application/octet-stream',body:fs.readFileSync(file)});
});
const errors = [], checks = [];
let failCatalog = false, testWindow = "main", heldLogReply, releaseLogReply;
page.on("pageerror", error => errors.push(String(error)));
await page.exposeBinding("desktopInvoke", async (_, command, args) => {
  if (command === 'server_catalog' && failCatalog) throw new Error('隔离测试：目录暂时不可读');
  const result=await call(command, { ...args, window: testWindow });
  if(command==='read_logs'&&args.filter?.query==='held-old-filter'){heldLogReply=true;await new Promise(resolve=>releaseLogReply=resolve);}
  return result;
});
await page.addInitScript(() => {
  let id = 0;
  const callbacks = new Map(), listeners = new Map();
  window.nativeWindowCalls = [];
  window.nativeMaximized = false;
  window.emitTestEvent = (event, payload = null) => { for (const [eventId, item] of listeners) if (item.event === event) callbacks.get(item.handler)?.({event, id:eventId, payload}); };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: sessionStorage.getItem("test-window") || "main" } },
    convertFileSrc: path => new URL('/__plugin__/'+path,location.href).href,
    transformCallback: callback => { const token = ++id; callbacks.set(token, callback); return token; },
    invoke: (command, args) => {
      if (command === 'open_update_link') { (window.openedUpdateLinks ??= []).push(args.url); return Promise.resolve(); }
      if (command === 'open_usage_help') { window.openedUsageHelp = true; return Promise.resolve(); }
      if (command === "plugin:event|listen") { const eventId = ++id; listeners.set(eventId, args); return Promise.resolve(eventId); }
      if (command === "plugin:event|unlisten" || command === "workspace_surface") return Promise.resolve();
      if (command.startsWith("plugin:window|")) {
        const action = command.split("|")[1];
        if (action === "is_maximized") return Promise.resolve(window.nativeMaximized);
        window.nativeWindowCalls.push(action);
        if (action === "toggle_maximize" || action === "internal_toggle_maximize") window.nativeMaximized = !window.nativeMaximized;
        return Promise.resolve();
      }
      return window.desktopInvoke(command, args);
    },
  };
});
await page.addInitScript({ content: nativeDragScript });
async function checkTitlebar() {
  const result = await page.evaluate(() => {
    // Native dragging can consume click/dblclick. The mouse-down sequence must suffice.
    const press = (node, detail, button = 0) => {
      for (const type of ["mousedown", "mouseup"]) node.dispatchEvent(new MouseEvent(type, { bubbles: true, composed: true, button, detail, clientX: 20, clientY: 20 }));
    };
    const check = selector => {
      const node = document.querySelector(selector);
      if (!node) throw new Error(`Missing title-bar target: ${selector}`);
      window.nativeWindowCalls.length = 0;
      press(node, 1); press(node, 2);
      const maximized = window.nativeMaximized;
      press(node, 1); press(node, 2);
      return { selector, maximized, restored: !window.nativeMaximized, calls: [...window.nativeWindowCalls] };
    };
    const blank = ["#drag-region", ".topbar", "#server-tabs", ".global-tools"].map(check);
    const interactive = ["#catalog-tab span", "#new-workspace svg", "#status-button .health-dot", "#messages-button svg", "#settings-button svg", "#window-controls button svg", "#server-workspace"];
    if (document.querySelector(".server-tab")) interactive.push(".server-tab", ".server-tab .tab-name", ".tab-menu-button svg", ".tab-close-button svg");
    const excluded = interactive.map(check);
    window.nativeWindowCalls.length = 0;
    press(document.querySelector("#drag-region"), 1, 2);
    press(document.querySelector("#drag-region"), 2, 2);
    return { blank, excluded, rightClick: [...window.nativeWindowCalls] };
  });
  for (const item of result.blank) {
    assert.equal(item.maximized, true, `${testWindow} ${item.selector} must maximize without a DOM dblclick`);
    assert.equal(item.restored, true, `${testWindow} ${item.selector} must restore`);
    assert.deepEqual(item.calls, ["start_dragging", "internal_toggle_maximize", "start_dragging", "internal_toggle_maximize"], `${testWindow} ${item.selector}: exactly one toggle per double click`);
  }
  for (const item of result.excluded) assert.deepEqual(item.calls, [], `${testWindow} ${item.selector} must keep its own interaction`);
  assert.deepEqual(result.rightClick, []);
  await page.locator("#drag-region").dblclick();
  assert.equal(await page.evaluate(() => window.nativeMaximized), true, "A real DOM double click must toggle only once");
  await page.locator("#drag-region").dblclick();
  assert.equal(await page.evaluate(() => window.nativeMaximized), false);
}
const dialog = () => page.locator("dialog[open]").last();
const field = name => dialog().locator(`[data-field="${name}"]`);
const close = async () => {
  await dialog().locator('[data-server-action="cancel"]').first().click();
  await page.waitForFunction(() => !document.querySelector("dialog[open]") || document.querySelector("dialog[open] [data-server-action='confirm-yes']"));
  if (await dialog().locator('[data-server-action="confirm-yes"]').count()) await dialog().locator('[data-server-action="confirm-yes"]').click();
  await page.locator("dialog[open]").waitFor({ state: "hidden" });
};
try {
  await call("set_theme", { theme: "dopamine" });
  await page.goto(server.resolvedUrls.local[0]);
  await page.waitForFunction(() => document.documentElement.dataset.theme === "dopamine");
  await checkTitlebar();
  assert.equal(await page.locator('.catalog-empty').count(),1,'First use has one empty state');
  assert.equal(await page.locator('.catalog-detail').count(),0);
  checks.push("Main title bar: blank areas maximize/restore without DOM dblclick; buttons, nested icons, content and right clicks stay independent");
  await page.locator('[data-server-action="new"]').click();
  const before = await page.evaluate(() => {
    const root = getComputedStyle(document.documentElement), field = getComputedStyle(document.querySelector(".catalog-field"));
    const body = document.querySelector(".catalog-form-body");
    return { canvas: root.getPropertyValue("--canvas").trim(), panel: root.getPropertyValue("--panel").trim(), alignItems: field.alignItems, overflow: getComputedStyle(body).overflowY, clientHeight: body.clientHeight, scrollHeight: body.scrollHeight };
  });
  fs.mkdirSync("artifacts/production-ui", { recursive: true });
  await page.screenshot({ path: "artifacts/production-ui/dopamine-form.png" });
  console.log(JSON.stringify({ bundledForm: before }));
  assert.equal(before.canvas, "#fbf1e8", "Bundled CSS must retain the saved dopamine palette");
  assert.equal(before.alignItems, "stretch", "Form labels must use component layout");
  await close();

  const plus=page.locator('#new-workspace'),picker=page.getByRole('dialog',{name:'选择服务器',exact:true});
  await plus.click();await picker.getByRole('status').filter({hasText:'暂无服务器'}).waitFor();
  await plus.click();await picker.waitFor({state:'hidden',timeout:2000});
  await plus.click();await picker.getByRole('status').filter({hasText:'暂无服务器'}).waitFor();
  await page.keyboard.press('Escape');assert.equal(await plus.evaluate(el=>el===document.activeElement),true);
  let saved=await call('change_catalog',{revision:(await call('server_catalog')).revision,change:{op:'saveGroup',id:null,name:'生产组'}});
  const groupId=saved.catalog.groups[0].id;
  const serverIds=[];
  for (const [name,host,username] of [['生产入口','192.0.2.10','root'],['测试环境','192.0.2.20','ops']]) {
    saved=await call('change_catalog',{revision:(await call('server_catalog')).revision,change:{op:'saveServer',draft:{name,host,port:22,username,groupId,authentication:{type:'password',secret:{action:'clear'}},route:[]}}});serverIds.push(saved.selectedId);
  }
  await plus.click();
  const search=picker.getByRole('searchbox',{name:'搜索服务器'});
  for(const query of ['生产入口','192.0.2.10','root','生产组']) { await search.fill(query);await picker.getByRole('button',{name:/生产入口/}).waitFor(); }
  await search.fill('没有这个服务器');await picker.getByRole('status').filter({hasText:'没有匹配'}).waitFor();
  await page.keyboard.press('Enter');assert.equal(await picker.isVisible(),true);
  await search.fill('');await page.keyboard.press('ArrowUp');
  assert.equal(await page.evaluate(()=>document.activeElement.dataset.serverId),await picker.locator('[data-server-id]').last().getAttribute('data-server-id'));
  await search.focus();await page.keyboard.press('ArrowDown');assert.match(await page.evaluate(()=>document.activeElement.textContent),/生产入口|测试环境/);
  await page.keyboard.press('Escape');
  failCatalog=true;await plus.click();await picker.getByRole('alert').filter({hasText:'目录暂时不可读'}).waitFor();
  failCatalog=false;await picker.getByRole('button',{name:'重新读取',exact:true}).click();await picker.getByRole('button',{name:/生产入口/}).waitFor();
  // A catalog event refreshes only the list and preserves the user's search.
  await search.fill('生产');
  const record=(await call('server_catalog')).servers.find(s=>s.id===serverIds[0]);
  await call('change_catalog',{revision:(await call('server_catalog')).revision,change:{op:'saveServer',draft:{...record,name:'生产入口-已更新',authentication:{type:'password',secret:{action:'keep',remember:false}}}}});
  await picker.getByRole('button',{name:/生产入口/}).focus();
  await page.evaluate(()=>window.emitTestEvent('server-catalog-changed'));
  await picker.getByRole('button',{name:/生产入口-已更新/}).waitFor();assert.equal(await search.inputValue(),'生产');
  assert.equal(await page.evaluate(()=>document.activeElement.dataset.serverId),serverIds[0]);
  await page.locator('#catalog-tab').click();await picker.waitFor({state:'hidden'});
  checks.push('一级加号：重复点击开关、空目录、搜索名称/地址/账号/分组、无结果 Enter 不误操作、读取失败重试、实时更新保留搜索与焦点、Esc 和外部关闭');

  const palettes = { light: "#f6f7f9", dark: "#191b22", dopamine: "#fbf1e8" };
  for (const [theme, canvas] of Object.entries(palettes)) {
    await call("set_theme", { theme }); await page.reload();
    await page.waitForFunction(theme => document.documentElement.dataset.theme === theme, theme);
    assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--canvas").trim()), canvas);
    for (const [width, height] of [[1100, 720], [720, 480]]) {
      await page.setViewportSize({ width, height });
      await plus.click();await picker.getByRole('button',{name:/生产入口-已更新/}).waitFor();
      assert.equal(await plus.evaluate(el=>getComputedStyle(el).borderTopWidth),'0px');
      assert.equal(await picker.evaluate(el=>getComputedStyle(el).overflow),'visible');
      assert.equal(await picker.evaluate(el=>getComputedStyle(el,'::before').borderTopWidth),theme==='dopamine'?'2px':'1px');
      assert(await picker.evaluate(el=>{const r=el.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&r.bottom<=innerHeight;}));
      if(width===1100){fs.mkdirSync('artifacts/tab-creation',{recursive:true});await page.screenshot({path:`artifacts/tab-creation/server-picker-${theme}.png`});}
      await page.keyboard.press('Escape');
      await page.locator('[data-tab="servers"]').click();await page.locator('.catalog-row').first().click();
      const connectButton=page.locator('.connection-card [data-connect-server]');await connectButton.waitFor();
      assert(await connectButton.evaluate(el=>{const r=el.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight;}),`${theme}/${width} connection action is visible`);
      assert(await page.locator('.connection-card').evaluate(el=>Boolean(el.compareDocumentPosition(el.parentElement.querySelector('.endpoint-fields'))&Node.DOCUMENT_POSITION_FOLLOWING)));
      await page.screenshot({path:`artifacts/production-ui/server-detail-${theme}-${width}.png`});
      for (const kind of ["servers", "proxies"]) {
        await page.locator(`[data-tab="${kind}"]`).click();
        await page.locator('[data-server-action="new"]').click();
        if (kind === "servers") {
          await field("authType").selectOption("privateKey");
          await field("routed").selectOption("true");
          for (let i = 0; i < 5; i++) await dialog().locator('[data-server-action="add-socks"]').click();
        } else await field("authType").selectOption("usernamePassword");
        const body = dialog().locator(".catalog-form-body"), footer = dialog().locator(".catalog-dialog-footer"), heading = dialog().locator(".catalog-dialog-heading");
        const top = await heading.boundingBox(), bottom = await footer.boundingBox();
        assert(top.y >= 0 && bottom.y + bottom.height <= height, `Heading and footer stay inside the window: ${JSON.stringify({ theme, kind, width, height, top, bottom })}`);
        assert.equal(await body.evaluate(el => getComputedStyle(el).overflowY), "auto");
        const canScroll = await body.evaluate(el => el.scrollHeight > el.clientHeight);
        await body.evaluate(el => el.scrollTo(0, 0));
        const bounds = await body.boundingBox();
        await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + Math.min(60, bounds.height / 2));
        if (canScroll) {
          await page.mouse.wheel(0, 500);
          await page.waitForFunction(() => document.querySelector(".catalog-form-body").scrollTop > 0);
          await page.mouse.wheel(0, 10000);
        }
        await page.waitForFunction(() => { const el = document.querySelector(".catalog-form-body"); return el.scrollTop + el.clientHeight >= el.scrollHeight - 1; });
        await field("name").fill(`${theme}-${kind}`);
        const name = await field("name").boundingBox();
        assert(name.y + name.height <= bottom.y + 1, "The last fields can scroll above the fixed footer");
        await page.keyboard.press("Tab");
        assert(await page.evaluate(() => !!document.activeElement.closest("dialog")));
        assert(await dialog().evaluate(el => el.scrollWidth <= el.clientWidth + 1));
        assert(Math.abs((await heading.boundingBox()).y - top.y) < 1);
        assert(Math.abs((await footer.boundingBox()).y - bottom.y) < 1);
        if (theme === "dopamine" && kind === "servers" && width === 720) await page.screenshot({ path: "artifacts/production-ui/dopamine-small-scrolled.png" });
        await close();
      }
    }
    checks.push(`${theme}: bundled palette, server/proxy forms, wheel scrolling, last fields, fixed footer and keyboard at 1100×720 and 720×480`);
  }

  await page.setViewportSize({ width: 1100, height: 720 });
  await page.locator("#settings-button").click();
  const appearanceNav = page.locator('[data-action="section"][data-id="appearance"]');
  await appearanceNav.focus(); await page.keyboard.press('Enter');
  assert.equal(await appearanceNav.evaluate(el => el === document.activeElement), true, 'Changed settings pages retain navigation focus');
  await page.locator('[data-action="theme"][data-id="light"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
  assert.equal(await page.locator('[data-action="theme"][data-id="light"]').evaluate(el => el === document.activeElement), true, 'Theme changes retain focus');
  const contrasts = await page.locator('.nav-item.active,.theme-option.selected').evaluateAll(nodes => {
    const luminance = color => { const c = color.match(/[\d.]+/g).slice(0,3).map(Number).map(v => v / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4); return c[0] * .2126 + c[1] * .7152 + c[2] * .0722; };
    return nodes.map(node => { const s = getComputedStyle(node), a = luminance(s.color), b = luminance(s.backgroundColor); return (Math.max(a,b) + .05) / (Math.min(a,b) + .05); });
  });
  assert(contrasts.every(ratio => ratio >= 4.5), `Selected small text contrast: ${contrasts}`);
  await page.locator('[data-action="section"][data-id="plugins"]').click();
  await page.locator('[data-action="section"][data-id="about"]').click();
  await page.getByRole('button',{name:'使用帮助',exact:true}).focus();await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(()=>window.openedUsageHelp),true);
  const closeLayer=page.locator('[data-action="close-layer"]');
  await page.getByRole('button',{name:'复制日志路径',exact:true}).focus();await page.keyboard.press('Tab');
  assert.equal(await closeLayer.evaluate(el=>el===document.activeElement),true,'Last Tab wraps to close');
  await page.keyboard.press('Shift+Tab');assert.equal(await page.getByRole('button',{name:'复制日志路径',exact:true}).evaluate(el=>el===document.activeElement),true);
  await page.keyboard.press('Escape');assert.equal(await page.locator('#settings-button').evaluate(el=>el===document.activeElement),true);
  await page.locator('#settings-button').click();await page.locator('[data-action="section"][data-id="plugins"]').click();
  await page.locator('[data-action="add-plugin"]').focus();await page.keyboard.press('Enter');
  assert.equal(await page.locator('[data-action="back-plugins"]').evaluate(el=>el===document.activeElement),true,'Adding a plugin moves keyboard focus into the new page');
  const terminal = page.locator('[data-official-plugin="sodeck.terminal"]');
  const install = () => terminal.locator('[data-action="install-official-plugin"]');
  assert.deepEqual(await page.locator('[data-official-plugin]').evaluateAll(nodes => nodes.map(node => node.dataset.officialPlugin).sort()), ['sodeck.files','sodeck.network-proxy','sodeck.terminal']);
  assert.match(await page.locator('[data-official-plugin="sodeck.terminal"]').textContent(), /真实终端/);
  assert.match(await page.locator('[data-official-plugin="sodeck.files"]').textContent(), /上传下载、管理文件/);
  assert.deepEqual(requests, [], "Opening settings must not download plugins automatically");
  await install().click(); await terminal.getByRole("alert").waitFor();
  assert.match(await terminal.getByRole("alert").textContent(), /尚未发布/);
  assert.equal((await call("snapshot")).plugins.length, 0);
  published = true; corrupt = true;
  await install().click(); await page.waitForFunction(() => document.querySelector('[data-official-plugin="sodeck.terminal"] [role="alert"]')?.textContent.includes("SHA-256"));
  assert.equal((await call("snapshot")).plugins.length, 0);
  corrupt = false; await install().click();
  await page.locator('.maintenance-pending').waitFor();
  assert.match(await page.locator('.maintenance-pending').textContent(), /等待重启/);
  assert.equal(await page.locator('[data-official-plugin]').count(), 0, 'Installed plugins leave the add screen');
  const installed = (await call("snapshot")).plugins[0];
  assert(installed.enabled && installed.pendingRestart);
  assert.equal(installed.status, "unloaded");
  assert.equal(installed.manifest.updateSource.repository, "BgaSol/soDeck");
  await page.reload(); await page.locator("#settings-button").click();
  await page.locator('[data-plugin-id="sodeck.terminal"]').waitFor();
  assert.equal(await page.locator('[data-plugin-id="sodeck.terminal"]').count(),1);
  assert.equal((await call("snapshot")).plugins.length, 1);
  await page.screenshot({ path: "artifacts/production-ui/official-plugins.png", animations: "disabled" });
  checks.push("Official plugin first install: no background downloads, unpublished release and corrupt archive retries, native package selection, pending restart and persisted record after reload");
  const base=`http://127.0.0.1:${downloads.address().port}`;
  for (const name of ['update-one','update-two']) {
    const pluginDirectory=path.join(directory,name);fs.mkdirSync(pluginDirectory);
    fs.copyFileSync(`src-tauri/target/debug/plugin-fixture${suffix}`,path.join(pluginDirectory,`plugin-fixture${suffix}`));
    if(process.platform!=='win32')fs.chmodSync(path.join(pluginDirectory,`plugin-fixture${suffix}`),0o755);
    const metadata={id:`test.${name}`,name:`更新测试 ${name}`,version:'0.1.1',apiVersion:'1',target,entry:`plugin-fixture${suffix}`,updateSource:{repository:'BgaSol/soDeck',tagPrefix:`plugin-${name}-v`}};
    fs.writeFileSync(path.join(pluginDirectory,'plugin.json'),JSON.stringify(metadata));
    const packed=path.join(directory,`${name}.tar.gz`);
    assert.equal(spawnSync(tar,['-czf',packed,'-C',directory,name],{windowsHide:true}).status,0);
    const bytes=fs.readFileSync(packed);extraPackages.set(`/${name}.tar.gz`,bytes);
    extraManifests.set(`/${name}.json`,{id:metadata.id,version:'0.1.1',apiVersion:'1',notes:markdownNotes,platforms:{[target]:{url:`${base}/${name}.tar.gz`,sha256:createHash('sha256').update(bytes).digest('hex')}}});
    extraReleases.push({tag_name:`plugin-${name}-v0.1.1`,draft:false,prerelease:false,assets:[{name:'plugin-update.json',browser_download_url:`${base}/${name}.json`}]});
    fs.writeFileSync(path.join(pluginDirectory,'plugin.json'),JSON.stringify({...metadata,version:'0.1.0'}));
    await call('add_plugin',{directory:pluginDirectory});
  }
  await page.reload();await page.locator('#settings-button').click();
  await page.locator('[data-action="section"][data-id="updates"]').click();
  await page.locator('[data-action="check-all-updates"]').click();
  const firstUpdate=page.locator('[data-update-id="test.update-one"]'),secondUpdate=page.locator('[data-update-id="test.update-two"]');
  await firstUpdate.locator('[data-action="download-update"]').waitFor();
  await secondUpdate.locator('[data-update-select]').focus();
  await page.keyboard.press('Space');
  assert.match(await page.locator('[data-action="download-all-updates"]').textContent(),/所选（1）/);
  await secondUpdate.locator('[data-update-select]').check();
  await firstUpdate.locator('.update-release-notes summary').click();
  const notes=firstUpdate.locator('.release-notes-markdown');
  assert.equal(await notes.getByRole('heading',{name:'更新测试',level:1}).count(),1);
  assert.equal(await notes.getByRole('heading',{name:'改进',level:2}).count(),1);
  assert.equal((await notes.locator('ul ul li').textContent()).trim(),'使用 npm run check 验证');
  assert.equal(await notes.locator('strong').textContent(),'保留工作');
  assert.equal((await notes.locator('blockquote').textContent()).trim(),'完成工作后再重启。');
  assert.equal(await notes.locator('table tbody tr').count(),2);
  // The status ticker can replace a notes node while Playwright captures it.
  // Capture its current page region after the content assertions above.
  await notes.evaluate(node=>node.scrollIntoView({block:'start'}));
  await page.screenshot({path:'artifacts/production-ui/update-notes-markdown.png',clip:await notes.boundingBox()});
  const updatePageUrl=page.url();
  await notes.getByRole('link',{name:'发行说明'}).focus();await page.keyboard.press('Enter');
  await page.waitForFunction(()=>window.openedUpdateLinks?.length===1);
  assert.deepEqual(await page.evaluate(()=>window.openedUpdateLinks),['https://github.com/BgaSol/soDeck/releases']);
  assert.equal(page.url(),updatePageUrl,'Release-note links leave the workbench in place');
  await page.locator('.settings-page').evaluate(el=>el.scrollTo(0,0));
  await page.screenshot({path:'artifacts/production-ui/updates-available.png'});
  await page.locator('#automatic-updates').uncheck();
  await page.waitForFunction(()=>!document.querySelector('#automatic-updates').checked);
  assert.equal(await firstUpdate.locator('details.update-release-notes').getAttribute('open'),'');
  await page.locator('[data-action="download-all-updates"]').click();
  await firstUpdate.locator('.update-state').filter({hasText:'重启后应用'}).waitFor();
  await secondUpdate.locator('[data-action="download-update"]').filter({hasText:'重试下载'}).waitFor();
  await page.screenshot({path:'artifacts/production-ui/updates-partial-failure.png'});
  assert.equal((await call('snapshot')).plugins.find(p=>p.manifest.id==='test.update-one').manifest.version,'0.1.0');
  extraCorrupt=false;
  await secondUpdate.locator('[data-action="download-update"]').click();
  await secondUpdate.locator('.update-state').filter({hasText:'重启后应用'}).waitFor();
  await page.locator('[data-action="check-all-updates"]').click();
  await firstUpdate.locator('.update-state').filter({hasText:'重启后应用'}).waitFor();
  assert.equal(await page.locator('[data-action="download-all-updates"]').count(),0);
  await page.screenshot({path:'artifacts/production-ui/updates-ready.png'});
  await page.locator('[data-action="section"][data-id="plugins"]').click();
  assert.equal(await page.locator('[data-plugin-id]').count(),3);
  assert.equal(await page.locator('#update-repository').count(),0,'Technical source fields are not in the overview');
  await page.locator('[data-plugin-id="test.update-one"] [data-action="select-plugin"]').focus();await page.keyboard.press('Enter');
  assert.equal(await page.locator('[data-action="back-plugins"]').evaluate(el=>el===document.activeElement),true,'Plugin details move keyboard focus into the new page');
  await page.locator('details[data-persist="source-test.update-one"] summary').click();
  const sourceSummary = page.locator('details[data-persist="source-test.update-one"] summary');
  await sourceSummary.focus(); await page.keyboard.press('Tab');
  assert.equal(await page.locator('#update-repository').evaluate(el => el === document.activeElement), true, 'Summary Tab enters expanded fields');
  await page.keyboard.press('Shift+Tab'); assert.equal(await sourceSummary.evaluate(el => el === document.activeElement), true);
  await sourceSummary.click();await sourceSummary.focus();await page.keyboard.press('Tab');
  assert.equal(await page.locator('details[data-persist="technical-test.update-one"] summary').evaluate(el=>el===document.activeElement),true,'Closed details skip hidden fields');
  await sourceSummary.click();
  await page.locator('#update-repository').fill('draft/source');
  await page.evaluate(()=>window.emitTestEvent('core-changed'));await page.waitForTimeout(300);
  assert.equal(await page.locator('#update-repository').inputValue(),'draft/source');
  assert.equal(await page.locator('#update-repository').evaluate(el=>el===document.activeElement),true);
  assert.equal(await page.locator('details[data-persist="source-test.update-one"]').getAttribute('open'),'');
  await page.locator('[data-action="back-plugins"]').click();
  for (const theme of ['light','dark','dopamine']) {
    await page.locator('[data-action="section"][data-id="appearance"]').click();
    await page.locator(`[data-action="theme"][data-id="${theme}"]`).click();
    await page.waitForFunction(theme=>document.documentElement.dataset.theme===theme,theme);
    await page.locator('[data-action="section"][data-id="updates"]').click();
    if (await firstUpdate.locator('.update-release-notes').getAttribute('open') === null)
      await firstUpdate.locator('.update-release-notes summary').click();
    for(const [width,height] of [[1100,720],[880,576],[734,480],[720,480]]) {
      await page.setViewportSize({width,height});
      await page.locator('.settings-page').evaluate(el=>el.scrollTo(0,0));
      assert.equal(await page.locator('.settings-page').evaluate(el=>el.scrollWidth>el.clientWidth+1),false,`${theme}/${width} settings overflow`);
      assert.equal(await notes.getByRole('heading',{name:'更新测试',level:1}).count(),1);
      assert(await notes.locator('pre').evaluate(el=>el.scrollWidth>el.clientWidth),'Long code scrolls within the notes');
      assert(await notes.locator('h1').evaluate(el=>parseFloat(getComputedStyle(el).fontSize)>parseFloat(getComputedStyle(el.parentElement).fontSize)),'Markdown titles retain hierarchy');
      assert(await page.locator('[data-action="restart-app"]').evaluate(el=>{const r=el.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight&&r.left>=0&&r.right<=innerWidth;}),`${theme}/${width} restart action remains visible`);
      await page.screenshot({path:`artifacts/production-ui/updates-${theme}-${width}.png`});
      await page.locator('.maintenance-pending summary').focus();
      await page.keyboard.press('Enter');
      await page.evaluate(()=>window.emitTestEvent('core-changed'));await page.waitForTimeout(300);
      assert.equal(await page.locator('.maintenance-pending summary').evaluate(el=>el===document.activeElement),true);
      assert(await page.locator('[data-action="restart-app"]').isEnabled());
      await page.keyboard.press('Enter');
    }
  }
  await page.locator('[data-action="close-layer"]').click();
  checks.push('Unified updates: Markdown headings/lists/code/tables and external-link dispatch, real plugin batch checks/downloads and independent failure/retry, keyboard selection, preserved prepared versions, source drafts/disclosures/focus, deduplicated plugin overview and three themes at 1100x720, 880x576, 734x480 and 720x480');
  if (!process.argv.includes('--updates-only')) {
  await call('fixture_load_plugin',{directory:uiFixture});
  const workspace=await call('workspace_fixture_open',{serverId:serverIds[0],connectionId:'bundled-menu-test',endpoint:'root@192.0.2.10:22'});
  await page.reload();
  const board=page.locator(`.server-workbench[data-id="${workspace.context.workspaceId}"]`);
  await board.waitFor();
  await checkTitlebar();
  for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[1100,720],[720,480]]){
    await page.setViewportSize({width,height});
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    const tabMenu=page.locator(`.tab-menu-button[data-id="${workspace.context.workspaceId}"]`);
    const tabBounds=await tabMenu.boundingBox();
    await tabMenu.click();await assertMenuLayout(page,tabBounds);await page.keyboard.press('Escape');
    const panelMenu=board.getByRole('button',{name:'文件 面板菜单',exact:true});
    const panelBounds=await panelMenu.boundingBox();
    await panelMenu.click();await assertMenuLayout(page,panelBounds);
    if(theme==='dopamine'&&width===1100)await page.screenshot({path:'artifacts/production-ui/panel-menu.png'});
    await page.keyboard.press('Escape');
    const more=board.getByRole('button',{name:'左侧更多组件',exact:true});
    const moreBounds=await more.boundingBox();
    await more.click();const popup=await assertMenuLayout(page,moreBounds,{long:true});
    if(theme==='dopamine'&&width===1100)await page.screenshot({path:'artifacts/production-ui/long-component-menu.png'});
    const search=popup.getByRole('textbox'),searchTop=await search.evaluate(el=>el.getBoundingClientRect().top);
    await popup.locator('.component-menu-list').hover();await page.mouse.wheel(0,10000);
    await page.waitForFunction(()=>{const list=document.querySelector('.component-menu-list');return list.scrollTop>0&&list.scrollTop+list.clientHeight>=list.scrollHeight-1;});
    assert.equal(await search.evaluate(el=>el.getBoundingClientRect().top),searchTop);
    await popup.getByRole('textbox').fill('扩展测试组件 19');
    await assertMenuLayout(page,moreBounds);
    assert.equal(await popup.locator('.component-menu-row:visible').count(),1);
    await page.keyboard.press('Escape');assert(await more.evaluate(el=>el===document.activeElement));
  }
  checks.push('Bundled workspace menus: fixed placement, no short-menu scrollbars, vertical-only long lists, search and Escape across all three themes at 1100×720 and 720×480');
  await page.setViewportSize({width:1100,height:720});await page.locator('#settings-button').click();
  await page.locator('.settings-nav').getByRole('button',{name:'显示偏好验收',exact:true}).click();const preference=page.locator('#component-settings-page').getByLabel('名称字号');await preference.fill('17');
  await preference.evaluate(el=>window.componentPreferenceInput=el);await page.evaluate(()=>window.emitTestEvent('core-changed'));await page.waitForTimeout(150);
  assert(await preference.evaluate(el=>el===window.componentPreferenceInput));assert.equal(await preference.inputValue(),'17');
  await page.waitForTimeout(200);assert(await preference.evaluate(el=>el===document.activeElement),'Retained form focus survives global refresh');
  await page.locator('[data-action="close-layer"]').click();await page.locator('#settings-button').click();assert.equal(await preference.inputValue(),'17');
  await page.locator('#component-settings-page').getByRole('button',{name:'保存',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.settings-feedback')?.textContent.includes('设置已保存'));
  const preferenceValue=await call('settings_page',{pluginId:uiMetadata.id,pageId:'display'});assert.equal(preferenceValue.values.fontSize,17);
  await page.locator('[data-action="close-layer"]').click();checks.push('Bundled component settings: existing navigation, core save, global refresh preserves input DOM, closing/reopening keeps drafts');
  await page.locator('#status-button').click();await page.getByRole('button',{name:'查看日志',exact:true}).click();
  await page.locator('.log-row').first().waitFor();const currentLogs=await page.locator('#log-results').textContent();assert(currentLogs.length>0);
  await page.locator('#log-query').fill('held-old-filter');await page.waitForFunction(()=>true);
  for(let i=0;!heldLogReply&&i<50;i++)await page.waitForTimeout(20);assert(heldLogReply);
  await page.locator('[data-action="pause-logs"]').click();
  await page.locator('#log-source').selectOption('core');await page.locator('#log-level').selectOption('error');
  await page.locator('#log-since').fill('2020-01-01T00:00');await page.locator('#log-until').fill('2020-01-02T00:00');
  await page.getByRole('button',{name:'清除筛选',exact:true}).click();
  assert.deepEqual(await page.locator('#log-source,#log-level,#log-query,#log-since,#log-until').evaluateAll(nodes=>nodes.map(n=>n.value)),['','','','','']);
  releaseLogReply();await page.waitForTimeout(100);assert.equal(await page.locator('#log-results').textContent(),currentLogs,'Paused clear keeps records and rejects stale response');
  assert.equal(await page.getByRole('button',{name:'继续显示',exact:true}).count(),1);
  assert.equal(await page.locator('#clear-log-filters').evaluate(el=>el===document.activeElement),true);
  await page.getByRole('button',{name:'继续显示',exact:true}).click();await page.locator('.log-row').first().waitFor();
  assert.equal(await page.locator('.log-meta').first().evaluate(el=>getComputedStyle(el).fontSize),'12px');
  await page.keyboard.press('Escape');
  await call("workspace_move", { window: "main", id: workspace.context.workspaceId, target: "workspace-titlebar", index: 0 });
  testWindow = "workspace-titlebar";
  await page.evaluate(() => sessionStorage.setItem("test-window", "workspace-titlebar"));
  await page.reload();
  await board.waitFor();
  await checkTitlebar();
  checks.push("Pop-out title bar: same maximize/restore behavior; server tabs and their controls retain their own interactions");
  }
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ passed: checks.length, checks, pageErrors: errors }, null, 2));
} catch (error) {
  await page.screenshot({ path: "artifacts/production-ui/failure.png" });
  if (await dialog().count()) console.error(await dialog().evaluate(el => [...el.querySelectorAll("form,fieldset,header,footer")].map(node => ({ tag: node.tagName, css: node.className, height: node.getBoundingClientRect().height, scroll: node.scrollHeight, client: node.clientHeight, min: getComputedStyle(node).minHeight, flex: getComputedStyle(node).flex }))));
  throw error;
} finally {
  await browser.close(); await new Promise(resolve => server.httpServer.close(resolve));
  if (child.exitCode === null) { const exited = new Promise(resolve => child.once("exit", resolve)); child.stdin.end(); await exited; }
  await new Promise(resolve => downloads.close(resolve));
  fs.rmSync(directory, { recursive: true, force: true });
}
