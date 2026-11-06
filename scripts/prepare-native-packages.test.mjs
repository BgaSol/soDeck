import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { validatePluginArchive } from "./prepare-native-packages.mjs";
import { nativeGithub } from "./native-github.mjs";
import { packageName } from "./resolve-native-inputs.mjs";
import { sha256 } from "./core-artifacts.mjs";

const target = "x86_64-unknown-linux-gnu";
function fixture(t, change = () => {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-native-package-test-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const config = { id: "sodeck.files", version: "0.4.0", apiVersion: "1", binary: "sodeck-files" };
  const file = packageName(config, target), stem = file.slice(0, -7);
  const source = path.join(root, "source"), directory = path.join(root, "download");
  const packageRoot = path.join(source, stem);
  fs.mkdirSync(path.join(packageRoot, "bin"), { recursive: true }); fs.mkdirSync(directory);
  const binary = Buffer.alloc(64); binary.write("\x7fELF", 0, "ascii"); binary[4] = 2; binary.writeUInt16LE(62, 18);
  const manifest = { id: config.id, version: config.version, apiVersion: "1", target, entry: "bin/tool" };
  change({ binary, manifest });
  fs.writeFileSync(path.join(packageRoot, "bin/tool"), binary);
  fs.writeFileSync(path.join(packageRoot, "plugin.json"), JSON.stringify(manifest));
  const archive = path.join(directory, file);
  const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32/tar.exe") : "tar";
  const result = spawnSync(tar, ["-czf", archive, "-C", source, stem], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  fs.writeFileSync(path.join(directory, `plugin-${target}.json`), JSON.stringify({ ...config, target, file, sha256: sha256(archive) }));
  return { root, directory, config, archive };
}

test("verified plugin archive is unpacked to its exact directory with matching identity and architecture", t => {
  const f = fixture(t);
  const result = validatePluginArchive(f.directory, f.config, target);
  assert.equal(path.basename(result.root), "sodeck-files_0.4.0_x86_64-unknown-linux-gnu");
  assert.equal(result.sha256, sha256(f.archive));
});

test("corrupt archives, unexpected files, wrong component/API/platform, escaping entry and wrong architecture fail", t => {
  const corrupt = fixture(t); fs.appendFileSync(corrupt.archive, "bad");
  assert.throws(() => validatePluginArchive(corrupt.directory, corrupt.config, target), /hash mismatch/);
  const extra = fixture(t); fs.writeFileSync(path.join(extra.directory, "another-package.zip"), "bad");
  assert.throws(() => validatePluginArchive(extra.directory, extra.config, target), /Unexpected/);
  for (const change of [
    ({ manifest }) => manifest.id = "sodeck.terminal",
    ({ manifest }) => manifest.apiVersion = "2",
    ({ manifest }) => manifest.target = "aarch64-unknown-linux-gnu",
    ({ manifest }) => manifest.version = "0.4.1",
    ({ manifest }) => manifest.entry = "../outside",
    ({ binary }) => binary.writeUInt16LE(183, 18),
  ]) { const f = fixture(t, change); assert.throws(() => validatePluginArchive(f.directory, f.config, target)); }
});

test("streamed Release downloads reject truncation, wrong digest and excess bytes without promoting partial files", async t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-native-download-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const bytes = Buffer.from("complete package"), expected = { size: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
  const file = path.join(directory, "package.bin");
  const api = nativeGithub("test/repo", "test-token", async () => new Response(bytes));
  await api.download("/releases/assets/1", file, expected);
  assert.deepEqual(fs.readFileSync(file), bytes);
  fs.rmSync(file);
  for (const body of [Buffer.from("short"), Buffer.alloc(bytes.length), Buffer.concat([bytes, bytes])]) {
    const bad = nativeGithub("test/repo", "test-token", async () => new Response(body));
    await assert.rejects(bad.download("/releases/assets/1", file, expected));
    assert.equal(fs.existsSync(file), false); assert.equal(fs.existsSync(`${file}.part`), false);
  }
});
