import assert from "node:assert/strict";
import { test } from "node:test";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { generateKeyPairSync, sign, createHash } from "node:crypto";
import {
  checkVersion,
  parseVersion,
  prepareVersion,
  versionFiles,
} from "./release-version.mjs";
import {
  assetNames,
  collectAssets,
  platforms,
  writeChecksums,
} from "./release-assets.mjs";
import {
  updateAssetNames,
  updateTargets,
  writeUpdateManifest,
  validateUpdaterSize,
} from "./update-assets.mjs";
import { publishRelease } from "./publish-release.mjs";
import { readReleaseNotes } from "./release-notes.mjs";

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
const signing = generateKeyPairSync("ed25519");
const keyId = Buffer.from("12345678");
const publicRaw = signing.publicKey
  .export({ format: "der", type: "spki" })
  .subarray(-32);
const publicKey = Buffer.from(
  "untrusted comment: test key\n" +
    Buffer.concat([Buffer.from("Ed"), keyId, publicRaw]).toString("base64") +
    "\n",
).toString("base64");
function signature(data, version) {
  const sig = sign(
      null,
      createHash("blake2b512").update(data).digest(),
      signing.privateKey,
    ),
    comment = "timestamp:1\tversion:" + version;
  const global = sign(
    null,
    Buffer.concat([sig, Buffer.from(comment)]),
    signing.privateKey,
  );
  return Buffer.from(
    "untrusted comment: test\n" +
      Buffer.concat([Buffer.from("ED"), keyId, sig]).toString("base64") +
      "\ntrusted comment: " +
      comment +
      "\n" +
      global.toString("base64") +
      "\n",
  ).toString("base64");
}
function installers(t, version = checkVersion(), updater = false) {
  const directory = temporary(t);
  for (const name of assetNames(version))
    writeFileSync(join(directory, name), "installer:" + name);
  if (updater)
    for (const name of updateAssetNames(version).filter(
      (n) => !n.endsWith(".sig"),
    )) {
      const data = Buffer.from("updater:" + name);
      writeFileSync(join(directory, name), data);
      writeFileSync(join(directory, name + ".sig"), signature(data, version));
    }
  return directory;
}

test("version command synchronizes all five metadata files and preserves dependency versions", (t) => {
  const directory = versionFixture(t);
  const previousLock = JSON.parse(
    readFileSync(join(directory, "package-lock.json"), "utf8"),
  );
  assert.equal(prepareVersion("1.2.3", directory), "1.2.3");
  assert.equal(checkVersion(directory, "v1.2.3"), "1.2.3");
  const nextLock = JSON.parse(
    readFileSync(join(directory, "package-lock.json"), "utf8"),
  );
  assert.deepEqual(
    nextLock.packages["node_modules/react"],
    previousLock.packages["node_modules/react"],
  );
  assert.throws(() => checkVersion(directory, "v1.2.4"), /does not match/);
  nextLock.packages[""].version = "1.2.2";
  writeFileSync(join(directory, "package-lock.json"), JSON.stringify(nextLock));
  assert.throws(() => checkVersion(directory), /packages\[root\]/);
});

test("invalid tags and versions fail before any version file is changed", (t) => {
  const directory = versionFixture(t);
  const original = readFileSync(join(directory, "package.json"), "utf8");
  for (const invalid of [
    undefined,
    "",
    "v1.2.3",
    "1.2",
    "01.2.3",
    "1.2.3-beta.1",
    "1.2.3+build",
    "256.0.0",
    "1.256.0",
    "1.2.65536",
    "1.2.3\n",
  ]) {
    assert.throws(() => prepareVersion(invalid, directory));
  }
  assert.equal(readFileSync(join(directory, "package.json"), "utf8"), original);
  assert.equal(parseVersion("255.255.65535"), "255.255.65535");
});

test("collect refuses incomplete bundles and ignores unrelated stale versions", (t) => {
  const source = temporary(t);
  const output = join(temporary(t), "assets");
  writeFileSync(join(source, "SoDeck_1.2.3_x64-setup.exe"), "exe");
  writeFileSync(join(source, "SoDeck_1.2.2_x64_en-US.msi"), "stale");
  assert.throws(
    () => collectAssets("windows-x64", source, output, "1.2.3"),
    /\.msi/,
  );
  writeFileSync(join(source, "SoDeck_1.2.3_x64_en-US.msi"), "msi");
  assert.equal(collectAssets("windows-x64", source, output, "1.2.3").length, 2);
  writeFileSync(join(source, "SoDeck_1.2.3_x64_fr-FR.msi"), "duplicate");
  assert.throws(
    () => collectAssets("windows-x64", source, output, "1.2.3"),
    /exactly one/,
  );
});

