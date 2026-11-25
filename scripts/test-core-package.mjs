import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { validateCoreArtifacts, coreNames, cleanupWindowsUninstaller } from "./core-artifacts.mjs";
import { verifyBinary } from "./verify-binary.mjs";

const [input, target, commit] = process.argv.slice(2);
assert(input && target && commit, "Provide package directory, native target and source commit");
const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-core-verify-")));
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
  const env = { SODECK_PACKAGE_ONLY: "1", SODECK_TEST_LOGIN_RESTART:"1", SODECK_REQUIRE_DESKTOP_SHELL: "1", SODECK_EXPECT_TARGET: target, SODECK_EXPECT_CORE_VERSION: metadata.version, SODECK_TEST_UPDATE_BUNDLE: bundle, WEBKIT_DISABLE_DMABUF_RENDERER: "1", APPIMAGE_EXTRACT_AND_RUN: "1" };
  function smoke(executable, appimage = false) {
    if (!appimage) verifyBinary(executable, target);
    const launch = process.platform === "linux" ? ["dbus-run-session", "--", "xvfb-run", "-a", "sh", "scripts/linux-package-desktop.sh", executable] : [executable];
    run(process.execPath, ["scripts/smoke.mjs", ...launch], env);
  }
  if (process.platform === "win32") {
    const modes = [
      { mode: "CurrentUser", hive: "HKCU", install: path.join(process.env.LOCALAPPDATA, "Programs", "SoDeck") },
      { mode: "AllUsers", hive: "HKLM", install: path.join(process.env.ProgramW6432 ?? process.env.ProgramFiles, "SoDeck") },
    ];
    function registration(hive) {
      const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `$key = '${hive}:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SoDeck'; if (Test-Path -LiteralPath $key) { Get-ItemProperty -LiteralPath $key -ErrorAction Stop | ConvertTo-Json -Compress }`], { encoding: "utf8", windowsHide: true });
      assert.equal(result.status, 0, result.stderr);
      return result.stdout.trim() ? JSON.parse(result.stdout) : null;
    }
    for (const { hive, install } of modes) {
      assert.equal(registration(hive), null, "Use a clean Windows runner without an existing SoDeck installation");
      assert(!fs.existsSync(install), `Existing installation directory: ${install}`);
    }
    report.windowsInstallModes = [];
    for (const { mode, hive, install } of modes) {
      run(bundle, ["/S", `/${mode}`]);
      try {
        const entry = registration(hive);
        assert(entry, `Missing ${mode} installation registration`);
        assert.equal(entry.InstallLocation.replaceAll('"', ''), install);
        assert.equal(entry[mode], "1", `Missing ${mode} installation marker`);
        assert.equal(registration(hive === "HKCU" ? "HKLM" : "HKCU"), null);
        smoke(path.join(install, "sodeck.exe"));
      } finally {
        run(path.join(install, "uninstall.exe"), ["/S", `/${mode}`, `_?=${install}`]);
      }
      assert.equal(registration(hive), null, `${mode} uninstall left installation registration`);
      assert(!fs.existsSync(path.join(install, "sodeck.exe")), `${mode} uninstall left the application`);
      // NSIS cannot remove its own executable when run in place with _?=.
      cleanupWindowsUninstaller(install);
      report.windowsInstallModes.push({ mode, hive, install, passed: true });
    }
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
  report.pluginIconsNative = true;
  report.desktopShellNative = true;
  report.desktopStartupNative = true;
  report.loginRegistrationIoNative = true;
  report.loginRestartNative = true;
  if (target === 'aarch64-apple-darwin') report.macosApplicationPolicyNative = true;
} catch (error) { report.error = String(error.stack ?? error); throw error; }
finally {
  fs.mkdirSync("artifacts/verification", { recursive: true });
  fs.writeFileSync(`artifacts/verification/core-${target}.json`, JSON.stringify(report, null, 2) + "\n");
  assert.equal(path.dirname(directory), fs.realpathSync(os.tmpdir())); fs.rmSync(directory, { recursive: true, force: true });
}
