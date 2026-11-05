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

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--bin", "plugin-fixture", "--locked"], { stdio: "inherit" });
if (build.status !== 0) throw new Error("Catalog test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-production-ui-"));
const suffix = process.platform === "win32" ? ".exe" : "";
const target = spawnSync("rustc", ["-vV"], { encoding: "utf8" }).stdout.match(/^host: (.+)$/m)[1];
const fixture = path.join(directory, "fixture"); fs.mkdirSync(fixture);
fs.copyFileSync(`src-tauri/target/debug/plugin-fixture${suffix}`, path.join(fixture, `plugin-fixture${suffix}`));
if (process.platform !== "win32") fs.chmodSync(path.join(fixture, `plugin-fixture${suffix}`), 0o755);
fs.writeFileSync(path.join(fixture, "plugin.json"), JSON.stringify({ id: "sodeck.hello", name: "Loopback Hello", version: "0.1.1", apiVersion: "1", target, entry: `plugin-fixture${suffix}` }));
const archive = path.join(directory, "fixture.tar.gz");
const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32", "tar.exe") : "tar";
if (spawnSync(tar, ["-czf", archive, "-C", directory, "fixture"], { windowsHide: true, stdio: "inherit" }).status !== 0) throw new Error("Failed to package test fixture");
const archiveBytes = fs.readFileSync(archive), requests = [];
let published = false, corrupt = false;
const downloads = http.createServer((request, response) => {
  requests.push(request.url);
  const base = `http://127.0.0.1:${downloads.address().port}`;
  let data;
  if (request.url.startsWith("/repos/BgaSol/soDeck/releases")) data = JSON.stringify(published ? [{ tag_name: "plugin-hello-v0.1.1", draft: false, prerelease: false, assets: [{ name: "plugin-update.json", browser_download_url: `${base}/manifest.json` }] }] : []);
  else if (request.url === "/manifest.json") data = JSON.stringify({ id: "sodeck.hello", version: "0.1.1", apiVersion: "1", platforms: { [target]: { url: `${base}/fixture.tar.gz`, sha256: createHash("sha256").update(archiveBytes).digest("hex") } } });
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
const errors = [], checks = [];
page.on("pageerror", error => errors.push(String(error)));
await page.exposeBinding("desktopInvoke", (_, command, args) => call(command, { ...args, window: "main" }));
await page.addInitScript(() => {
  let id = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" } },
    transformCallback: () => ++id,
    invoke: (command, args) => {
      if (command === "plugin:event|listen") return Promise.resolve(++id);
      if (command === "plugin:event|unlisten" || command === "workspace_surface") return Promise.resolve();
      return window.desktopInvoke(command, args);
    },
  };
});
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

  const palettes = { light: "#f6f7f9", dark: "#191b22", dopamine: "#fbf1e8" };
  for (const [theme, canvas] of Object.entries(palettes)) {
    await call("set_theme", { theme }); await page.reload();
    await page.waitForFunction(theme => document.documentElement.dataset.theme === theme, theme);
    assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--canvas").trim()), canvas);
    for (const [width, height] of [[1100, 720], [720, 480]]) {
      await page.setViewportSize({ width, height });
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
        assert(await body.evaluate(el => el.scrollHeight > el.clientHeight), "Long form must have an internal scroll range");
        await body.evaluate(el => el.scrollTo(0, 0));
        const bounds = await body.boundingBox();
        await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + Math.min(60, bounds.height / 2));
        await page.mouse.wheel(0, 500);
        await page.waitForFunction(() => document.querySelector(".catalog-form-body").scrollTop > 0);
        await page.mouse.wheel(0, 10000);
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
  const hello = page.locator('[data-official-plugin="sodeck.hello"]');
  const install = () => hello.locator('[data-action="install-official-plugin"]');
  for(const id of ['sodeck.hello','sodeck.terminal','sodeck.files','sodeck.ui-example']) assert.equal(await page.locator(`[data-official-plugin="${id}"]`).count(),1);
  assert.match(await page.locator('[data-official-plugin="sodeck.terminal"]').textContent(), /真实终端/);
  assert.match(await page.locator('[data-official-plugin="sodeck.files"]').textContent(), /编辑 UTF-8 小文件/);
  assert.deepEqual(requests, [], "Opening settings must not download plugins automatically");
  await install().click(); await hello.getByRole("alert").waitFor();
  assert.match(await hello.getByRole("alert").textContent(), /尚未发布/);
  assert.equal((await call("snapshot")).plugins.length, 0);
  published = true; corrupt = true;
  await install().click(); await page.waitForFunction(() => document.querySelector('[data-official-plugin="sodeck.hello"] [role="alert"]')?.textContent.includes("SHA-256"));
  assert.equal((await call("snapshot")).plugins.length, 0);
  corrupt = false; await install().click();
  await hello.locator('[data-action="select-plugin"]').waitFor();
  assert.match(await hello.textContent(), /待重启/);
  assert.equal(await install().count(), 0);
  const installed = (await call("snapshot")).plugins[0];
  assert(installed.enabled && installed.pendingRestart);
  assert.equal(installed.status, "unloaded");
  assert.equal(installed.manifest.updateSource.repository, "BgaSol/soDeck");
  await page.reload(); await page.locator("#settings-button").click();
  await hello.locator('[data-action="select-plugin"]').waitFor();
  assert.equal((await call("snapshot")).plugins.length, 1);
  assert.match(await page.locator('[data-official-plugin="sodeck.ui-example"]').textContent(), /不执行远程业务/);
  await page.screenshot({ path: "artifacts/production-ui/official-plugins.png", animations: "disabled" });
  checks.push("Official plugin first install: no background downloads, unpublished release and corrupt archive retries, native package selection, pending restart and persisted record after reload");
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
