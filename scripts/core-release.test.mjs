import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { artifactMetadata, coreNames, targets, validateCoreArtifacts } from "./core-artifacts.mjs";
import { coreConfig, generate } from "./release-metadata.mjs";
import { artifactRun, githubClient, prepareCoreRelease, releaseState } from "./core-release.mjs";

const commit = "a".repeat(40), config = { id: "sodeck.core", version: "1.2.3", tag: "v1.2.3", notes: "Release notes" };
function fixture(t, selected = targets) {
  const input = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-core-artifact-test-"));
  t.after(() => fs.rmSync(input, { recursive: true, force: true }));
  for (const target of selected) {
    const directory = path.join(input, target); fs.mkdirSync(directory);
    for (const name of Object.values(coreNames(config.version, target)).flat()) fs.writeFileSync(path.join(directory, name), `bytes:${name}`);
    fs.writeFileSync(path.join(directory, "core-artifact.json"), JSON.stringify(artifactMetadata(directory, config.version, target, commit)));
  }
  return input;
}
function fakeGithub() {
  const state = { release: null, assets: [], writes: [], tagCommit: commit, tagExists: false, failUpload: false, uploaded: [] };
  const api = async (method, route, body) => {
    if (method === "GET" && route === "") return {};
    if (method === "GET" && route.startsWith("/releases/tags/")) return state.release?.draft ? null : state.release;
    if (method === "GET" && route.startsWith("/releases?")) return state.release ? [{ tag_name: config.tag, ...state.release }] : [];
    if (method === "GET" && route.startsWith("/git/ref/tags/")) return state.release || state.tagExists ? { object: { type: "commit", sha: state.tagCommit } } : null;
    if (method === "GET" && route.startsWith("/releases/1/assets?")) return state.assets;
    state.writes.push({ method, route });
    if (method === "POST" && route === "/git/refs") { assert.equal(body.sha, commit); state.tagExists = true; return {}; }
    if (method === "POST" && route === "/releases") {
      assert.equal(body.draft, true); assert.equal(body.target_commitish, commit);
      state.release = { id: 1, draft: true, upload_url: "https://uploads.github.com/repos/test/repo/releases/1/assets{?name}", html_url: "https://github.com/test/repo/releases/tag/v1.2.3" };
      return state.release;
    }
    if (method === "POST" && route.startsWith("https://uploads.github.com/")) {
      const name = new URL(route).searchParams.get("name");
      const asset = { id: state.assets.length + 1, name, size: body.length, digest: `sha256:${createHash("sha256").update(body).digest("hex")}`, state: "uploaded" };
      state.assets.push(asset); state.uploaded.push(name);
      // Simulate a response lost after GitHub has accepted an upload.
      if (state.failUpload) { state.failUpload = false; throw new Error("Upload connection interrupted"); }
      return asset;
    }
    if (method === "DELETE" && route.startsWith("/releases/assets/")) { state.assets = state.assets.filter(asset => asset.id !== Number(route.split("/").at(-1))); return null; }
    if (method === "PATCH") { state.release.tag_name = body.tag_name ?? "untagged-fixture"; return state.release; }
    throw new Error(`Unexpected API call ${method} ${route}`);
  };
  return { api, state };
}

