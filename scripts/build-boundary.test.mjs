import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

test("a type-only defect fails source checks while the packaging frontend still builds", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-build-boundary-"));
  const npm = process.env.npm_execpath ?? path.join(path.dirname(process.execPath), "node_modules/npm/bin/npm-cli.js");
  try {
    const { scripts } = JSON.parse(fs.readFileSync("package.json", "utf8"));
    fs.writeFileSync(path.join(root, "package.json"), JSON.stringify({ type: "module", scripts }));
    fs.copyFileSync("tsconfig.json", path.join(root, "tsconfig.json"));
    fs.symlinkSync(path.resolve("node_modules"), path.join(root, "node_modules"), process.platform === "win32" ? "junction" : "dir");
    fs.mkdirSync(path.join(root, "src"));
    fs.writeFileSync(path.join(root, "src/main.ts"), 'const value: number = "type-only failure"; document.body.textContent = String(value);');
    fs.writeFileSync(path.join(root, "index.html"), '<script type="module" src="/src/main.ts"></script>');
    const run = name => spawnSync(process.execPath, [npm, "run", name], { cwd: root, encoding: "utf8", windowsHide: true, timeout: 30000 });
    const checked = run("check");
    assert.notEqual(checked.status, 0); assert.match(checked.stdout + checked.stderr, /TS2322/);
    const built = run("web:build");
    assert.equal(built.status, 0, built.stdout + built.stderr);
    assert(fs.statSync(path.join(root, "dist/index.html")).size > 0);
  } finally {
    assert.equal(path.dirname(root), os.tmpdir());
    fs.rmSync(root, { recursive: true, force: true });
  }
});
