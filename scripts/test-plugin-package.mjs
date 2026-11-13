import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { validatePluginArchive } from "./prepare-native-packages.mjs";
import { pluginPeer } from "./plugin-peer.mjs";
import { sha256 } from "./core-artifacts.mjs";

const [input, target, commit, plugin] = process.argv.slice(2);
assert(input && target && /^[a-f0-9]{40}$/.test(commit ?? ""), "Provide input, native target, source commit and plugin");
assert(["terminal", "files", "network-proxy", "system-status", "docker"].includes(plugin));
assert.equal(target, `${process.arch === "arm64" ? "aarch64" : "x86_64"}-${{ win32: "pc-windows-msvc", linux: "unknown-linux-gnu", darwin: "apple-darwin" }[process.platform]}`);
const metadata = JSON.parse(fs.readFileSync(path.join(input, `plugin-${target}.json`), "utf8"));
assert.equal(metadata.id, `sodeck.${plugin}`); assert.equal(metadata.commit, commit);
assert.equal(path.basename(metadata.file), metadata.file);
assert.equal(metadata.size, fs.statSync(path.join(input, metadata.file)).size);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-package-verify-"));
const report = { id: metadata.id, version: metadata.version, target, commit, sha256: metadata.sha256, runId: metadata.runId, toolCommit: process.env.GITHUB_SHA, passed: false };
let peer;
try {
  for (const file of [metadata.file, `plugin-${target}.json`]) fs.copyFileSync(path.join(input, file), path.join(directory, file));
  const checked = validatePluginArchive(directory, { ...metadata, binary: `sodeck-${plugin}` }, target);
  const manifest = JSON.parse(fs.readFileSync(path.join(checked.root, "plugin.json"), "utf8"));
  assert.equal(manifest.apiVersion, "1"); assert.equal(manifest.ui.apiVersion, "2");
  for (const file of ["index.html", "shared.css", "text-menu.js", "clipboard-bridge.js"]) assert(fs.statSync(path.join(checked.root, "ui", file)).size > 0, file);
  for (const view of manifest.ui.views) assert(fs.statSync(path.join(checked.root, view.entry ?? manifest.ui.entry ?? "ui/index.html")).size > 0);
  if (["terminal", "files"].includes(plugin)) {
    const fonts = path.join(checked.root, "ui/fonts");
    const sources = JSON.parse(fs.readFileSync(path.join(fonts, "sources.json"), "utf8"));
    assert(sources.files.length > 0);
    for (const item of sources.files) {
      assert.equal(path.basename(item.file), item.file);
      assert.equal(fs.statSync(path.join(fonts, item.file)).size, item.bytes);
      assert.equal(sha256(path.join(fonts, item.file)), item.sha256);
    }
  }
  if (plugin === "terminal") for (const file of ["history-worker.js", "vendor/xterm.js", "vendor/xterm.js.LICENSE", "vendor/serialize.js", "vendor/serialize.js.LICENSE"]) assert(fs.statSync(path.join(checked.root, "ui", file)).size > 0);
  if (plugin === "network-proxy") for (const file of ["dom-update.js", "licenses/morphdom.LICENSE"]) assert(fs.statSync(path.join(checked.root, "ui", file)).size > 0);
  if (plugin === "files") {
    const vendor = path.join(checked.root, "ui/file-editor");
    for (const file of ["editor.js", "index.js", "open.js", "editor.css", "monaco.LICENSE", "ThirdPartyNotices.txt", "dompurify.LICENSE", "marked.LICENSE.md"]) assert(fs.statSync(path.join(vendor, file)).size > 0);
    assert(fs.readdirSync(vendor, { recursive: true }).some(file => /worker.*\.js$/.test(file)), "Editor worker missing");
  }
  if (plugin === "docker") {
    assert.equal(manifest.ui.views.length, 3);
    const browser = manifest.ui.views.find(view => view.id === "browser");
    assert.equal(browser.defaultSide, "right"); assert.equal(browser.defaultPinned, true);
    assert.equal(browser.openOnWorkspace, "none"); assert.equal(browser.icon, "docker");
    for (const file of ["style.css", "bridge.js", "view.js"]) assert(fs.statSync(path.join(checked.root, "ui", file)).size > 0);
  }
  if (plugin === "system-status") {
    assert.equal(manifest.ui.views[0].backgroundOnWorkspace, true);
    for (const file of ["view.js", "charts.js", "style.css"]) assert(fs.statSync(path.join(checked.root, "ui", file)).size > 0);
  }
  const unexpected = [], calls = [], resources = new Set(), context = { workspaceId: "package", serverId: "server", connectionId: "connection", generation: 1 };
  peer = pluginPeer(path.join(checked.root, manifest.entry), async (method, params) => {
    calls.push(method);
    if (method === "settings.get") return { values: { dynamicIntervalSeconds: "5", refreshSeconds: "30", logTail: "200", directoryMode: "off" } };
    if (method === "session.state") return { context, state: "disconnected" };
    if (["logging.write", "messages.publish", "ui.updateView"].includes(method)) return {};
    if (method === "terminal.open" || (method === "files.request" && params.action === "open")) { resources.add("fixture"); return { id: "fixture" }; }
    if (method === "terminal.close" || (method === "files.request" && params.action === "close")) { assert(resources.delete(params.id)); return {}; }
    unexpected.push(method); throw new Error(`Unexpected package RPC: ${method}`);
  });
  const capabilities = ["commands.v1", "terminal.v1", "viewData.v1", "terminal.history.v1", "files.v1", "files.directoryCursors.v1", "files.operations.v1", "files.operations.v2", "files.editor.v1", "files.localInputs.v1", "net.tcp.v1", "net.listenerLifecycle.v1", "ui.dialog.v1", "session.exec.v1", "ui.background.v1", "settings.v1", "ui.workbench.v2", "ui.openView.v1"];
  const initialized = await peer.request("initialize", { apiVersion: "1", capabilities, dataDirectory: path.join(directory, "data") });
  assert.equal(initialized.apiVersion, "1");
  if (manifest.commands) {
    assert.equal(initialized.commandsApiVersion, manifest.commands.apiVersion);
    assert.deepEqual([...initialized.commandHandlers].sort(), [...manifest.commands.handlers].sort());
  }
  const viewType = { terminal: "terminal", files: "files", "network-proxy": "manager", "system-status": "overview", docker: "browser" }[plugin];
  const view = { viewId: "package-view", viewType, context };
  await peer.request("ui.createView", view);
  await peer.request("ui.getView", view);
  if (plugin === "system-status") { await peer.request("ui.event", { ...view, data: { action: "pause" } }); assert.equal((await peer.request("ui.event", { ...view, data: { action: "snapshot" } })).data.paused, true); }
  await peer.request("ui.disposeView", view);
  assert.deepEqual(unexpected, []);
  assert.equal(resources.size, 0, "View resources must be released");
  await peer.close(); peer = null;
  assert.equal(sha256(path.join(input, metadata.file)), metadata.sha256);
  Object.assign(report, { passed: true, calls });
  console.log(`${plugin}: actual ${target} archive initialized, mounted, disposed and exited`);
} catch (error) { report.error = String(error.stack ?? error); throw error; }
finally {
  if (peer) await peer.kill();
  fs.mkdirSync("artifacts/verification", { recursive: true });
  fs.writeFileSync(`artifacts/verification/${plugin}-${target}.json`, JSON.stringify(report, null, 2) + "\n");
  assert.equal(path.dirname(directory), os.tmpdir()); fs.rmSync(directory, { recursive: true, force: true });
}
