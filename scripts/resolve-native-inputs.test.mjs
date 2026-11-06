import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { targets, coreNames } from "./core-artifacts.mjs";
import { resolveInputs, validatePinned, platforms, packageName } from "./resolve-native-inputs.mjs";
import { nativeGithub } from "./native-github.mjs";

const commit = "a".repeat(40), pluginCommit = "b".repeat(40), scriptCommit = "c".repeat(40);
const digest = bytes => createHash("sha256").update(bytes).digest("hex");
function fixture() {
  const runs = new Map(), artifacts = new Map(), releases = new Map(), bytes = new Map(), tags = new Map();
  const values = { core: "0.9.1", terminal: "0.2.0", files: "0.4.0", network: "0.2.0" };
  const keys = { core: "core", terminal: "terminal", files: "files", network: "network-proxy" };
  let next = 100;
  const state = { runs, artifacts, releases, bytes, tags, values, failedJob: null, apiMajor: "1", missingArtifact: false };
  const config = key => ({ id: `sodeck.${keys[key]}`, version: values[key], ...(key === "core" ? {} : { binary: `sodeck-${keys[key]}` }) });
  const inputs = { core_run_id: "1", terminal_run_id: "2", files_run_id: "3", network_run_id: "4" };
  for (const [key, id] of Object.entries(inputs).map(([field, id]) => [field.replace("_run_id", ""), id])) {
    const source = key === "core" ? commit : pluginCommit;
    runs.set(id, { id: Number(id), path: key === "core" ? ".github/workflows/build.yml" : ".github/workflows/plugin.yml", status: "completed", conclusion: key === "core" ? "failure" : "success", head_sha: source, repository: { full_name: "test/repo" } });
    artifacts.set(id, targets.map(target => ({ id: next++, name: `${key === "core" ? "SoDeck" : `Plugin-${keys[key]}`}-${platforms[target]}`, size_in_bytes: 10, digest: `sha256:${"d".repeat(64)}`, expired: false, workflow_run: { id: Number(id), head_sha: source } })));
  }
  const api = {
    repository: "test/repo",
    async get(route) {
      if (route.startsWith("/actions/runs/")) return runs.get(route.split("/")[3]);
      if (route.startsWith("/git/ref/tags/")) { const sha = tags.get(decodeURIComponent(route.split("/tags/")[1])); return sha ? { object: { type: "commit", sha } } : null; }
      if (route.startsWith("/git/tags/")) return { object: { type: "commit", sha: commit } };
      if (route.startsWith("/releases/tags/")) return [...releases.values()].find(item => item.tag_name === route.split("/tags/")[1]) ?? null;
      if (route.startsWith("/releases/")) return releases.get(Number(route.split("/")[2]));
      throw new Error(`Unexpected GET ${route}`);
    },
    async list(route) {
      if (route.includes("/artifacts")) { const items = artifacts.get(route.split("/")[3]); return state.missingArtifact ? items.slice(1) : items; }
      if (route.includes("/jobs")) return Object.values(platforms).map(name => ({ name, status: "completed", conclusion: name === state.failedJob ? "failure" : "success" }));
      if (route.includes("/workflows/")) return [...runs.values()].filter(run => run.path.endsWith(route.includes("build.yml") ? "build.yml" : "plugin.yml"));
      if (route.includes("/assets")) return releases.get(Number(route.split("/")[2])).assets;
      throw new Error(`Unexpected list ${route}`);
    },
    async text(file) {
      if (file === "src-tauri/tauri.conf.json") return JSON.stringify({ version: values.core });
      if (file === "src-tauri/src/lib.rs") return 'pub const API_VERSION: &str = "1";';
      const key = file.split("/")[1] === "network-proxy" ? "network" : file.split("/")[1];
      if (file.endsWith("Cargo.toml")) return `version = "${values[key]}"`;
      return JSON.stringify({ ...config(key), apiVersion: state.apiMajor });
    },
    async bytes(route) { return bytes.get(Number(route.split("/").at(-1))); },
  };
  function release(key) {
    const tag = key === "core" ? `v${values[key]}` : `plugin-${keys[key]}-v${values[key]}`;
    tags.set(tag, key === "core" ? commit : pluginCommit);
    const assets = [], manifest = { id: config(key).id, version: values[key], ...(key === "core" ? {} : { apiVersion: "1" }), platforms: {} };
    const asset = (name, body = Buffer.from(name)) => {
      const item = { id: next++, name, size: body.length, digest: `sha256:${digest(body)}`, state: "uploaded" };
      assets.push(item); bytes.set(item.id, body); return item;
    };
    for (const target of targets) {
      let updater;
      if (key === "core") {
        const names = coreNames(values[key], target);
        for (const name of Object.values(names).flat()) asset(name);
        updater = target.includes("windows") ? names.nsis[0] : target.includes("linux") ? names.appimage[0] : names.macos[0];
        manifest.platforms[target] = { url: `https://github.com/test/repo/releases/download/${tag}/${updater}`, signature: `${updater}.sig` };
      } else {
        updater = packageName(config(key), target);
        const item = asset(updater);
        manifest.platforms[target] = { url: `https://github.com/test/repo/releases/download/${tag}/${updater}`, sha256: item.digest.slice(7) };
      }
    }
    asset(key === "core" ? "core-update.json" : "plugin-update.json", Buffer.from(JSON.stringify(manifest)));
    const item = { id: next++, tag_name: tag, draft: false, prerelease: false, assets };
    releases.set(item.id, item); return item;
  }
  return { api, inputs, state, release };
}

