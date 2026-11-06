import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { configureUbuntuMirror } from "./configure-ubuntu-mirror.mjs";

test("official Ubuntu mirror covers legacy and Deb822 files, preserves ports/vendor sources and is idempotent", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-apt-test-"));
  try {
    fs.mkdirSync(path.join(directory, "sources.list.d"));
    const files = {
      "sources.list": "deb http://azure.archive.ubuntu.com/ubuntu jammy main\r\n",
      "sources.list.d/ubuntu.sources": "Types: deb\nURIs: https://azure.archive.ubuntu.com/ubuntu\nSuites: jammy jammy-updates\nComponents: main universe\n",
      "sources.list.d/extra.list": "deb https://azure.archive.ubuntu.com/ubuntu jammy-security main\n",
      "sources.list.d/ports.sources": "URIs: http://ports.ubuntu.com/ubuntu-ports\n",
      "sources.list.d/vendor.list": "deb https://packages.microsoft.com/repos/code stable main\n",
      "sources.list.d/ignored.backup": "http://azure.archive.ubuntu.com/ubuntu\n",
    };
    for (const [name, content] of Object.entries(files)) fs.writeFileSync(path.join(directory, name), content);
    assert.equal(configureUbuntuMirror(directory).length, 3);
    const expected = { ...files,
      "sources.list": "deb https://archive.ubuntu.com/ubuntu jammy main\r\n",
      "sources.list.d/ubuntu.sources": "Types: deb\nURIs: https://archive.ubuntu.com/ubuntu\nSuites: jammy jammy-updates\nComponents: main universe\n",
      "sources.list.d/extra.list": "deb https://archive.ubuntu.com/ubuntu jammy-security main\n",
    };
    for (const [name, content] of Object.entries(expected)) assert.equal(fs.readFileSync(path.join(directory, name), "utf8"), content);
    assert.deepEqual(configureUbuntuMirror(directory), []);
  } finally {
    assert.equal(path.dirname(directory), path.resolve(os.tmpdir()));
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("missing APT source files are harmless and relative configuration paths are refused", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-apt-test-"));
  try {
    assert.deepEqual(configureUbuntuMirror(directory), []);
    assert.throws(() => configureUbuntuMirror("relative"), /absolute/);
  } finally {
    assert.equal(path.dirname(directory), path.resolve(os.tmpdir()));
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