test("verified five-platform assets retain installer names, AppImage selection and macOS updater URLs", t => {
  const input = fixture(t), verified = validateCoreArtifacts(input, config, commit);
  assert.equal(verified.assets.length, 15); assert.deepEqual(verified.missing, []);
  const manifest = generate("core", config, input);
  assert.equal(Object.keys(manifest.platforms).length, 5);
  for (const target of targets) {
    const item = manifest.platforms[target], name = decodeURIComponent(new URL(item.url).pathname.split("/").at(-1));
    assert.equal(item.signature, fs.readFileSync(path.join(input, target, `${name}.sig`), "utf8"));
    assert.match(item.url, /\/v1\.2\.3\/SoDeck_1\.2\.3_/);
    if (target.includes("linux")) assert.match(name, /\.AppImage$/);
  }
});
test("partial builds report missing native platforms and cannot reuse unrecorded files", t => {
  const input = fixture(t, [targets[0]]), verified = validateCoreArtifacts(input, config, commit);
  assert.deepEqual(verified.missing, targets.slice(1));
  fs.writeFileSync(path.join(input, targets[0], "old.exe"), "old");
  assert.throws(() => validateCoreArtifacts(input, config, commit), /Unexpected or missing/);
});
test("core metadata rejects other components, versions, source commits and platforms", t => {
  const input = fixture(t, [targets[0]]), file = path.join(input, targets[0], "core-artifact.json"), original = JSON.parse(fs.readFileSync(file));
  for (const [key, value] of [["id", "sodeck.files"], ["version", "1.2.2"], ["commit", "b".repeat(40)], ["target", targets[1]]]) {
    fs.writeFileSync(file, JSON.stringify({ ...original, [key]: value }));
    assert.throws(() => validateCoreArtifacts(input, config, commit), /metadata mismatch/);
  }
});
test("missing packages/signatures, duplicate entries and corrupted bytes fail before release writes", t => {
  const input = fixture(t, [targets[0]]), directory = path.join(input, targets[0]), file = path.join(directory, "core-artifact.json"), original = JSON.parse(fs.readFileSync(file));
  fs.writeFileSync(file, JSON.stringify({ ...original, files: [original.files[0], original.files[0]] }));
  assert.throws(() => validateCoreArtifacts(input, config, commit), /duplicate core files/);
  fs.writeFileSync(file, JSON.stringify(original));
  fs.writeFileSync(path.join(directory, original.files[0].name), "corrupt");
  assert.throws(() => validateCoreArtifacts(input, config, commit), /hash mismatch/);
  fs.unlinkSync(path.join(directory, original.files[1].name));
  assert.throws(() => validateCoreArtifacts(input, config, commit), /missing core files/);
});
test("invalid core versions and empty/missing notes fail before compilation", t => {
  const root = fixture(t, []); fs.mkdirSync(path.join(root, "src-tauri")); fs.mkdirSync(path.join(root, "docs/releases"), { recursive: true });
  fs.writeFileSync(path.join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version: "../wrong" }));
  assert.throws(() => coreConfig(root), /Invalid core release version/);
  fs.writeFileSync(path.join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version: config.version }));
  assert.throws(() => coreConfig(root), /ENOENT/);
  fs.writeFileSync(path.join(root, `docs/releases/${config.tag}.md`), " \n");
  assert.throws(() => coreConfig(root), /Missing core release notes/);
});
test("only GitHub 404 represents absence; auth, rate limit, network and server failures stop preflight", async () => {
  for (const status of [401, 403, 429, 500]) {
    const api = githubClient("test/repo", "test-token", async () => new Response("{}", { status }));
    await assert.rejects(api("GET", "/releases/tags/v1.2.3", undefined, false, true), new RegExp(`HTTP ${status}`));
  }
  const absent = githubClient("test/repo", "test-token", async () => new Response("{}", { status: 404 }));
  assert.equal(await absent("GET", "/releases/tags/v1.2.3", undefined, false, true), null);
  await assert.rejects(releaseState(absent, config, commit), /HTTP 404/);
  const failed = githubClient("test/repo", "test-token", async () => { throw new Error("network failure"); });
  await assert.rejects(releaseState(failed, config, commit), /network failure/);
});
test("published versions and tags on other commits are refused before any mutation", async () => {
  const { api, state } = fakeGithub(); state.release = { id: 1, draft: false };
  await assert.rejects(releaseState(api, config, commit), /immutable/);
  state.release.draft = true; state.tagCommit = "b".repeat(40);
  await assert.rejects(releaseState(api, config, commit), /another source commit/);
  assert.deepEqual(state.writes, []);
});
test("GitHub draft by-tag 404 falls back to the authenticated release list, including older pages", async () => {
  const { api, state } = fakeGithub(); state.release = { id: 1, draft: true };
  const paged = async (...args) => args[1] === "/releases?per_page=100&page=1" ? Array.from({ length: 100 }, () => ({ tag_name: "another-component" })) : api(...args);
  assert.equal((await releaseState(paged, config, commit)).id, 1);
  assert.deepEqual(state.writes, []);
});
test("release recovery selects only completed core runs with available Artifacts and their exact source", async () => {
  const run = { path: ".github/workflows/build.yml", status: "completed", head_sha: commit, conclusion: "failure" };
  const item = { name: "SoDeck-windows-x86_64", expired: false };
  const api = async (_method, route) => route.includes("/artifacts?") ? { artifacts: [item] } : run;
  assert.equal(await artifactRun(api, "123"), commit);
  for (const id of ["", "wrong", "../123"]) await assert.rejects(artifactRun(api, id), /Invalid core Artifact/);
  run.path = ".github/workflows/plugin.yml";
  await assert.rejects(artifactRun(api, "123"), /completed core package run/);
  run.path = ".github/workflows/build.yml"; item.expired = true;
  await assert.rejects(artifactRun(api, "123"), /missing or expired/);
});
test("draft upload retries use the same verified input and skip already uploaded identical bytes", async t => {
  const input = fixture(t), { api, state } = fakeGithub(); state.failUpload = true;
  await assert.rejects(prepareCoreRelease(api, config, input, commit, "test/repo"), /interrupted/);
  const result = await prepareCoreRelease(api, config, input, commit, "test/repo");
  assert.equal(result.missing.length, 0); assert.equal(state.assets.length, 16);
  assert.equal(new Set(state.uploaded).size, state.uploaded.length);
  const count = state.uploaded.length;
  await prepareCoreRelease(api, config, input, commit, "test/repo");
  assert.equal(state.uploaded.length, count);
  state.assets[0].digest = "sha256:wrong";
  const before = state.writes.length;
  await assert.rejects(prepareCoreRelease(api, config, input, commit, "test/repo"), /draft asset differs/);
  assert.equal(state.writes.length, before);
});
test("published drafts and old extra platform assets cannot be overwritten during retry", async t => {
  const input = fixture(t, [targets[0]]), { api, state } = fakeGithub();
  await prepareCoreRelease(api, config, input, commit, "test/repo");
  state.assets.push({ id: 100, name: "old-platform.exe", state: "uploaded" });
  await assert.rejects(prepareCoreRelease(api, config, input, commit, "test/repo"), /unexpected or duplicate assets/);
  state.release.draft = false;
  await assert.rejects(prepareCoreRelease(api, config, input, commit, "test/repo"), /immutable/);
});
test("updating draft notes explicitly retains its version tag on repeated preparation", async t => {
  const input = fixture(t, [targets[0]]), { api, state } = fakeGithub();
  await prepareCoreRelease(api, config, input, commit, "test/repo");
  assert.equal(state.release.tag_name, config.tag);
  await prepareCoreRelease(api, config, input, commit, "test/repo");
  assert.equal(state.release.tag_name, config.tag);
});
test("branch manual runs generate a manifest without contacting GitHub or creating Releases", t => {
  const root = fixture(t, [targets[0]]); fs.mkdirSync(path.join(root, "src-tauri")); fs.mkdirSync(path.join(root, "docs/releases"), { recursive: true });
  fs.writeFileSync(path.join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version: config.version }));
  fs.writeFileSync(path.join(root, `docs/releases/${config.tag}.md`), config.notes);
  const env = { ...process.env, GITHUB_SHA: commit, GITHUB_REF_TYPE: "branch", CORE_PUBLISH: "false", GH_TOKEN: "" };
  const script = fileURLToPath(new URL("core-release.mjs", import.meta.url));
  const preflight = spawnSync(process.execPath, [script, "preflight"], { cwd: root, env, encoding: "utf8" });
  assert.equal(preflight.status, 0, preflight.stderr); assert.match(preflight.stdout, /Artifact only/);
  // Source files are outside the artifact input, as on a real runner.
  const input = path.join(root, "input"); fs.mkdirSync(input); fs.renameSync(path.join(root, targets[0]), path.join(input, targets[0]));
  const prepare = spawnSync(process.execPath, [script, "prepare", input], { cwd: root, env, encoding: "utf8" });
  assert.equal(prepare.status, 0, prepare.stderr); assert.match(prepare.stdout, /no Release created/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(input, "core-update.json"))).version, config.version);
  env.GITHUB_REF_TYPE = "tag"; env.GITHUB_REF_NAME = "v1.2.2";
  const wrong = spawnSync(process.execPath, [script, "preflight"], { cwd: root, env, encoding: "utf8" });
  assert.notEqual(wrong.status, 0); assert.match(wrong.stderr, /must match/);
});
