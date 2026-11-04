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
const plugin = process.env.SODECK_TEST_PLUGIN;
function launch(persisted = false) {
  const report = path.join(directory, "smoke-result.json");
  fs.rmSync(report, { force: true });
  const extra = ["--smoke-test", "--test-data-dir", directory];
  if (plugin) extra.push("--plugin-test", plugin);
  if (persisted) extra.push("--verify-persisted");
  if (process.env.SODECK_TEST_UPDATE_BUNDLE) extra.push("--update-bundle-test", path.resolve(process.env.SODECK_TEST_UPDATE_BUNDLE));
  const result = spawnSync(command, [...args, ...extra], {
    stdio: "inherit",
    timeout: 30_000,
  });
  const failure = path.join(directory, "smoke-error.txt");
  if (fs.existsSync(failure)) throw new Error(fs.readFileSync(failure, "utf8"));
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(
      `Packaged application failed: exit=${result.status}, signal=${result.signal}`,
    );
  if (
    !fs.existsSync(report) ||
    !JSON.parse(fs.readFileSync(report, "utf8")).completed
  )
    throw new Error(
      "Packaged application did not complete the current smoke-test protocol",
    );
}
try {
  launch(process.env.SODECK_VERIFY_PERSISTED === "1");
  if (plugin) launch(true);
  console.log(
    plugin
      ? "Packaged core loaded/stopped/reloaded plugin; logs, messages and preferences survived restart."
      : "Packaged application rendered the workbench and exited successfully.",
  );
} finally {
  // This path was created by mkdtemp in this process, never supplied by the user.
  if (!process.env.SODECK_TEST_DATA_DIR)
    fs.rmSync(directory, { recursive: true, force: true });
}
