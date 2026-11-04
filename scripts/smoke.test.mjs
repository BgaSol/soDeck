import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

test("package validation paths survive a launcher changing its working directory", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-launcher-test-"));
  try {
    const appDirectory = path.join(root, "mounted-app");
    fs.mkdirSync(appDirectory);
    fs.writeFileSync(path.join(root, "update.AppImage"), "signed package");
    const launcher = path.join(appDirectory, "launcher.mjs");
    fs.writeFileSync(launcher, `
      import fs from "node:fs";
      import path from "node:path";
      import assert from "node:assert/strict";
      import { fileURLToPath } from "node:url";
      const args = process.argv.slice(2);
      const value = flag => args[args.indexOf(flag) + 1];
      process.chdir(path.dirname(fileURLToPath(import.meta.url)));
      assert.equal(fs.readFileSync(value("--update-bundle-test"), "utf8"), "signed package");
      fs.writeFileSync(path.join(value("--test-data-dir"), "smoke-result.json"), '{"completed":true}');
    `);
    const env = { ...process.env, SODECK_TEST_UPDATE_BUNDLE: "update.AppImage" };
    delete env.SODECK_TEST_PLUGIN;
    delete env.SODECK_TEST_DATA_DIR;
    delete env.SODECK_VERIFY_PERSISTED;
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./smoke.mjs", import.meta.url)), process.execPath, launcher], { cwd: root, env, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
