import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { generate } from "./release-metadata.mjs";

test("macOS collected archives use the same versioned asset names as tauri-action", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-collection-test-"));
  try {
    const target = "aarch64-apple-darwin";
    const source = path.join(root, `src-tauri/target/${target}/release/bundle/macos`);
    fs.mkdirSync(source, { recursive: true });
    fs.writeFileSync(path.join(root, "src-tauri/tauri.conf.json"), '{"version":"0.2.0"}');
    fs.writeFileSync(path.join(source, "SoDeck.app.tar.gz"), "app");
    fs.writeFileSync(path.join(source, "SoDeck.app.tar.gz.sig"), "signature");
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./collect-core.mjs", import.meta.url)), target], { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const manifest = generate("core", { id: "sodeck.core", version: "0.2.0", tag: "v0.2.0", notes: "" }, path.join(root, "artifacts/core"));
    assert.match(manifest.platforms[target].url, /SoDeck_0.2.0_aarch64.app.tar.gz$/);
    assert.equal(manifest.platforms[target].signature, "signature");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
