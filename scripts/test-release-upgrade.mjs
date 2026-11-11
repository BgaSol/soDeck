// Real old/new release bytes; native updater install and persistent plugin activation.
// The companion driver is a test fixture, never a shipped application.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { upgradeInputs } from "./upgrade-inputs.mjs";
import { writeVerifiedDownload } from "./native-github.mjs";

export async function download(asset, directory, request = fetch) {
  assert(asset && /^sha256:[0-9a-f]{64}$/.test(asset.digest), "Release asset must have a GitHub SHA-256 digest");
  const filename = path.join(directory, asset.name);
  assert.equal(path.basename(filename), asset.name);
  const signal = AbortSignal.timeout(120_000);
  const response = await request(asset.browser_download_url, { signal });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(`Download ${asset.name}: ${response.status}`);
  }
  await writeVerifiedDownload(response, filename, { size: asset.size, sha256: asset.digest.slice(7) }, { signal });
  return filename;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
const { target, component, from, to, current, version, verifyDiscovery } = upgradeInputs(process.env);
assert.equal(target, `${process.arch === "arm64" ? "aarch64" : "x86_64"}-${{ win32: "pc-windows-msvc", linux: "unknown-linux-gnu", darwin: "apple-darwin" }[process.platform]}`);
const output = path.resolve("artifacts/release-upgrade", target);
assert(!fs.existsSync(output), "Use a clean output directory for a new verification run");
fs.mkdirSync(output, { recursive: true });
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-release-upgrade-")));
const data = path.join(root, "data");
const suffix = process.platform === "win32" ? ".exe" : "";
const driver = path.resolve(`src-tauri/target/debug/release-upgrade-fixture${suffix}`);
const plugins = component === "core" ? [] : [{ name: component, id: `sodeck.${component}`, current: from, version: to }];
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
  const response = await fetch(`https://api.github.com/repos/BgaSol/soDeck/releases/tags/${tag}`, { signal: AbortSignal.timeout(120_000), headers: { "User-Agent": "SoDeck-native-upgrade-test", ...(process.env.GH_TOKEN ? { Authorization: `Bearer ${process.env.GH_TOKEN}` } : {}) } });
  assert(response.ok, `Release ${tag}: ${response.status}`);
  const result = await response.json();
  assert.equal(result.draft, false); assert.equal(result.prerelease, false); assert.equal(result.tag_name, tag);
  return result;
}
function asset(release, name) {
  const matches = release.assets.filter(a => a.name === name);
  assert.equal(matches.length, 1, `Missing/duplicate asset ${name}`);
  return matches[0];
}
function smoke(executable, preserved = false, directory = data) {
  native(process.execPath, ["scripts/smoke.mjs", executable], { SODECK_TEST_DATA_DIR: directory, SODECK_EXPECT_CORE_VERSION: preserved ? version : current, SODECK_EXPECT_TARGET: target, SODECK_VERIFY_PERSISTED: preserved ? "1" : "0" });
}
const report = { target, component, current, version, from, to, completed: false, fixtureDriven: true, inputs: [] };
try {
  const old = await release(`v${current}`);
  const next = await release(component === "core" ? `v${to}` : `plugin-${component}-v${to}`);
  const manifestAsset = asset(next, component === "core" ? "core-update.json" : "plugin-update.json");
  const manifest = JSON.parse(fs.readFileSync(await download(manifestAsset, root), "utf8"));
  assert.equal(manifest.id, component === "core" ? "sodeck.core" : `sodeck.${component}`);
  assert.equal(manifest.version, to); assert(manifest.platforms[target], "Missing target");
  manifest.manifestUrl = manifestAsset.browser_download_url;
  manifest.source = { repository: "BgaSol/soDeck", tagPrefix: component === "core" ? "v" : `plugin-${component}-v` };
  const updateAsset = next.assets.find(a => a.browser_download_url === manifest.platforms[target].url);
  assert(updateAsset, "Manifest must refer to this exact Release");
  const nextPackage = await download(updateAsset, path.join(root, "new"));
  if (component !== "core") assert.equal(digest(nextPackage), manifest.platforms[target].sha256);
  report.inputs.push({ releaseId: next.id, tag: next.tag_name, manifest: manifestAsset, package: updateAsset });

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
  report.inputs.push({ releaseId: old.id, tag: old.tag_name, assets: old.assets });
  for (const plugin of plugins) {
    const released = await release(`plugin-${plugin.name}-v${plugin.current}`);
    report.inputs.push({ releaseId: released.id, tag: released.tag_name, assets: released.assets });
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
  fs.writeFileSync(input, JSON.stringify({ root: data, executable, current, version, plugins, corePackage: component === "core" ? nextPackage : null, manifests: [manifest] }));
  run(driver, ["seed", input]);
  if (verifyDiscovery) {
    run(driver, ["discover", input]);
    report.discovered = JSON.parse(fs.readFileSync(path.join(data, "upgrade-discovered.json"), "utf8"));
    assert.equal(report.discovered.completed, true);
  }
  native(driver, ["install", input]);
  assert(!fs.existsSync(path.join(data, "upgrade-error.txt")), fs.existsSync(path.join(data, "upgrade-error.txt")) ? fs.readFileSync(path.join(data, "upgrade-error.txt"), "utf8") : "");
  let expectedDigest = digest(executable);
  if (component === "core") {
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
  expectedDigest = digest(expected);
  const deadline = Date.now() + 180_000;
  for (;;) {
    try { if (digest(executable) === expectedDigest) break; } catch {}
    assert(Date.now() < deadline, "Native installer did not replace the old program with the verified release bytes");
    await delay(1000);
  }
  await delay(2000);
  smoke(executable, true);
  }
  run(driver, ["activate", input]);
  if (process.platform === "linux" && component === "core") {
    const latest = next;
    const newDeb = await download(asset(latest, `SoDeck_${version}_${architecture === "aarch64" ? "arm64" : "amd64"}.deb`), root);
    const debData = path.join(root, "deb-data");
    run("sudo", ["dpkg", "-i", oldDeb]);
    smoke("/usr/bin/sodeck", false, debData);
    run("sudo", ["dpkg", "-i", newDeb]);
    smoke("/usr/bin/sodeck", true, debData);
    report.debSystemUpgrade = true;
  }
  report.coreDigest = expectedDigest;
  report.baseline = JSON.parse(fs.readFileSync(path.join(data, "upgrade-baseline.json"), "utf8"));
  report.prepared = JSON.parse(fs.readFileSync(path.join(data, "upgrade-prepared.json"), "utf8"));
  report.activated = JSON.parse(fs.readFileSync(path.join(data, "upgrade-activated.json"), "utf8"));
  report.completed = true;
} catch (error) {
  report.error = error.stack || String(error);
  throw error;
} finally {
  try {
    if (fs.existsSync(path.join(data, "logs"))) fs.cpSync(path.join(data, "logs"), path.join(output, "logs"), { recursive: true });
    assert.equal(path.dirname(root), fs.realpathSync(os.tmpdir()));
    // WebView2 and the native installer can release file handles after process exit.
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
    report.cleanedUp = true;
  } catch (error) {
    report.completed = false;
    report.cleanupError = String(error.stack ?? error);
    throw error;
  } finally {
    fs.writeFileSync(path.join(output, "result.json"), JSON.stringify(report, null, 2));
  }
}
}
