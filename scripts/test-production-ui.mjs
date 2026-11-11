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
import { recordTrace } from './ui-trace.mjs';
import { parseProductionUiTestArgs } from './test-cli-arguments.mjs';

const { updatesOnly } = parseProductionUiTestArgs(process.argv.slice(2));
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
const filesFixture = path.join(directory, 'files-fixture'); fs.mkdirSync(filesFixture);
fs.copyFileSync(path.join(fixture, `plugin-fixture${suffix}`), path.join(filesFixture, `plugin-fixture${suffix}`));
if (process.platform !== 'win32') fs.chmodSync(path.join(filesFixture, `plugin-fixture${suffix}`), 0o755);
fs.writeFileSync(path.join(filesFixture, 'plugin.json'), JSON.stringify({ id:'sodeck.files', name:'文件安装验收', version:'0.1.1', apiVersion:'1', target, entry:`plugin-fixture${suffix}` }));
const filesArchive = path.join(directory, 'files.tar.gz');
assert.equal(spawnSync(tar, ['-czf', filesArchive, '-C', directory, 'files-fixture'], { windowsHide:true }).status, 0);
const filesBytes = fs.readFileSync(filesArchive);
const catalogFiles = new Map(fs.readdirSync('plugins').filter(name=>fs.existsSync(`plugins/${name}/release.json`)).map(name=>[name, JSON.parse(fs.readFileSync(`plugins/${name}/release.json`,'utf8'))]));
const catalogIds = [...catalogFiles.values()].map(config=>config.id).sort();
let catalogUnavailable = true, holdCatalog = false, releaseCatalog;
let holdDiagnostic = false, releaseDiagnostic, diagnosticReached;
const markdownNotes = '# 更新测试\n\n## 改进\n\n- **保留工作**并支持*重试*\n  - 使用 `npm run check` 验证\n- 下载后继续使用\n\n> 完成工作后再重启。\n\n[发行说明](https://github.com/BgaSol/soDeck/releases)\n\n```sh\nnpm run check\n' + 'a'.repeat(180) + '\n```\n\n| 项目 | 结果 |\n| --- | --- |\n| 核心 | 可更新 |\n| 插件 | 独立下载 |';
const downloads = http.createServer(async (request, response) => {
  requests.push(request.url);
  if (request.url.startsWith('/diagnostic/') && holdDiagnostic) { diagnosticReached(); await new Promise(resolve=>releaseDiagnostic=resolve); }
  if (request.url.startsWith('/diagnostic-failed/')) { response.writeHead(503).end(); return; }
  if (request.url.startsWith('/diagnostic/')) { const original=new URL(request.url.slice('/diagnostic/'.length)); request.url=original.pathname+original.search; }
  const base = `http://127.0.0.1:${downloads.address().port}`;
  const blobs = [...catalogFiles].map(([name,config])=>({path:`plugins/${name}/release.json`,type:'blob',sha:createHash('sha256').update(JSON.stringify(config)).digest('hex').slice(0,40),config}));
  let data;
  if (request.url.startsWith('/repos/BgaSol/soDeck/git/trees/main')) {
    if (holdCatalog) await new Promise(resolve=>releaseCatalog=resolve);
    if (catalogUnavailable) { response.writeHead(503).end(); return; }
    data = JSON.stringify({truncated:false,tree:blobs.map(({config,...entry})=>entry)});
  }
  else if (request.url.startsWith('/repos/BgaSol/soDeck/git/blobs/')) {
    assert.equal(request.headers.accept,'application/vnd.github+json');
    const item=blobs.find(item=>request.url.endsWith(item.sha));
    if(!item){response.writeHead(404).end();return;}
    const content=Buffer.from(JSON.stringify(item.config));
    data=JSON.stringify({encoding:'base64',size:content.length,content:content.toString('base64')});
  }
  else if (request.url.startsWith("/repos/BgaSol/soDeck/releases")) data = JSON.stringify(published ? [{ tag_name: "plugin-terminal-v0.1.1", draft: false, prerelease: false, assets: [{ name: "plugin-update.json", browser_download_url: `${base}/manifest.json` }] }, { tag_name:'plugin-files-v0.1.1', draft:false, prerelease:false, assets:[{name:'plugin-update.json',browser_download_url:`${base}/files.json`}] }, ...extraReleases] : []);
  else if (request.url === '/core-update.json') data = JSON.stringify({id:'sodeck.core',version:'0.1.1',platforms:{[target]:{url:`${base}/fixture.tar.gz`,signature:'fixture-diagnostic-only'}}});
  else if (request.url === '/files.json') data = JSON.stringify({ id:'sodeck.files', version:'0.1.1', apiVersion:'1', platforms:{[target]:{url:`${base}/files.tar.gz`,sha256:createHash('sha256').update(filesBytes).digest('hex')}} });
  else if (request.url === '/files.tar.gz') data = filesBytes;
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
const finishTrace = await recordTrace(page.context(), 'production');
await page.route('**/__plugin__/**',route=>{
  const relative=decodeURIComponent(new URL(route.request().url()).pathname).split('/').slice(3).join('/');
  const file=path.join(uiFixture,relative),types={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
  return route.fulfill({contentType:types[path.extname(file)]||'application/octet-stream',body:fs.readFileSync(file)});
});
const errors = [], checks = [];
let failCatalog = false, testWindow = "main", heldLogReply, releaseLogReply, chosenPluginDirectory = null;
let holdLocalNetwork = false, localNetworkReached, releaseLocalNetwork;
let holdPickerCatalog = false, releasePickerCatalog, pickerConnects = 0;
page.on("pageerror", error => errors.push(String(error)));
await page.exposeBinding("desktopInvoke", async (_, command, args) => {
  if (command === 'plugin:dialog|open') return chosenPluginDirectory;
  if (command === 'server_catalog' && failCatalog) throw new Error('隔离测试：目录暂时不可读');
  const result=await call(command, { ...args, window: testWindow });
  if(command==='connection'&&args.command?.op==='start')pickerConnects++;
  if(command==='server_catalog'&&holdPickerCatalog){holdPickerCatalog=false;await new Promise(resolve=>releasePickerCatalog=resolve);}
  if(command==='network_snapshot'&&holdLocalNetwork){holdLocalNetwork=false;localNetworkReached();await new Promise(resolve=>releaseLocalNetwork=resolve);}
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
      return window.desktopInvoke(command, args).then(result=>{
        if(command==='add_plugin')window.localPluginAdded=true;
        if(command==='server_catalog')(window.receivedCatalogRevisions??=[]).push(result.revision);
        return result;
      });
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
    if (document.querySelector(".server-tab")) interactive.push(".server-tab", ".server-tab .tab-name", ".tab-close-button svg");
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
async function checkTitlebarPosition(state) {
  const viewport = page.viewportSize();
  const original = await page.evaluate(() => ({ theme: document.documentElement.dataset.theme, macos: document.documentElement.classList.contains('macos') }));
  for (const theme of ['light', 'dark', 'dopamine']) for (const width of [1100, 720]) for (const macos of [false, true]) {
    await page.setViewportSize({ width, height: 720 });
    const layout = await page.evaluate(({ theme, macos }) => {
      document.documentElement.dataset.theme = theme;
      document.documentElement.classList.toggle('macos', macos);
      const strip = document.querySelector('#server-tabs').getBoundingClientRect();
      const last = (document.querySelector('.server-tab:last-child') || document.querySelector('#catalog-tab')).getBoundingClientRect();
      const plus = document.querySelector('#new-workspace').getBoundingClientRect();
      const tools = document.querySelector('.global-tools').getBoundingClientRect();
      return { gap: plus.left - Math.min(last.right, strip.right), offset: plus.y + plus.height / 2 - last.y - last.height / 2, width: plus.width, visible: plus.left >= strip.right && plus.right <= tools.left };
    }, { theme, macos });
    fs.mkdirSync('artifacts/titlebar-position', { recursive: true });
    if (width === 1100 && !macos) await page.screenshot({ path: `artifacts/titlebar-position/${state}-${theme}.png`, clip: { x: 0, y: 0, width, height: 80 } });
    const label = `${state}/${theme}/${width}/${macos ? 'macos' : 'windows-linux'}: ${JSON.stringify(layout)}`;
    assert(layout.gap >= 0 && layout.gap <= 8, `Plus follows the last visible tab: ${label}`);
    assert(Math.abs(layout.offset) <= 1, `Plus aligns with the tab center: ${label}`);
    assert.equal(layout.width, 28, label);
    assert(layout.visible, `Plus remains outside the scrolling tabs: ${label}`);
  }
  await page.setViewportSize(viewport);
  await page.evaluate(({ theme, macos }) => { document.documentElement.dataset.theme = theme; document.documentElement.classList.toggle('macos', macos); }, original);
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
  assert.deepEqual(requests, [], 'Starting the application does not depend on the official plugin network catalog');
  await checkTitlebarPosition('empty');
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
  const pickerConnections=pickerConnects;
  await page.setViewportSize({width:720,height:480});
  const pickerAnchorStyle=await plus.getAttribute('style');
  await plus.evaluate(node=>Object.assign(node.style,{position:'fixed',left:'680px',top:'400px',zIndex:'100'}));
  await plus.click();await picker.getByRole('status').filter({hasText:'暂无服务器'}).waitFor();
  await page.waitForFunction(()=>{const node=document.querySelector('#server-picker'),r=node.getBoundingClientRect();return node.style.visibility==='visible'&&node.dataset.placement?.startsWith('top')&&r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8;},null,{timeout:2000});
  assert.match(await picker.getAttribute('data-placement'),/^top/,'A low anchor flips the picker above it');
  await plus.evaluate(node=>node.style.top='40px');
  await page.waitForFunction(()=>{const anchor=document.querySelector('#new-workspace').getBoundingClientRect(),popup=document.querySelector('#server-picker').getBoundingClientRect();return Math.abs(popup.top-anchor.bottom-4)<=1;},null,{timeout:2000});
  await plus.evaluate(node=>node.hidden=true);await picker.waitFor({state:'hidden',timeout:2000});
  await plus.evaluate((node,style)=>{node.hidden=false;if(style===null)node.removeAttribute('style');else node.setAttribute('style',style);},pickerAnchorStyle);
  await page.setViewportSize({width:1100,height:720});
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
  holdPickerCatalog=true;releasePickerCatalog=undefined;
  await plus.click();
  for(let attempt=0;!releasePickerCatalog&&attempt<100;attempt++)await page.waitForTimeout(20);
  assert(releasePickerCatalog,'The isolated catalog response is held');
  const oldPickerRevision=(await call('server_catalog')).revision;
  await plus.click();await picker.waitFor({state:'hidden'});
  const freshPickerServer=await call('change_catalog',{revision:oldPickerRevision,change:{op:'saveServer',draft:{name:'迟到响应验收',host:'192.0.2.30',port:22,username:'ops',groupId,authentication:{type:'password',secret:{action:'clear'}},route:[]}}});
  await plus.click();await search.fill('迟到响应验收');await picker.getByRole('button',{name:/迟到响应验收/}).waitFor();
  const oldPickerReplies=await page.evaluate(revision=>window.receivedCatalogRevisions.filter(value=>value===revision).length,oldPickerRevision);
  releasePickerCatalog();releasePickerCatalog=undefined;
  await page.waitForFunction(({revision,count})=>window.receivedCatalogRevisions.filter(value=>value===revision).length>count,{revision:oldPickerRevision,count:oldPickerReplies});
  assert.equal(await search.inputValue(),'迟到响应验收','A prior opening cannot overwrite the current search');
  assert.equal(await picker.getByRole('button',{name:/迟到响应验收/}).count(),1,'A prior catalog cannot replace the current opening');
  await page.keyboard.press('Escape');
  await call('change_catalog',{revision:(await call('server_catalog')).revision,change:{op:'deleteServer',id:freshPickerServer.selectedId}});
  assert.equal(pickerConnects,pickerConnections,'Positioning and repeated opens do not initiate a connection');
  checks.push('一级加号定位：边缘翻转、锚点移动和隐藏、关闭重开后的迟到目录响应，以及定位不触发连接');
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
  assert.equal(await page.locator('#settings-nav-appearance').getAttribute('aria-current'), 'page', 'Settings initially opens preferences');
  assert.deepEqual(await page.locator('.settings-nav .nav-label').allTextContents(), ['偏好设置','应用管理']);
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
  await page.getByRole('alert').filter({hasText:'无法获取官方插件列表'}).waitFor();
  assert.equal(await page.locator('[data-official-plugin]').count(),0,'An unavailable catalog is not presented as an empty successful list');
  assert(await page.locator('#choose-plugin').isEnabled(),'Local plugins remain available when the repository cannot be reached');
  catalogUnavailable = false; holdCatalog = true;
  await page.locator('#refresh-official-plugins').click();
  await page.getByRole('status').filter({hasText:'正在从官方仓库获取插件'}).waitFor();
  await page.locator('[data-action="section"][data-id="about"]').click();
  assert.equal(await page.getByRole('heading',{name:'应用与数据',exact:true}).count(),1,'A pending catalog request does not block settings navigation');
  for(let i=0;!releaseCatalog&&i<50;i++)await page.waitForTimeout(20);
  assert(releaseCatalog);holdCatalog=false;releaseCatalog();
  await page.getByRole('button',{name:'使用帮助',exact:true}).focus();await page.keyboard.press('Enter');
  assert.equal(await page.evaluate(()=>window.openedUsageHelp),true);
  const closeLayer=page.locator('[data-action="close-layer"]');
  await page.getByRole('button',{name:'复制日志路径',exact:true}).focus();await page.keyboard.press('Tab');
  assert.equal(await closeLayer.evaluate(el=>el===document.activeElement),true,'Last Tab wraps to close');
  await page.keyboard.press('Shift+Tab');assert.equal(await page.getByRole('button',{name:'复制日志路径',exact:true}).evaluate(el=>el===document.activeElement),true);
  await page.keyboard.press('Escape');assert.equal(await page.locator('#settings-button').evaluate(el=>el===document.activeElement),true);
  await page.locator('#settings-button').click();await page.locator('[data-action="section"][data-id="plugins"]').click();
  assert.equal(await page.locator('#plugins-official').getAttribute('aria-pressed'),'true','An empty installation opens official plugins directly');
  await page.locator('#plugins-installed').focus();await page.keyboard.press('Enter');
  await page.getByRole('heading',{name:'还没有安装插件'}).waitFor();
  await page.locator('#plugins-official').focus();await page.keyboard.press('Enter');
  assert.equal(await page.locator('#plugins-official').evaluate(el=>el===document.activeElement),true,'Switching lists retains keyboard focus');
  const terminal = page.locator('[data-official-plugin="sodeck.terminal"]');
  const install = () => terminal.locator('[data-action="install-official-plugin"]');
  await install().waitFor();
  assert.deepEqual(await page.locator('[data-official-plugin]').evaluateAll(nodes => nodes.map(node => node.dataset.officialPlugin).sort()), catalogIds);
  assert.match(await page.locator('[data-official-plugin="sodeck.docker"]').textContent(), /查看远端容器/);
  assert.match(await page.locator('[data-official-plugin="sodeck.terminal"]').textContent(), /真实终端/);
  assert.match(await page.locator('[data-official-plugin="sodeck.files"]').textContent(), /上传下载、管理文件/);
  assert(requests.every(url=>url.startsWith('/repos/BgaSol/soDeck/git/')), "Opening settings reads catalog metadata without checking releases or downloading packages");
  await page.locator('#choose-plugin').click();
  assert.equal(await page.locator('#plugins-official').getAttribute('aria-pressed'),'true','Cancelling the directory picker leaves the official list in place');
  assert.equal((await call('snapshot')).plugins.length,0);
  await page.screenshot({path:'artifacts/production-ui/plugins-discover.png'});
  catalogFiles.set('new-tool',{id:'example.catalog-new',name:'仓库新增插件',description:'无需升级核心即可发现',tagPrefix:'new-tool-v',icon:'plug',order:50});
  await page.locator('#refresh-official-plugins').click();
  await page.locator('[data-official-plugin="example.catalog-new"]').waitFor();
  assert.equal(await page.locator('[data-official-plugin]').count(),catalogIds.length+1,'A repository addition appears in the running core');
  catalogUnavailable=true;
  await page.locator('#refresh-official-plugins').click();
  await page.getByRole('alert').filter({hasText:'上次成功获取'}).waitFor();
  assert.equal(await page.locator('[data-official-plugin]').count(),catalogIds.length+1,'Refresh failure preserves the last complete catalog');
  catalogUnavailable=false;catalogFiles.delete('new-tool');
  await page.locator('#refresh-official-plugins').click();
  await page.locator('[data-official-plugin="example.catalog-new"]').waitFor({state:'detached'});
  assert.equal(await page.locator('.official-catalog-status + [role="alert"]').count(),0,'A successful refresh clears the cache warning');
  await install().click(); await terminal.getByRole("alert").waitFor();
  assert.match(await terminal.getByRole("alert").textContent(), /尚未发布/);
  assert.equal((await call("snapshot")).plugins.length, 0);
  published = true; corrupt = true;
  await install().click(); await page.waitForFunction(() => document.querySelector('[data-official-plugin="sodeck.terminal"] [role="alert"]')?.textContent.includes("SHA-256"));
  assert.equal((await call("snapshot")).plugins.length, 0);
  corrupt = false; await install().click();
  await page.locator('.maintenance-pending').waitFor();
  assert.match(await page.locator('.maintenance-pending').textContent(), /等待重启/);
  assert.equal(await page.locator('[data-official-plugin]').count(), catalogIds.length, 'Installation stays in the official list for the next plugin');
  assert.equal(await terminal.locator('[data-action="install-official-plugin"]').count(), 0, 'An installed plugin cannot be installed twice');
  assert.match(await terminal.locator('[role="status"]').textContent(), /已安装 · 重启后启用/);
  const installed = (await call("snapshot")).plugins[0];
  assert(installed.enabled && installed.pendingRestart);
  assert.equal(installed.status, "unloaded");
  assert.equal(installed.manifest.updateSource.repository, "BgaSol/soDeck");
  const files = page.locator('[data-official-plugin="sodeck.files"]');
  await files.locator('[data-action="install-official-plugin"]').click();
  await files.locator('[data-action="select-plugin"]').waitFor();
  assert.equal((await call('snapshot')).plugins.length, 2, 'A second official plugin installs without leaving the list');
  await terminal.locator('[data-action="select-plugin"]').click();
  assert.equal(await page.locator('[data-action="download-update"],[data-action="check-update"]').count(),0,'Plugin details route upgrades to software updates');
  await page.locator('[data-action="back-plugins"]').click();
  assert.equal(await page.locator('#plugins-official').getAttribute('aria-pressed'),'true','Back returns to the same plugin list');
  assert.equal(await terminal.locator('[data-action="select-plugin"]').evaluate(el=>el===document.activeElement),true,'Back restores focus to the plugin');
  await page.locator('.toast-error').last().waitFor({state:'hidden'});
  for (const theme of ['light','dark','dopamine']) {
    await page.locator('#settings-nav-appearance').click();
    await page.locator(`#theme-${theme}`).click();
    await page.waitForFunction(theme=>document.documentElement.dataset.theme===theme,theme);
    await page.locator('#settings-nav-plugins').click();
    for (const [width,height] of [[1100,720],[720,480]]) {
      await page.setViewportSize({width,height});
      assert.equal(await page.locator('.settings-page').evaluate(el=>el.scrollWidth>el.clientWidth+1),false,`${theme}/${width} official plugins fit`);
      await page.locator('[data-official-plugin="sodeck.system-status"] button').scrollIntoViewIfNeeded();
      assert(await page.locator('[data-official-plugin="sodeck.system-status"] button').isEnabled());
      await page.locator('.settings-page').evaluate(el=>el.scrollTo(0,0));
      const firstAction = await terminal.locator('button').boundingBox(), pendingBar = await page.locator('.maintenance-pending').boundingBox();
      assert(firstAction.y + firstAction.height <= pendingBar.y, `${theme}/${width} first plugin action is visible without scrolling`);
      await page.screenshot({path:`artifacts/production-ui/plugins-official-${theme}-${width}.png`});
    }
  }
  await page.setViewportSize({width:1100,height:720});
  await page.reload(); await page.locator("#settings-button").click();
  await page.locator('#settings-nav-plugins').click();
  await page.locator('[data-plugin-id="sodeck.terminal"]').waitFor();
  assert.equal(await page.locator('[data-plugin-id="sodeck.terminal"]').count(),1);
  assert.equal((await call("snapshot")).plugins.length, 2);
  await page.screenshot({ path: "artifacts/production-ui/official-plugins.png", animations: "disabled" });
  checks.push("Plugin management: empty install opens official list, keyboard list switching, no background downloads, unpublished/corrupt retries, two consecutive native package installs without leaving the list, no duplicate install, back restores list/focus, persisted installed default, three themes at 1100x720 and 720x480");
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
    if (name === 'update-one') {
      // A refresh may still contain the old plugin list when the picker returns.
      holdLocalNetwork=true;
      const reached=new Promise(resolve=>localNetworkReached=resolve);
      await page.locator('#plugins-official').click();
      await reached;
      chosenPluginDirectory = pluginDirectory;
      await page.locator('#choose-plugin').click();
      await page.waitForFunction(() => window.localPluginAdded);
      releaseLocalNetwork();
      await page.locator('#back-plugins').waitFor();
      await page.waitForFunction(() => document.activeElement?.id === 'back-plugins');
      assert.equal(await page.locator('#back-plugins').evaluate(el=>el===document.activeElement),true,'Local registration focuses plugin details');
      await page.locator('#back-plugins').click();
      assert.equal(await page.locator('#plugins-installed').getAttribute('aria-pressed'),'true','A local plugin returns to the installed list');
      await page.locator('[data-plugin-id="test.update-one"]').waitFor();
      chosenPluginDirectory = null;
    } else await call('add_plugin',{directory:pluginDirectory});
  }
  await page.reload();await page.locator('#settings-button').click();
  await page.locator('[data-action="section"][data-id="updates"]').click();
  await page.locator('#update-network-summary').click();
  await page.locator('#update-network-summary').focus();await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(()=>document.activeElement.id),'network-fallback','Keyboard enters expanded network settings');
  await page.locator('#network-fallback').uncheck();
  await page.locator('#network-add').click();
  await page.locator('#mirror-name-0').fill('公司节点');
  await page.locator('#mirror-prefix-0').fill('http://example.com');
  await page.locator('#network-save').click();
  await page.locator('.update-network [role="alert"]').filter({hasText:'HTTPS'}).waitFor();
  assert.equal(await page.locator('#mirror-name-0').inputValue(),'公司节点');
  await page.locator('#mirror-prefix-0').fill('https://Example.com/proxy');
  await page.evaluate(()=>window.emitTestEvent('core-changed'));await page.waitForTimeout(300);
  assert.equal(await page.locator('#mirror-prefix-0').inputValue(),'https://Example.com/proxy');
  assert.equal(await page.locator('#mirror-prefix-0').evaluate(el=>el===document.activeElement),true);
  await page.locator('#network-save').click();
  await page.locator('.network-notice').waitFor();
  assert.equal((await call('snapshot')).preferences.updateNetwork.customMirrors[0].prefix,'https://example.com/proxy/');
  await page.locator('#network-add').click();
  await page.locator('#mirror-name-1').fill('备用节点');
  await page.locator('#mirror-prefix-1').fill('https://backup.example.com/');
  await page.locator('[data-action="network-up"][data-id="1"]').click();
  await page.waitForFunction(()=>document.querySelector('#mirror-name-0')?.value==='备用节点');
  assert.equal(await page.locator('#mirror-name-0').inputValue(),'备用节点');
  await page.locator('#mirror-enabled-1').uncheck();
  await page.locator('#mirror-builtin-ghfast').uncheck();
  const concurrent=(await call('snapshot')).preferences.updateNetwork;
  await call('set_update_network',{expectedRevision:concurrent.revision,preferences:concurrent});
  await page.locator('#network-save').click();
  await page.locator('.update-network [role="alert"]').filter({hasText:'另一窗口'}).waitFor();
  assert.equal(await page.locator('#mirror-name-0').inputValue(),'备用节点');
  await page.locator('[data-action="network-reload"]').click();
  await page.locator('#network-fallback').check();
  await page.locator('#mirror-builtin-ghfast').uncheck();
  await page.locator('#network-save').click();await page.locator('.network-notice').waitFor();
  await page.waitForTimeout(300);
  assert((await call('snapshot')).preferences.updateNetwork.disabledBuiltins.includes('ghfast'));
  await page.locator('#network-test-custom-0').click();
  await page.locator('.network-diagnostic li').filter({hasText:'安装包首段'}).waitFor();
  assert.equal(await page.locator('.network-diagnostic .update-error').count(),0);
  assert(requests.some(p=>p.startsWith('/diagnostic/http://127.0.0.1:')));
  holdDiagnostic=true;
  const diagnosticStarted=new Promise(resolve=>diagnosticReached=resolve);
  await page.locator('#network-test-custom-0').click();await diagnosticStarted;
  await page.locator('#mirror-prefix-0').fill('https://edited.example.com/');
  holdDiagnostic=false;releaseDiagnostic();
  await page.waitForFunction(()=>document.querySelector('#network-test-custom-0').textContent==='测试');
  assert.equal(await page.locator('.network-diagnostic').count(),0,'Late diagnostic results must not overwrite edited address state');
  await page.locator('#mirror-prefix-0').fill('https://failed.example.com/');
  const diagnosticStart=requests.length;
  await page.locator('#network-test-custom-0').click();
  await page.locator('.network-diagnostic .update-error').first().waitFor();
  assert(requests.slice(diagnosticStart).every(p=>p.startsWith('/diagnostic-failed/')),'A selected failing endpoint must never report another endpoint success');
  await page.locator('[data-action="network-reload"]').click();
  await page.locator('#network-fallback').uncheck();
  for (const theme of ['light','dark','dopamine']) for (const [width,height] of [[1100,720],[720,480]]) {
    await page.setViewportSize({width,height});
    await call('set_theme',{theme});
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await page.waitForTimeout(160); // Let the existing 100 ms control background transition finish.
    await page.evaluate(()=>document.querySelector('#network-add').scrollIntoView({block:'center'}));
    assert.equal(await page.locator('.settings-page').evaluate(el=>el.scrollWidth>el.clientWidth+1),false,`${theme}/${width} network settings fit`);
    const networkContrasts=await page.locator('.network-intro,.network-custom > label:not(.network-toggle),.network-builtin small,.update-network .primary:not(:disabled),.update-network .text-button:not(:disabled)').evaluateAll(nodes=>{
      const luminance=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};
      return nodes.map(node=>{let surface=node;while(surface.parentElement&&Number(getComputedStyle(surface).backgroundColor.match(/[\d.]+/g)?.[3]??1)===0)surface=surface.parentElement;const a=luminance(getComputedStyle(node).color),b=luminance(getComputedStyle(surface).backgroundColor);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);});
    });
    assert(networkContrasts.length&&networkContrasts.every(ratio=>ratio>=4.5),`${theme}/${width} network text contrast: ${networkContrasts}`);
    await page.screenshot({path:`artifacts/production-ui/update-network-${theme}-${width}.png`});
  }
  await page.setViewportSize({width:1100,height:720});
  await page.locator('#update-network-summary').click();
  await page.locator('.settings-page').evaluate(el=>el.scrollTo(0,0));
  checks.push('Update network: editable drafts survive refresh and retain focus, invalid URL and revision conflict preserve input, custom ordering/toggles, normalized save, single-endpoint API/blob/manifest/package diagnostics, three themes and small window');
  await page.locator('[data-action="check-all-updates"]').click();
  const firstUpdate=page.locator('[data-update-id="test.update-one"]'),secondUpdate=page.locator('[data-update-id="test.update-two"]');
  await firstUpdate.locator('[data-action="download-update"]').waitFor();
  await secondUpdate.locator('[data-update-select]').focus();
  await page.keyboard.press('Space');
  assert.match(await page.locator('[data-action="download-all-updates"]').textContent(),/所选（1）/);
  await secondUpdate.locator('[data-update-select]').check();
  await page.locator('#settings-nav-plugins').click();
  const updateShortcut = page.locator('[data-plugin-id="test.update-one"] [data-action="plugin-updates"]');
  await updateShortcut.click();
  assert.equal(await firstUpdate.evaluate(el=>el===document.activeElement),true,'A plugin update shortcut locates and focuses its software update row');
  assert.equal(await page.locator('.update-group').first().getAttribute('aria-label'),'SoDeck 应用');
  assert.equal(await firstUpdate.evaluate(el=>el.closest('.update-group').getAttribute('aria-label')),'插件更新');
  assert.equal(await page.locator('[aria-label="插件更新"] .update-row').first().getAttribute('data-update-id'),'test.update-one','Actionable plugins precede current versions');
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
  assert.equal(await page.locator('[data-plugin-id]').count(),4);
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
    const navigationContrasts=await page.locator('.settings-nav').evaluate(nav=>{
      const luminance=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};
      const background=luminance(getComputedStyle(nav).backgroundColor);
      return [...nav.querySelectorAll('.nav-label,.nav-bottom,.settings-nav-hint')].map(node=>{const foreground=luminance(getComputedStyle(node).color);return {text:node.textContent,ratio:(Math.max(foreground,background)+.05)/(Math.min(foreground,background)+.05)};});
    });
    assert(navigationContrasts.length>=2&&navigationContrasts.every(item=>item.ratio>=4.5),`${theme} navigation small text contrast: ${JSON.stringify(navigationContrasts)}`);
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
  if (!updatesOnly) {
  await call('fixture_load_plugin',{directory:uiFixture});
  const workspace=await call('workspace_fixture_open',{serverId:serverIds[0],connectionId:'bundled-menu-test',endpoint:'root@192.0.2.10:22'});
  await page.reload();
  const board=page.locator(`.server-workbench[data-id="${workspace.context.workspaceId}"]`);
  await board.waitFor();
  await checkTitlebarPosition('single');
  await checkTitlebar();
  for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[1100,720],[720,480]]){
    await page.setViewportSize({width,height});
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    const tabMenu=page.locator(`.server-tab-select[data-id="${workspace.context.workspaceId}"]`);
    const tabBounds=await tabMenu.boundingBox();
    const point={x:Math.round(tabBounds.x+tabBounds.width/2),y:Math.round(tabBounds.y+tabBounds.height/2),width:0,height:0};await page.mouse.click(point.x,point.y,{button:"right"});await assertMenuLayout(page,point,{point:true});await page.keyboard.press("Escape");
    assert.equal(await board.locator('[data-component-action="panel-menu"]').count(),0);
    assert.equal(await board.getByRole('button',{name:'收起文件',exact:true}).count(),1);
    if(theme==='dopamine'&&width===1100)await page.screenshot({path:'artifacts/production-ui/panel-header.png'});
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
  for (const destination of ['appearance','plugins','updates']) {
    await page.locator(`#settings-nav-${destination}`).click();
    assert.equal(await page.locator('.settings-nav [aria-current="page"]').count(),1,'Only the visible settings page is selected');
    assert.equal(await page.locator('.settings-nav .nav-item.active').count(),1,'Previous component settings do not stay highlighted');
    await page.locator('.settings-nav').getByRole('button',{name:'显示偏好验收',exact:true}).click();
    assert.equal(await preference.inputValue(),'17','Returning to component preferences preserves their values');
    assert.equal(await page.locator('.settings-nav [aria-current="page"]').count(),1);
  }
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
  const messageFixture=path.join(directory,'messages');fs.cpSync(fixture,messageFixture,{recursive:true});
  const messageManifest=JSON.parse(fs.readFileSync(path.join(messageFixture,'plugin.json'),'utf8'));messageManifest.id='test.messages';
  fs.writeFileSync(path.join(messageFixture,'plugin.json'),JSON.stringify(messageManifest));
  await call('fixture_load_plugin',{directory:messageFixture});
  await page.locator('#messages-button').click();
  const allMessages=page.locator('[data-action="all-messages"]'),unreadMessages=page.locator('[data-action="unread-messages"]');
  assert.equal(await allMessages.getAttribute('aria-pressed'),'true','Message filters expose their selected state');
  await unreadMessages.focus();await page.keyboard.press('Enter');
  assert.equal(await unreadMessages.getAttribute('aria-pressed'),'true');assert.equal(await allMessages.getAttribute('aria-pressed'),'false');
  assert.equal(await unreadMessages.evaluate(el=>el===document.activeElement),true);
  await allMessages.focus();await page.keyboard.press('Enter');
  await page.locator('.message-row').first().focus();await page.keyboard.press('Enter');
  await page.locator('.message-row.selected').waitFor();
  const messageContrasts=await page.locator('.message-row.selected').evaluate(row=>{
    const luminance=color=>{const c=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};
    const background=luminance(getComputedStyle(row).backgroundColor);
    return [...row.querySelectorAll('time,p,small')].map(node=>{const foreground=luminance(getComputedStyle(node).color);return (Math.max(foreground,background)+.05)/(Math.min(foreground,background)+.05);});
  });
  assert(messageContrasts.length&&messageContrasts.every(ratio=>ratio>=4.5),`Selected message small text contrast: ${messageContrasts}`);
  assert.equal(await page.locator('.message-row.selected').evaluate(el=>el===document.activeElement),true,'Reading a message retains row focus');
  await page.keyboard.press('Escape');
  checks.push('Messages: real fixture publication, accessible filter selection and keyboard focus, selected-row contrast and focus after marking read');
  for (let index = 0; index < 7; index++) {
    const saved = await call('change_catalog', { revision: (await call('server_catalog')).revision, change: { op: 'saveServer', draft: { name: '标题栏溢出验收-' + '长标题'.repeat(8) + index, host: `overflow-${index}.invalid`, port: 22, username: 'ops', authentication: { type: 'password', secret: { action: 'clear' } }, route: [] } } });
    await call('workspace_fixture_open', { serverId: saved.selectedId, connectionId: `titlebar-${index}`, endpoint: `ops@overflow-${index}.invalid:22` });
  }
  await page.reload();
  await page.waitForFunction(() => document.querySelectorAll('.server-tab').length === 8);
  assert(await page.locator('#server-tabs').evaluate(el => el.scrollWidth > el.clientWidth), 'Long server tabs overflow');
  await checkTitlebarPosition('overflow');
  await plus.click(); await picker.waitFor({ state: 'visible' }); await page.keyboard.press('Escape');
  assert.equal(await plus.evaluate(el => el === document.activeElement), true);
  checks.push('Title-bar plus: adjacent and vertically aligned in empty, single and overflowing long-tab states, three themes, 1100/720 widths and macOS inset; overflow picker remains clickable');
  await call("workspace_move", { window: "main", id: workspace.context.workspaceId, target: "workspace-titlebar", index: 0 });
  testWindow = "workspace-titlebar";
  await page.evaluate(() => sessionStorage.setItem("test-window", "workspace-titlebar"));
  await page.reload();
  await board.waitFor();
  await checkTitlebarPosition('popout');
  await checkTitlebar();
  checks.push("Pop-out title bar: same maximize/restore behavior; server tabs and their controls retain their own interactions");
  }
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ passed: checks.length, checks, pageErrors: errors }, null, 2));
} catch (error) {
  await finishTrace(true);
  await page.screenshot({ path: "artifacts/production-ui/failure.png" });
  if (await dialog().count()) console.error(await dialog().evaluate(el => [...el.querySelectorAll("form,fieldset,header,footer")].map(node => ({ tag: node.tagName, css: node.className, height: node.getBoundingClientRect().height, scroll: node.scrollHeight, client: node.clientHeight, min: getComputedStyle(node).minHeight, flex: getComputedStyle(node).flex }))));
  throw error;
} finally {
  await finishTrace(false);
  releaseLocalNetwork?.();
  holdCatalog = false; releaseCatalog?.();
  await browser.close(); await new Promise(resolve => server.httpServer.close(resolve));
  if (child.exitCode === null) { const exited = new Promise(resolve => child.once("exit", resolve)); child.stdin.end(); await exited; }
  await new Promise(resolve => downloads.close(resolve));
  fs.rmSync(directory, { recursive: true, force: true });
}