test("checksums cover exactly five installers and reject missing or empty files", (t) => {
  const directory = installers(t, "1.2.3");
  assert.equal(writeChecksums(directory, "1.2.3").length, 6);
  const checksum = readFileSync(join(directory, "SHA256SUMS.txt"), "utf8");
  for (const name of assetNames("1.2.3")) {
    const hash = createHash("sha256")
      .update(readFileSync(join(directory, name)))
      .digest("hex");
    assert.ok(checksum.includes(hash + "  " + name + "\n"));
  }
  const name = assetNames("1.2.3")[0];
  writeFileSync(join(directory, name), "");
  assert.throws(() => writeChecksums(directory, "1.2.3"), /Empty/);
  rmSync(join(directory, name));
  assert.throws(() => writeChecksums(directory, "1.2.3"), /incomplete/);
});

test("release and updater support Apple Silicon macOS without Intel artifacts", () => {
  assert.deepEqual(Object.keys(platforms).sort(), [
    "linux-x64",
    "macos-arm64",
    "windows-x64",
  ]);
  assert.deepEqual(
    Object.keys(updateTargets).sort(),
    Object.keys(platforms).sort(),
  );
  assert.ok(!assetNames("1.2.3").some((name) => name.includes("macos_x64")));
  assert.ok(
    !updateAssetNames("1.2.3").some((name) => name.includes("macos_x64")),
  );
});

function fakeGitHub() {
  const state = {
    release: undefined,
    assets: [],
    events: [],
    sha: "a".repeat(40),
    failUpload: false,
    corrupt: false,
    releases: [],
  };
  const repos = {
    getLatestRelease: async () => {
      throw Object.assign(new Error("Not found"), { status: 404 });
    },
    listReleases: async () => ({ data: state.releases }),
    getCommit: async () => ({ data: { sha: state.sha } }),
    getReleaseByTag: async () => {
      if (!state.release)
        throw Object.assign(new Error("Not found"), { status: 404 });
      return { data: state.release };
    },
    generateReleaseNotes: async () => ({ data: { body: "Changes" } }),
    createRelease: async (options) => {
      state.events.push("create-draft");
      state.release = {
        ...options,
        id: 1,
        html_url: "https://example.test/release",
      };
      return { data: state.release };
    },
    listReleaseAssets: async () => ({ data: state.assets }),
    deleteReleaseAsset: async ({ asset_id }) => {
      state.assets = state.assets.filter((asset) => asset.id !== asset_id);
    },
    uploadReleaseAsset: async ({ name, data }) => {
      if (state.failUpload) throw new Error("Upload interrupted");
      state.assets.push({
        id: name,
        name,
        size: data.length,
        state: "uploaded",
        digest: state.corrupt
          ? "sha256:wrong"
          : "sha256:" + createHash("sha256").update(data).digest("hex"),
      });
      state.events.push("upload");
    },
    updateRelease: async (options) => {
      state.events.push("publish");
      state.release = { ...state.release, ...options };
      return { data: state.release };
    },
  };
  const summary = {
    addHeading() {
      return this;
    },
    addLink() {
      return this;
    },
    addList() {
      return this;
    },
    async write() {},
  };
  const args = {
    publicKey,
    releaseNotesText: "Test release notes",
    probeManifest: async (expected) => expected,
    github: {
      rest: { repos },
      paginate: async (method, options) => (await method(options)).data,
    },
    context: {
      repo: { owner: "test", repo: "test" },
      ref: "refs/tags/v" + checkVersion(),
      sha: state.sha,
    },
    core: { info() {}, summary },
  };
  return { state, args };
}

test("release is published only after all signed update assets are uploaded and verified", async (t) => {
  const { state, args } = fakeGitHub();
  await publishRelease({
    ...args,
    directory: installers(t, checkVersion(), true),
  });
  assert.equal(state.assets.length, 11);
  assert.equal(state.events[0], "create-draft");
  assert.equal(state.events.at(-1), "publish");
  assert.equal(state.release.draft, false);
  await assert.rejects(
    publishRelease({ ...args, directory: installers(t, checkVersion(), true) }),
    /cannot be overwritten/,
  );
});

test("interrupted uploads remain draft and can be retried", async (t) => {
  const { state, args } = fakeGitHub();
  state.failUpload = true;
  const directory = installers(t, checkVersion(), true);
  await assert.rejects(publishRelease({ ...args, directory }), /interrupted/);
  assert.equal(state.release.draft, true);
  state.failUpload = false;
  await publishRelease({ ...args, directory });
  assert.equal(state.release.draft, false);
  assert.equal(state.assets.length, 11);
});

test("moved tags and asset corruption never publish a release", async (t) => {
  const { state, args } = fakeGitHub();
  const directory = installers(t, checkVersion(), true);
  state.sha = "b".repeat(40);
  await assert.rejects(publishRelease({ ...args, directory }), /tag moved/);
  assert.equal(state.release, undefined);
  state.sha = args.context.sha;
  state.corrupt = true;
  await assert.rejects(
    publishRelease({ ...args, directory }),
    /failed verification/,
  );
  assert.equal(state.release.draft, true);
  assert.ok(!state.events.includes("publish"));
});

