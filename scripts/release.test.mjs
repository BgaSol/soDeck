import assert from "node:assert/strict";
import { test } from "node:test";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { checkVersion, parseVersion, prepareVersion, versionFiles } from "./release-version.mjs";
import { assetNames, collectAssets, writeChecksums } from "./release-assets.mjs";
import { publishRelease } from "./publish-release.mjs";

function temporary(t) {
  const tempRoot = resolve(tmpdir());
  const directory = mkdtempSync(join(tempRoot, "sodeck-release-"));
  assert.equal(dirname(directory), tempRoot);
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}
function versionFixture(t) {
  const directory = temporary(t);
  for (const file of versionFiles) {
    mkdirSync(dirname(join(directory, file)), { recursive: true });
    copyFileSync(file, join(directory, file));
  }
  return directory;
}
function installers(t, version = checkVersion()) {
  const directory = temporary(t);
  for (const name of assetNames(version)) writeFileSync(join(directory, name), "installer:" + name);
  return directory;
}

test("version command synchronizes all five metadata files and preserves dependency versions", t => {
  const directory = versionFixture(t);
  const previousLock = JSON.parse(readFileSync(join(directory, "package-lock.json"), "utf8"));
  assert.equal(prepareVersion("1.2.3", directory), "1.2.3");
  assert.equal(checkVersion(directory, "v1.2.3"), "1.2.3");
  const nextLock = JSON.parse(readFileSync(join(directory, "package-lock.json"), "utf8"));
  assert.deepEqual(nextLock.packages["node_modules/react"], previousLock.packages["node_modules/react"]);
  assert.throws(() => checkVersion(directory, "v1.2.4"), /does not match/);
  nextLock.packages[""].version = "1.2.2";
  writeFileSync(join(directory, "package-lock.json"), JSON.stringify(nextLock));
  assert.throws(() => checkVersion(directory), /packages\[root\]/);
});

test("invalid tags and versions fail before any version file is changed", t => {
  const directory = versionFixture(t);
  const original = readFileSync(join(directory, "package.json"), "utf8");
  for (const invalid of [undefined, "", "v1.2.3", "1.2", "01.2.3", "1.2.3-beta.1", "1.2.3+build", "256.0.0", "1.256.0", "1.2.65536", "1.2.3\n"]) {
    assert.throws(() => prepareVersion(invalid, directory));
  }
  assert.equal(readFileSync(join(directory, "package.json"), "utf8"), original);
  assert.equal(parseVersion("255.255.65535"), "255.255.65535");
});

test("collect refuses incomplete bundles and ignores unrelated stale versions", t => {
  const source = temporary(t);
  const output = join(temporary(t), "assets");
  writeFileSync(join(source, "SoDeck_1.2.3_x64-setup.exe"), "exe");
  writeFileSync(join(source, "SoDeck_1.2.2_x64_en-US.msi"), "stale");
  assert.throws(() => collectAssets("windows-x64", source, output, "1.2.3"), /\.msi/);
  writeFileSync(join(source, "SoDeck_1.2.3_x64_en-US.msi"), "msi");
  assert.equal(collectAssets("windows-x64", source, output, "1.2.3").length, 2);
  writeFileSync(join(source, "SoDeck_1.2.3_x64_fr-FR.msi"), "duplicate");
  assert.throws(() => collectAssets("windows-x64", source, output, "1.2.3"), /exactly one/);
});

test("checksums cover exactly six installers and reject missing or empty files", t => {
  const directory = installers(t, "1.2.3");
  assert.equal(writeChecksums(directory, "1.2.3").length, 7);
  const checksum = readFileSync(join(directory, "SHA256SUMS.txt"), "utf8");
  for (const name of assetNames("1.2.3")) {
    const hash = createHash("sha256").update(readFileSync(join(directory, name))).digest("hex");
    assert.ok(checksum.includes(hash + "  " + name + "\n"));
  }
  const name = assetNames("1.2.3")[0];
  writeFileSync(join(directory, name), "");
  assert.throws(() => writeChecksums(directory, "1.2.3"), /Empty/);
  rmSync(join(directory, name));
  assert.throws(() => writeChecksums(directory, "1.2.3"), /incomplete/);
});

function fakeGitHub() {
  const state = { release: undefined, assets: [], events: [], sha: "a".repeat(40), failUpload: false, corrupt: false };
  const repos = {
    getCommit: async () => ({ data: { sha: state.sha } }),
    getReleaseByTag: async () => {
      if (!state.release) throw Object.assign(new Error("Not found"), { status: 404 });
      return { data: state.release };
    },
    generateReleaseNotes: async () => ({ data: { body: "Changes" } }),
    createRelease: async options => {
      state.events.push("create-draft");
      state.release = { ...options, id: 1, html_url: "https://example.test/release" };
      return { data: state.release };
    },
    listReleaseAssets: async () => ({ data: state.assets }),
    deleteReleaseAsset: async ({ asset_id }) => { state.assets = state.assets.filter(asset => asset.id !== asset_id); },
    uploadReleaseAsset: async ({ name, data }) => {
      if (state.failUpload) throw new Error("Upload interrupted");
      state.assets.push({ id: name, name, size: data.length, state: "uploaded", digest: state.corrupt ? "sha256:wrong" : "sha256:" + createHash("sha256").update(data).digest("hex") });
      state.events.push("upload");
    },
    updateRelease: async options => {
      state.events.push("publish");
      state.release = { ...state.release, ...options };
      return { data: state.release };
    },
  };
  const summary = { addHeading() { return this; }, addLink() { return this; }, addList() { return this; }, async write() {} };
  const args = {
    github: { rest: { repos }, paginate: async (method, options) => (await method(options)).data },
    context: { repo: { owner: "test", repo: "test" }, ref: "refs/tags/v" + checkVersion(), sha: state.sha },
    core: { info() {}, summary },
  };
  return { state, args };
}

test("release is published only after all seven assets are uploaded and verified", async t => {
  const { state, args } = fakeGitHub();
  await publishRelease({ ...args, directory: installers(t) });
  assert.equal(state.assets.length, 7);
  assert.equal(state.events[0], "create-draft");
  assert.equal(state.events.at(-1), "publish");
  assert.equal(state.release.draft, false);
  await assert.rejects(publishRelease({ ...args, directory: installers(t) }), /cannot be overwritten/);
});

test("interrupted uploads remain draft and can be retried", async t => {
  const { state, args } = fakeGitHub();
  state.failUpload = true;
  const directory = installers(t);
  await assert.rejects(publishRelease({ ...args, directory }), /interrupted/);
  assert.equal(state.release.draft, true);
  state.failUpload = false;
  await publishRelease({ ...args, directory });
  assert.equal(state.release.draft, false);
  assert.equal(state.assets.length, 7);
});

test("moved tags and asset corruption never publish a release", async t => {
  const { state, args } = fakeGitHub();
  const directory = installers(t);
  state.sha = "b".repeat(40);
  await assert.rejects(publishRelease({ ...args, directory }), /tag moved/);
  assert.equal(state.release, undefined);
  state.sha = args.context.sha;
  state.corrupt = true;
  await assert.rejects(publishRelease({ ...args, directory }), /failed verification/);
  assert.equal(state.release.draft, true);
  assert.ok(!state.events.includes("publish"));
});
