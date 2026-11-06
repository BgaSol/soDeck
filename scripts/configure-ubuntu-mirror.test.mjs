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

test("hosted runner mirror lists select the official archive directly and preserve independent mirror lists", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-apt-test-"));
  try {
    fs.mkdirSync(path.join(directory, "sources.list.d"));
    const legacy = path.join(directory, "sources.list"), deb822 = path.join(directory, "sources.list.d/ubuntu.sources");
    fs.writeFileSync(legacy, "deb mirror+file:/etc/apt/apt-mirrors.txt jammy main\ndeb mirror+file:/etc/apt/vendor-mirrors.txt stable main\n");
    fs.writeFileSync(deb822, "Types: deb\nURIs: mirror+file:/etc/apt/apt-mirrors.txt\nSuites: jammy-updates\n");
    assert.equal(configureUbuntuMirror(directory).length, 2);
    assert.equal(fs.readFileSync(legacy, "utf8"), "deb https://archive.ubuntu.com/ubuntu/ jammy main\ndeb mirror+file:/etc/apt/vendor-mirrors.txt stable main\n");
    assert.equal(fs.readFileSync(deb822, "utf8"), "Types: deb\nURIs: https://archive.ubuntu.com/ubuntu/\nSuites: jammy-updates\n");
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
