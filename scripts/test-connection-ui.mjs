// Real DOM -> actual core: ephemeral credentials, connect progress, cancellation and retry.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import net from "node:net";
import readline from "node:readline";
import { createServer } from "vite";
import { chromium } from "playwright";

const build = spawnSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--bin", "catalog-fixture", "--locked"], { stdio: "inherit", windowsHide: true });
if (build.status !== 0) throw new Error("Core test bridge build failed");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-connection-ui-"));
const child = spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform === "win32" ? ".exe" : ""}`), [directory], { stdio: ["pipe", "pipe", "inherit"], windowsHide: true });
const queue = [];
readline.createInterface({ input: child.stdout }).on("line", line => { const response = JSON.parse(line), request = queue.shift(); if (response.error) request.reject(new Error(response.error)); else request.resolve(response.value); });
const call = (command, args = {}) => new Promise((resolve, reject) => { queue.push({ resolve, reject }); child.stdin.write(JSON.stringify({ command, args }) + "\n"); });
child.on("exit", () => { for (const request of queue.splice(0)) request.reject(new Error("Core bridge exited")); });
const sockets = new Set();
const server = net.createServer(socket => { sockets.add(socket); socket.on("data", () => {}); socket.on("error", () => {}); socket.on("close", () => sockets.delete(socket)); });
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const vite = await createServer({ server: { host: "127.0.0.1", port: 0 } }); await vite.listen(); const url = vite.resolvedUrls.local[0];
const browser = await chromium.launch({ headless: true }); const page = await browser.newPage({ viewport: { width: 1100, height: 720 } });
const errors = []; page.on("pageerror", error => errors.push(String(error)));
await page.exposeBinding("testInvoke", (_, command, args) => call(command, args));
await page.addInitScript(() => { window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.testInvoke(command, args) }; });
await page.route(url, route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><head><meta charset="UTF-8"></head><body><main class="workspace"></main><script type="module">import '/style.css';import {ServerManager} from '/servers.ts';import {ConnectionsUi} from '/connections.ts';const manager=new ServerManager(document.querySelector('main'),console.error);window.connections=new ConnectionsUi('main',v=>manager.setConnections(v),console.error);await manager.start();window.testReady=true;</script></body></html>` }));
async function settle(predicate) { for (let i = 0; i < 100; i++) { const v = await call("connection", { command: { op: "snapshot" } }); if (predicate(v)) { await page.evaluate(() => window.connections.refresh()); return v; } await new Promise(resolve => setTimeout(resolve, 25)); } throw new Error("Connection state did not settle"); }
try {
  await call("change_catalog", { revision: 0, change: { op: "saveServer", draft: { name: "隔离测试服务器", host: "127.0.0.1", port: server.address().port, username: "ops", authentication: { type: "password", secret: { action: "clear" } }, route: [] } } });
  await page.goto(url); await page.waitForFunction(() => window.testReady === true); await page.locator(".catalog-row").click();
  await page.locator("[data-connect-server]").click(); await settle(v => v[0]?.question?.kind === "credentials");
  const dialog = page.getByRole("dialog", { name: "补充连接凭据" }); await dialog.waitFor({ state: "visible" });
  fs.mkdirSync("artifacts/connection-ui", { recursive: true }); await page.screenshot({ path: "artifacts/connection-ui/credentials.png" });
  await dialog.locator('[name="secret"]').fill("ui-ephemeral-secret"); await dialog.locator("[data-prompt-accept]").click();
  await settle(v => v[0]?.stage === "sshHandshake"); await dialog.waitFor({ state: "hidden" });
  assert.equal(sockets.size, 1); assert.match(await page.locator(".connection-card").textContent(), /SSH 握手/);
  await page.screenshot({ path: "artifacts/connection-ui/progress.png" });
  assert(!JSON.stringify(await call("connection", { command: { op: "snapshot" } })).includes("ui-ephemeral-secret"));
  await page.locator("[data-connection-cancel]").click(); await settle(v => v[0]?.state === "cancelled");
  for (let i = 0; sockets.size && i < 100; i++) await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(sockets.size, 0); assert.equal((await call("workspace_snapshot", { window: "main" })).workspaces.length, 0);
  assert(!fs.readFileSync(path.join(directory, "servers.json"), "utf8").includes("ui-ephemeral-secret"));
  await page.locator("[data-connect-server]").click(); await settle(v => v[0]?.question?.kind === "credentials"); await dialog.waitFor({ state: "visible" });
  assert.equal(await dialog.locator('[name="secret"]').inputValue(), ""); await page.keyboard.press("Escape"); await settle(v => v[0]?.state === "cancelled");
  await dialog.waitFor({ state: "hidden" }); assert.deepEqual(errors, []);
  console.log("Connection UI: credentials, progress, cancellation, retry, Escape and redaction passed; no page errors.");
} finally {
  await browser.close(); await vite.close();
  for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve));
  if (child.exitCode === null) { const exited = new Promise(resolve => child.once("exit", resolve)); child.stdin.end(); await exited; }
  if (path.dirname(path.resolve(directory)) !== path.resolve(os.tmpdir()) || !path.basename(directory).startsWith("sodeck-connection-ui-")) throw new Error("Unexpected test cleanup path");
  fs.rmSync(directory, { recursive: true, force: true });
}