test("manifest verifies every signature and pins URLs to the tag", (t) => {
  const version = checkVersion(),
    directory = installers(t, version, true);
  const manifest = writeUpdateManifest(
    directory,
    version,
    { owner: "BgaSol", repo: "soDeck" },
    "Changes",
    "2026-10-20T00:00:00Z",
    publicKey,
  );
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "darwin-aarch64",
    "linux-x86_64",
    "windows-x86_64-nsis",
  ]);
  for (const item of Object.values(manifest.platforms))
    assert.ok(item.url.includes("/download/v" + version + "/"));
  const name = updateAssetNames(version).find((n) => !n.endsWith(".sig"));
  writeFileSync(join(directory, name), "corrupted");
  assert.throws(
    () =>
      writeUpdateManifest(
        directory,
        version,
        { owner: "BgaSol", repo: "soDeck" },
        "",
        "2026-10-20T00:00:00Z",
        publicKey,
      ),
    /verification failed/,
  );
});
test("missing signatures and older stable releases never publish", async (t) => {
  const { state, args } = fakeGitHub(),
    directory = installers(t, checkVersion(), true);
  const signatureName = updateAssetNames(checkVersion()).find((n) =>
    n.endsWith(".sig"),
  );
  rmSync(join(directory, signatureName));
  await assert.rejects(publishRelease({ ...args, directory }));
  assert.ok(!state.events.includes("publish"));
  state.releases = [
    { tag_name: "v255.255.65535", draft: false, prerelease: false },
  ];
  await assert.rejects(
    publishRelease({ ...args, directory: installers(t, checkVersion(), true) }),
    /must be newer/,
  );
});

test("a signed old package cannot be advertised under a new version", (t) => {
  const version = checkVersion(),
    directory = installers(t, version, true);
  const name = updateAssetNames(version).find((n) => !n.endsWith(".sig"));
  writeFileSync(
    join(directory, name + ".sig"),
    signature(readFileSync(join(directory, name)), "0.0.1"),
  );
  assert.throws(
    () =>
      writeUpdateManifest(
        directory,
        version,
        { owner: "BgaSol", repo: "soDeck" },
        "",
        "2026-10-20T00:00:00Z",
        publicKey,
      ),
    /version does not match/,
  );
});

test("all historical stable releases count even if latest was rolled back", async (t) => {
  const { state, args } = fakeGitHub();
  state.releases = [
    { tag_name: "v255.255.65535", draft: false, prerelease: false },
    { tag_name: "v0.0.1", draft: false, prerelease: false },
  ];
  await assert.rejects(
    publishRelease({ ...args, directory: installers(t, checkVersion(), true) }),
    /every published stable/,
  );
  assert.equal(state.release, undefined);
});
test("public probe failures accurately report that the release is already public", async (t) => {
  const { state, args } = fakeGitHub();
  await assert.rejects(
    publishRelease({
      ...args,
      directory: installers(t, checkVersion(), true),
      probeManifest: async () => ({ version: "0.0.1" }),
    }),
    /is public.*probe failed/,
  );
  assert.equal(state.release.draft, false);
});
test("manifest manual assets exist and notes are shared with the release", async (t) => {
  const { state, args } = fakeGitHub();
  const directory = installers(t, checkVersion(), true);
  await publishRelease({ ...args, directory });
  const manifest = JSON.parse(
    readFileSync(join(directory, "latest.json"), "utf8"),
  );
  assert.equal(manifest.notes, args.releaseNotesText);
  assert.ok(state.release.body.startsWith(manifest.notes));
  assert.equal(manifest.sodeck.schemaVersion, 1);
  assert.equal(Object.keys(manifest.sodeck.manualAssets).length, 5);
  const manual = manifest.sodeck.manualAssets["windows-x86_64-msi"];
  rmSync(join(directory, manual));
  assert.throws(() =>
    writeUpdateManifest(
      directory,
      checkVersion(),
      args.context.repo,
      "Notes",
      new Date().toISOString(),
      publicKey,
    ),
  );
});
test("missing or oversized release notes and oversized updater assets fail before publishing", async (t) => {
  const root = temporary(t);
  assert.throws(() => readReleaseNotes("1.0.0", root), /Missing/);
  mkdirSync(join(root, "docs/release-notes"), { recursive: true });
  writeFileSync(join(root, "docs/release-notes/1.0.0.md"), " ");
  assert.throws(() => readReleaseNotes("1.0.0", root), /nonempty/);
  validateUpdaterSize(512 * 1024 * 1024);
  assert.throws(() => validateUpdaterSize(512 * 1024 * 1024 + 1), /512 MiB/);
  const { state, args } = fakeGitHub();
  await assert.rejects(
    publishRelease({
      ...args,
      releaseNotesText: "",
      directory: installers(t, checkVersion(), true),
    }),
    /Missing/,
  );
  assert.equal(state.release, undefined);
});
