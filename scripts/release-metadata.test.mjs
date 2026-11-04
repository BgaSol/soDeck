import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { checkTag, generate } from "./release-metadata.mjs";
test("component tags stay independent", () => {
  const config = { tag: "plugin-hello-v1.2.3" };
  checkTag(config, "plugin-hello-v1.2.3");
  assert.throws(() => checkTag(config, "v1.2.3"));
  assert.throws(() => checkTag(config, "plugin-files-v1.2.3"));
});
test("plugin manifest includes only packages with matching component, version and hash", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-release-test-"));
  try {
    const config = { id: "sodeck.hello", version: "1.2.3", apiVersion: "1", tag: "plugin-hello-v1.2.3", notes: "hello" };
    const archive = Buffer.from("plugin archive");
    fs.writeFileSync(path.join(root, "hello.zip"), archive);
    const metadata = { id: config.id, version: config.version, apiVersion: "1", target: "aarch64-pc-windows-msvc", file: "hello.zip", sha256: createHash("sha256").update(archive).digest("hex") };
    const file = path.join(root, "plugin-windows.json");
    fs.writeFileSync(file, JSON.stringify(metadata));
    const manifest = generate("plugin", config, root);
    assert.deepEqual(Object.keys(manifest.platforms), [metadata.target]);
    assert.match(manifest.platforms[metadata.target].url, /plugin-hello-v1.2.3\/hello.zip$/);
    fs.writeFileSync(path.join(root, "hello.zip"), "corrupt");
    assert.throws(() => generate("plugin", config, root), /hash mismatch/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
test("core manifests select signatures per native target and require the signed artifact", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-core-release-test-"));
  try {
    const directory = path.join(root, "aarch64-apple-darwin"); fs.mkdirSync(directory);
    fs.writeFileSync(path.join(directory, "SoDeck.app.tar.gz.sig"), "signature");
    const config = { id: "sodeck.core", version: "1.0.0", tag: "v1.0.0", notes: "" };
    assert.throws(() => generate("core", config, root), /Missing signed artifact/);
    fs.writeFileSync(path.join(directory, "SoDeck.app.tar.gz"), "app");
    assert.equal(generate("core", config, root).platforms["aarch64-apple-darwin"].signature, "signature");
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