test("run-only selection accepts successful platform packages despite failed metadata and independent commits", async () => {
  const { api, inputs } = fixture();
  const result = await resolveInputs(api, inputs, scriptCommit);
  assert.equal(result.components.core.source.conclusion, "failure");
  assert.equal(result.components.core.commit, commit);
  assert.equal(result.components.files.commit, pluginCommit);
  assert.equal(Object.keys(result.components.files.platforms).length, 5);
  await validatePinned(api, result);
});

test("invalid inputs, wrong workflow/repository, failed jobs, expired or duplicate Artifacts are rejected", async () => {
  for (const mutate of [
    f => f.inputs.core_run_id = "../1",
    f => f.inputs.core_version = "0.9.2",
    f => f.inputs.files_version = "latest",
    f => f.state.runs.get("1").path = ".github/workflows/check.yml",
    f => f.state.runs.get("1").status = "in_progress",
    f => f.state.runs.get("1").repository.full_name = "wrong/repo",
    f => f.state.failedJob = "windows-aarch64",
    f => f.state.artifacts.get("1")[0].expired = true,
    f => f.state.artifacts.get("1").push(f.state.artifacts.get("1")[0]),
    f => f.state.artifacts.get("1")[0].workflow_run.head_sha = pluginCommit,
    f => f.state.missingArtifact = true,
    f => f.state.apiMajor = "2",
  ]) { const f = fixture(); mutate(f); await assert.rejects(resolveInputs(f.api, f.inputs, scriptCommit)); }
});

test("exact formal Releases override newer same-version experimental runs and select installer separately from updater", async () => {
  const f = fixture();
  for (const key of ["core", "terminal", "files", "network"]) f.release(key);
  const inputs = Object.fromEntries(Object.entries(f.state.values).map(([key, value]) => [`${key}_version`, value]));
  const result = await resolveInputs(f.api, inputs, scriptCommit);
  assert.equal(result.components.core.source.kind, "release");
  assert.match(result.components.core.platforms[targets[2]].file, /\.deb$/);
  assert.match(result.components.core.platforms[targets[2]].signature, /\.AppImage\.sig$/);
  await validatePinned(f.api, result);
  inputs.files_run_id = "3";
  assert.equal((await resolveInputs(f.api, inputs, scriptCommit)).components.files.source.kind, "artifact");
});

