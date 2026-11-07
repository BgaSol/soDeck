// Real old/new release bytes; native updater install and persistent plugin activation.
// The companion driver is a test fixture, never a shipped application.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

const target = process.env.SODECK_UPGRADE_TARGET;
assert(target, "Set SODECK_UPGRADE_TARGET to the native Rust target");
const output = path.resolve("artifacts/release-upgrade", target);
assert(!fs.existsSync(output), "Use a clean output directory for a new verification run");
fs.mkdirSync(output, { recursive: true });
const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-release-upgrade-"));
const data = path.join(root, "data");
const suffix = process.platform === "win32" ? ".exe" : "";
const driver = path.resolve(`src-tauri/target/debug/release-upgrade-fixture${suffix}`);
const current = "0.9.2", version = "0.9.3";
const plugins = [
  { name: "terminal", id: "sodeck.terminal", current: "0.2.1", version: "0.2.2" },
  { name: "files", id: "sodeck.files", current: "0.4.1", version: "0.4.2" },
  { name: "network-proxy", id: "sodeck.network-proxy", current: "0.2.1", version: "0.2.2" },
];
const digest = filename => createHash("sha256").update(fs.readFileSync(filename)).digest("hex");
function run(command, args, env = {}) {
  const result = spawnSync(command, args, { windowsHide: true, stdio: "inherit", timeout: 600_000, env: { ...process.env, ...env } });
  assert.equal(result.status, 0, result.error?.message || `${command} failed: ${result.status}`);
}
function native(command, args, env = {}) {
  if (process.platform === "linux") run("dbus-run-session", ["--", "xvfb-run", "-a", command, ...args], { APPIMAGE_EXTRACT_AND_RUN: "1", WEBKIT_DISABLE_DMABUF_RENDERER: "1", ...env });
  else run(command, args, env);
}
async function release(tag) {
  const response = await fetch(`https://api.github.com/repos/BgaSol/soDeck/releases/tags/${tag}`, { headers: { "User-Agent": "SoDeck-native-upgrade-test", ...(process.env.GH_TOKEN ? { Authorization: `Bearer ${process.env.GH_TOKEN}` } : {}) } });
  assert(response.ok, `Release ${tag}: ${response.status}`);
  const result = await response.json();
  assert.equal(result.draft, false); assert.equal(result.prerelease, false); assert.equal(result.tag_name, tag);
  return result;
}
async function download(asset, directory) {
  assert(asset && /^sha256:[0-9a-f]{64}$/.test(asset.digest), "Release asset must have a GitHub SHA-256 digest");
  fs.mkdirSync(directory, { recursive: true });
  const filename = path.join(directory, asset.name);
  assert.equal(path.basename(filename), asset.name);
  const response = await fetch(asset.browser_download_url);
  assert(response.ok, `Download ${asset.name}: ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  assert.equal(bytes.length, asset.size); fs.writeFileSync(filename, bytes);
  assert.equal(digest(filename), asset.digest.slice(7));
  return filename;
}
function asset(release, name) {
  const matches = release.assets.filter(a => a.name === name);
  assert.equal(matches.length, 1, `Missing/duplicate asset ${name}`);
  return matches[0];
}
function smoke(executable, preserved = false, directory = data) {
  native(process.execPath, ["scripts/smoke.mjs", executable], { SODECK_TEST_DATA_DIR: directory, SODECK_EXPECT_CORE_VERSION: preserved ? version : current, SODECK_EXPECT_TARGET: target, SODECK_VERIFY_PERSISTED: preserved ? "1" : "0" });
}
const report = { target, current, version, completed: false, fixtureDriven: true };
try {
  const old = await release(`v${current}`);
  const architecture = target.startsWith("aarch64") ? "aarch64" : "amd64";
  let executable, oldDeb;
  if (process.platform === "win32") {
    const filename = `SoDeck_${current}_${architecture === "aarch64" ? "arm64" : "x64"}-setup.exe`;
    const installer = await download(asset(old, filename), root);
    const directory = path.join(root, "program");
    run(installer, ["/S", `/D=${directory}`]);
    executable = path.join(directory, "sodeck.exe");
  } else if (process.platform === "linux") {
    executable = await download(asset(old, `SoDeck_${current}_${architecture}.AppImage`), root);
    fs.chmodSync(executable, 0o755);
    oldDeb = await download(asset(old, `SoDeck_${current}_${architecture === "aarch64" ? "arm64" : "amd64"}.deb`), root);
  } else {
    const dmg = await download(asset(old, `SoDeck_${current}_aarch64.dmg`), root);
    const mount = path.join(root, "mounted");
    run("hdiutil", ["attach", dmg, "-nobrowse", "-readonly", "-mountpoint", mount]);
    try { run("ditto", [path.join(mount, "SoDeck.app"), path.join(root, "SoDeck.app")]); }
    finally { run("hdiutil", ["detach", mount]); }
    executable = path.join(root, "SoDeck.app/Contents/MacOS/sodeck");
  }
  for (const plugin of plugins) {
    const released = await release(`plugin-${plugin.name}-v${plugin.current}`);
    const archive = await download(asset(released, `sodeck-${plugin.name}_${plugin.current}_${target}.${process.platform === "win32" ? "zip" : "tar.gz"}`), root);
    const extracted = path.join(root, "plugins"); fs.mkdirSync(extracted, { recursive: true });
    run("tar", ["-xf", archive, "-C", extracted]);
    plugin.directory = path.join(extracted, `sodeck-${plugin.name}_${plugin.current}_${target}`);
    const manifest = JSON.parse(fs.readFileSync(path.join(plugin.directory, "plugin.json"), "utf8"));
    assert.equal(manifest.id, plugin.id); assert.equal(manifest.version, plugin.current); assert.equal(manifest.target, target);
    run(process.execPath, ["scripts/verify-binary.mjs", path.join(plugin.directory, manifest.entry), target]);
  }
  run(process.execPath, ["scripts/verify-binary.mjs", executable, target]);
  smoke(executable);
  const input = path.join(root, "input.json");
  fs.writeFileSync(input, JSON.stringify({ root: data, executable, current, version, plugins }));
  run(driver, ["seed", input]);
  native(driver, ["install", input]);
  assert(!fs.existsSync(path.join(data, "upgrade-error.txt")), fs.existsSync(path.join(data, "upgrade-error.txt")) ? fs.readFileSync(path.join(data, "upgrade-error.txt"), "utf8") : "");
  const expectedRoot = path.join(root, "expected"); fs.mkdirSync(expectedRoot);
  const downloaded = path.join(data, "core-download.bin");
  let expected;
  if (process.platform === "win32") {
    run("7z", ["x", downloaded, `-o${expectedRoot}`, "-y"]); expected = path.join(expectedRoot, "sodeck.exe");
  } else if (process.platform === "linux") expected = downloaded;
  else {
    run("tar", ["-xzf", downloaded, "-C", expectedRoot]);
    expected = path.join(expectedRoot, "SoDeck.app/Contents/MacOS/sodeck");
  }
  const expectedDigest = digest(expected), deadline = Date.now() + 180_000;
  for (;;) {
    try { if (digest(executable) === expectedDigest) break; } catch {}
    assert(Date.now() < deadline, "Native installer did not replace the old program with the verified release bytes");
    await delay(1000);
  }
  await delay(2000);
  smoke(executable, true);
  run(driver, ["activate", input]);
  if (process.platform === "linux") {
    const latest = await release(`v${version}`);
    const newDeb = await download(asset(latest, `SoDeck_${version}_${architecture === "aarch64" ? "arm64" : "amd64"}.deb`), root);
    const debData = path.join(root, "deb-data");
    run("sudo", ["dpkg", "-i", oldDeb]);
    smoke("/usr/bin/sodeck", false, debData);
    run("sudo", ["dpkg", "-i", newDeb]);
    smoke("/usr/bin/sodeck", true, debData);
    report.debSystemUpgrade = true;
  }
  const installed = JSON.parse(fs.readFileSync(path.join(data, "plugins.json"), "utf8"));
  const businessData = path.join(root, "business-data"); fs.cpSync(data, businessData, { recursive: true });
  const businessEnv = { SODECK_TEST_DATA_DIR: businessData, SODECK_EXPECT_CORE_VERSION: version, SODECK_EXPECT_TARGET: target, SODECK_VERIFY_PERSISTED: "0" };
  for (const plugin of plugins) businessEnv[`SODECK_TEST_${plugin.name === "network-proxy" ? "NETWORK" : plugin.name.toUpperCase()}_PLUGIN`] = installed.find(p => p.manifest.id === plugin.id).directory;
  native(process.execPath, ["scripts/test-openssh.mjs", "--packaged", executable], businessEnv);
  report.coreDigest = expectedDigest;
  report.baseline = JSON.parse(fs.readFileSync(path.join(data, "upgrade-baseline.json"), "utf8"));
  report.prepared = JSON.parse(fs.readFileSync(path.join(data, "upgrade-prepared.json"), "utf8"));
  report.activated = JSON.parse(fs.readFileSync(path.join(data, "upgrade-activated.json"), "utf8"));
  report.business = JSON.parse(fs.readFileSync(path.join(businessData, "smoke-result.json"), "utf8"));
  report.completed = true;
} catch (error) {
  report.error = error.stack || String(error);
  throw error;
} finally {
  fs.writeFileSync(path.join(output, "result.json"), JSON.stringify(report, null, 2));
  if (fs.existsSync(path.join(data, "logs"))) fs.cpSync(path.join(data, "logs"), path.join(output, "logs"), { recursive: true });
  // root was created by this script, never provided by a caller.
  fs.rmSync(root, { recursive: true, force: true });
}
