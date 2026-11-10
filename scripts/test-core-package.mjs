import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { validateCoreArtifacts, coreNames } from "./core-artifacts.mjs";
import { verifyBinary } from "./verify-binary.mjs";

const [input, target, commit] = process.argv.slice(2);
assert(input && target && commit, "Provide package directory, native target and source commit");
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-core-verify-"));
const metadata = JSON.parse(fs.readFileSync(path.join(input, target, "core-artifact.json"), "utf8"));
const report = { ...metadata, toolCommit: process.env.GITHUB_SHA, passed: false };
function run(command, args, env = {}) {
  const result = spawnSync(command, args, { windowsHide: true, stdio: "inherit", timeout: 180000, env: { ...process.env, ...env } });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} failed: ${result.status}/${result.signal}`);
}
try {
  assert.equal(target, `${process.arch === "arm64" ? "aarch64" : "x86_64"}-${{ win32: "pc-windows-msvc", linux: "unknown-linux-gnu", darwin: "apple-darwin" }[process.platform]}`);
  assert.equal(metadata.target, target);
  assert.deepEqual(validateCoreArtifacts(input, metadata, commit).present, [target]);
  const names = coreNames(metadata.version, target), file = name => path.resolve(input, target, name);
  const bundle = file(target.includes("windows") ? names.nsis[0] : target.includes("linux") ? names.appimage[0] : names.macos[0]);
  const env = { SODECK_PACKAGE_ONLY: "1", SODECK_EXPECT_TARGET: target, SODECK_EXPECT_CORE_VERSION: metadata.version, SODECK_TEST_UPDATE_BUNDLE: bundle, WEBKIT_DISABLE_DMABUF_RENDERER: "1", APPIMAGE_EXTRACT_AND_RUN: "1" };
  function smoke(executable, appimage = false) {
    if (!appimage) verifyBinary(executable, target);
    const launch = process.platform === "linux" ? ["dbus-run-session", "--", "xvfb-run", "-a", executable] : [executable];
    run(process.execPath, ["scripts/smoke.mjs", ...launch], env);
  }
  if (process.platform === "win32") {
    run(bundle, ["/S"]);
    const install = path.join(process.env.ProgramW6432 ?? process.env.ProgramFiles, "SoDeck");
    const registration = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SoDeck').InstallLocation"], { encoding: "utf8", windowsHide: true });
    assert.equal(registration.status, 0); assert.equal(registration.stdout.trim().replaceAll('"', ''), install);
    smoke(path.join(install, "sodeck.exe"));
  } else if (process.platform === "linux") {
    run("dpkg-deb", ["-x", file(names.deb[0]), directory]);
    smoke(path.join(directory, "usr/bin/sodeck"));
    fs.chmodSync(bundle, 0o755); smoke(bundle, true);
  } else {
    const mount = path.join(directory, "mounted");
    run("hdiutil", ["attach", file(names.dmg[0]), "-nobrowse", "-readonly", "-mountpoint", mount]);
    try { smoke(path.join(mount, "SoDeck.app/Contents/MacOS/sodeck")); }
    finally { run("hdiutil", ["detach", mount]); }
    const update = path.join(directory, "update"); fs.mkdirSync(update);
    run("tar", ["-xzf", bundle, "-C", update]);
    smoke(path.join(update, "SoDeck.app/Contents/MacOS/sodeck"));
  }
  validateCoreArtifacts(input, metadata, commit);
  report.passed = true;
} catch (error) { report.error = String(error.stack ?? error); throw error; }
finally {
  fs.mkdirSync("artifacts/verification", { recursive: true });
  fs.writeFileSync(`artifacts/verification/core-${target}.json`, JSON.stringify(report, null, 2) + "\n");
  assert.equal(path.dirname(directory), os.tmpdir()); fs.rmSync(directory, { recursive: true, force: true });
}