test("unpublished versions use unique tag-bound packages, reject ambiguity and do not treat metadata-only runs as packages", async () => {
  const f = fixture(); delete f.inputs.core_run_id; f.inputs.core_version = f.state.values.core;
  f.state.tags.set("v0.9.1", commit);
  f.state.runs.set("5", { ...f.state.runs.get("1"), id: 5, head_sha: pluginCommit });
  f.state.artifacts.set("5", []);
  assert.equal((await resolveInputs(f.api, f.inputs, scriptCommit)).components.core.source.runId, "1");
  f.state.runs.set("6", { ...f.state.runs.get("1"), id: 6 });
  f.state.artifacts.set("6", f.state.artifacts.get("1").map(item => ({ ...item, workflow_run: { id: 6, head_sha: commit } })));
  await assert.rejects(resolveInputs(f.api, f.inputs, scriptCommit), /ambiguous.*1@.*6@/);
  f.state.runs.delete("6");
  f.state.tags.set("v0.9.1", pluginCommit);
  await assert.rejects(resolveInputs(f.api, f.inputs, scriptCommit), /no complete/);
});

test("network omission is explicit and pinned inputs reject changed hashes, tag source, manifest and API", async () => {
  const f = fixture(); delete f.inputs.network_run_id;
  const result = await resolveInputs(f.api, f.inputs, scriptCommit);
  assert.equal(result.components.network, null);
  f.state.artifacts.get("1")[0].digest = `sha256:${"e".repeat(64)}`;
  await assert.rejects(validatePinned(f.api, result), /source changed/);
  const g = fixture(); delete g.inputs.core_run_id; g.inputs.core_version = "0.9.1"; g.state.tags.set("v0.9.1", commit);
  const pinned = await resolveInputs(g.api, g.inputs, scriptCommit);
  g.state.tags.set("v0.9.1", pluginCommit);
  await assert.rejects(validatePinned(g.api, pinned), /tag source changed/);
  pinned.schemaVersion = 2;
  await assert.rejects(validatePinned(g.api, pinned), /Invalid native/);
});

test("formal Release missing platforms, wrong URLs/signatures, absent digests and tag/source conflicts fail without fallback", async () => {
  for (const mutate of [
    (f, r) => r.assets.pop(),
    (f, r) => r.assets[0].digest = null,
    (f, r) => r.prerelease = true,
    (f, r) => f.state.tags.set(r.tag_name, null),
    (f, r) => f.state.bytes.set(r.assets.find(a => a.name === "core-update.json").id, Buffer.from(JSON.stringify({ id: "wrong" }))),
    (f, r) => f.state.bytes.set(r.assets.find(a => a.name.endsWith(".sig")).id, Buffer.from("wrong signature")),
  ]) { const f = fixture(); const r = f.release("core"); mutate(f, r); delete f.inputs.core_run_id; f.inputs.core_version = "0.9.1"; await assert.rejects(resolveInputs(f.api, f.inputs, scriptCommit)); }
});

test("GitHub read errors distinguish absent resources, refuse unauthorized fallback and bound retries/pagination", async () => {
  let calls = 0;
  const api = nativeGithub("test/repo", "test-token", async () => { calls++; return new Response("{}", { status: calls < 3 ? 503 : 200 }); }, async () => {});
  assert.deepEqual(await api.get("/test"), {}); assert.equal(calls, 3);
  const denied = nativeGithub("test/repo", "test-token", async () => new Response("", { status: 403 }));
  await assert.rejects(denied.get("/missing", true), /HTTP 403/);
  const absent = nativeGithub("test/repo", "test-token", async () => new Response("", { status: 404 }));
  assert.equal(await absent.get("/missing", true), null);
  const paged = nativeGithub("test/repo", "test-token", async () => Response.json(Array.from({ length: 100 }, () => ({}))));
  await assert.rejects(paged.list("/items"), /pagination limit.*incomplete/);
});

test("snapshot ZIP downloads use the Artifact API media type while Release assets use octet-stream", async () => {
  const body = Buffer.from("snapshot ZIP"), expected = { size: body.length, sha256: digest(body) };
  const api = nativeGithub("test/repo", "test-token", async (url, options) => {
    const wanted = url.includes("/actions/artifacts/") ? "application/vnd.github+json" : "application/octet-stream";
    return options.headers.Accept === wanted ? new Response(body) : new Response("", { status: 415 });
  });
  assert.deepEqual(await api.bytes("/actions/artifacts/1/zip", expected), body);
  assert.deepEqual(await api.bytes("/releases/assets/1", expected), body);
});
