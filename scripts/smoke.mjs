import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const [command, ...args] = process.argv.slice(2);
if (!command)
  throw new Error("Provide the packaged application or its launcher");

const directory =
  process.env.SODECK_TEST_DATA_DIR ||
  fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-package-"));
function launch(persisted = false) {
  const report = path.join(directory, "smoke-result.json");
  fs.rmSync(report, { force: true });
  const failure = path.join(directory, "smoke-error.txt");
  fs.rmSync(failure, { force: true });
  const networkReport = path.join(directory, "network-smoke-result.json");
  fs.rmSync(networkReport, { force: true });
  const extra = ["--smoke-test", "--test-data-dir", directory];
  if (process.env.SODECK_TEST_CLIPBOARD === "1") extra.push("--clipboard-test");
  if (process.env.SODECK_TEST_DESKTOP_FILES) extra.push("--desktop-files-test", path.resolve(process.env.SODECK_TEST_DESKTOP_FILES));
  if (process.env.SODECK_TEST_SSH_CONFIG) extra.push("--ssh-test-config", path.resolve(process.env.SODECK_TEST_SSH_CONFIG));
  if (process.env.SODECK_TEST_DESKTOP_OPEN) extra.push("--desktop-open-test", path.resolve(process.env.SODECK_TEST_DESKTOP_OPEN));
  if (persisted) extra.push("--verify-persisted");
  if (process.env.SODECK_TEST_UPDATE_BUNDLE) extra.push("--update-bundle-test", path.resolve(process.env.SODECK_TEST_UPDATE_BUNDLE));
  const result = spawnSync(command, [...args, ...extra], {
    stdio: "inherit",
    ...(process.platform === 'win32' ? { env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: path.join(directory, 'webview2') } } : {}),
    timeout: process.env.SODECK_TEST_NETWORK_PLUGIN ? 120_000 : process.env.SODECK_TEST_TERMINAL_PLUGIN ? 90_000 : 30_000,
  });
  if (fs.existsSync(failure)) {
    const log = path.join(directory, 'logs/current.jsonl');
    if (fs.existsSync(log)) {
      // Isolated smoke data only. Include the plugin's real error, not just the
      // host-side wait that noticed it, before the temporary directory is removed.
      const failures = fs.readFileSync(log, 'utf8').split('\n').filter(line => {
        try { const record = JSON.parse(line); return record.level === 'error' || record.level === 'warn'; } catch { return false; }
      });
      console.error(failures.slice(-10).join('\n'));
    }
    throw new Error(fs.readFileSync(failure, "utf8"));
  }
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(
      `Packaged application failed: exit=${result.status}, signal=${result.signal}`,
    );
  if (
    !fs.existsSync(report) ||
    !JSON.parse(fs.readFileSync(report, "utf8")).completed || !JSON.parse(fs.readFileSync(report, "utf8")).workspaceNative
    || (process.env.SODECK_TEST_SSH_CONFIG && !JSON.parse(fs.readFileSync(report, "utf8")).sshNative)
    || (process.env.SODECK_TEST_CLIPBOARD === "1" && !JSON.parse(fs.readFileSync(report, "utf8")).clipboardNative)
    || (process.env.SODECK_TEST_DESKTOP_FILES && !JSON.parse(fs.readFileSync(report, "utf8")).desktopFilesNative)
    || (process.env.SODECK_TEST_DESKTOP_OPEN && !JSON.parse(fs.readFileSync(report, "utf8")).desktopOpenNative)
    || (process.env.SODECK_TEST_NETWORK_PLUGIN && (!fs.existsSync(networkReport) || !JSON.parse(fs.readFileSync(networkReport, "utf8")).completed))
  )
    throw new Error(
      "Packaged application did not complete the current smoke-test protocol",
    );
  const actual = JSON.parse(fs.readFileSync(report, "utf8"));
  if ((process.env.SODECK_EXPECT_CORE_VERSION && actual.version !== process.env.SODECK_EXPECT_CORE_VERSION)
      || (process.env.SODECK_EXPECT_TARGET && actual.target !== process.env.SODECK_EXPECT_TARGET))
    throw new Error(`Packaged application version/target mismatch: ${actual.version ?? "missing"} ${actual.target ?? "missing"}`);
}
try {
  launch(process.env.SODECK_VERIFY_PERSISTED === "1");
  // A file probe changes its provider to text during the first launch and cannot be replayed.
  if (process.env.SODECK_VERIFY_PERSISTED !== "1" && !process.env.SODECK_TEST_DESKTOP_FILES && !process.env.SODECK_TEST_DESKTOP_OPEN) launch(true);
  console.log(
    process.env.SODECK_TEST_DESKTOP_FILES
      ? "Packaged app verified native file clipboard and exited successfully."
      : "Packaged app rendered, moved workspaces between native windows, closed secondary windows, saved catalog records, reopened persisted data and exited successfully.",
  );
} finally {
  // This path was created by mkdtemp in this process, never supplied by the user.
  // WebView2 may release its temporary file handles shortly after the app exits.
  if (!process.env.SODECK_TEST_DATA_DIR)
    fs.rmSync(directory, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
}
