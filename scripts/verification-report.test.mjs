import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { targets, sha256 } from "./core-artifacts.mjs";
import { verifyReports } from "./verification-report.mjs";
import { pluginArtifacts } from "./plugin-release.mjs";
test("release consumes five successful reports bound to the exact source and package bytes", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-verification-test-"));
  const reports = path.join(root, "reports"); fs.mkdirSync(reports);
  const commit = "a".repeat(40), config = { id: "sodeck.terminal", version: "1.2.3", apiVersion: "1" };
  const write = (file, value) => fs.writeFileSync(file, JSON.stringify(value));
  try {
    for (const target of targets) {
      const file = target + ".zip"; fs.writeFileSync(path.join(root, file), target);
      const item = { ...config, target, commit, file, size: Buffer.byteLength(target), sha256: sha256(path.join(root, file)) };
      write(path.join(root, `plugin-${target}.json`), item);
      write(path.join(reports, `terminal-${target}.json`), { ...item, passed: true });
    }
    assert.equal(pluginArtifacts(root, config, commit).length, 5);
    verifyReports("terminal", root, reports, commit);
    assert.throws(() => pluginArtifacts(root, config, "b".repeat(40)), /source/);
    const resultFile = path.join(reports, `terminal-${targets[0]}.json`), result = JSON.parse(fs.readFileSync(resultFile));
    write(resultFile, { ...result, passed: false }); assert.throws(() => verifyReports("terminal", root, reports, commit), /failed/);
    write(resultFile, result); fs.writeFileSync(path.join(root, targets[0] + ".zip"), "changed");
    assert.throws(() => verifyReports("terminal", root, reports, commit), /bytes/);
    assert.throws(() => pluginArtifacts(root, config, commit), /bytes/);
    fs.rmSync(resultFile); assert.throws(() => verifyReports("terminal", root, reports, commit));
  } finally { assert.equal(path.dirname(root), os.tmpdir()); fs.rmSync(root, { recursive: true, force: true }); }
});
