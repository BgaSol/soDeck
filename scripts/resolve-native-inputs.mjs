import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { targets, coreNames } from "./core-artifacts.mjs";
import { nativeGithub, assetRecord } from "./native-github.mjs";

export const platforms = Object.fromEntries(targets.map(target => [target, target.includes("windows") ? `windows-${target.startsWith("aarch64") ? "aarch64" : "x86_64"}` : target.includes("linux") ? `linux-${target.startsWith("aarch64") ? "aarch64" : "x86_64"}` : "macos-aarch64"]));
export const components = { core: "core", terminal: "terminal", files: "files", network: "network-proxy" };
export const snapshotName = "Native-verification-inputs";
const sha = value => /^[a-f0-9]{40}$/.test(value ?? "");
const version = value => /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value ?? "");
const runId = value => /^[1-9]\d*$/.test(String(value ?? ""));
const workflow = key => key === "core" ? ".github/workflows/build.yml" : ".github/workflows/plugin.yml";
const tagName = (key, value) => `${key === "core" ? "v" : `plugin-${components[key]}-v`}${value}`;
const downloadUrl = (repository, tag, name) => `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;

export function packageName(config, target) {
  if (!targets.includes(target) || !version(config.version)) throw new Error("Invalid package target or version");
  if (config.id === "sodeck.core") {
    const names = coreNames(config.version, target);
    return target.includes("windows") ? names.nsis[0] : target.includes("linux") ? names.deb[0] : names.dmg[0];
  }
  if (!/^sodeck\.(terminal|files|network-proxy)$/.test(config.id) || !/^[a-z][a-z0-9-]*$/.test(config.binary ?? "")) throw new Error("Invalid plugin package identity");
  return `${config.binary}_${config.version}_${target}${target.includes("windows") ? ".zip" : ".tar.gz"}`;
}

export async function tagCommit(api, tag) {
  const ref = await api.get(`/git/ref/tags/${encodeURIComponent(tag)}`, true);
  let object = ref?.object;
  for (let depth = 0; object?.type === "tag" && depth < 5; depth++) object = (await api.get(`/git/tags/${object.sha}`)).object;
  if (!object) return null;
  if (object.type !== "commit" || !sha(object.sha)) throw new Error(`Tag does not resolve to a commit: ${tag}`);
  return object.sha;
}

async function sourceConfig(api, key, commit) {
  if (!sha(commit)) throw new Error("Invalid package source commit");
  if (key === "core") {
    const config = JSON.parse(await api.text("src-tauri/tauri.conf.json", commit));
    const apiVersion = (await api.text("src-tauri/src/lib.rs", commit)).match(/pub const API_VERSION:\s*&str\s*=\s*"([^"]+)"/)?.[1];
    if (!version(config.version) || !/^\d+(?:\.\d+)*$/.test(apiVersion ?? "")) throw new Error("Invalid core source version/API");
    return { id: "sodeck.core", version: config.version, apiVersion, commit };
  }
  const plugin = components[key];
  const metadata = JSON.parse(await api.text(`plugins/${plugin}/release.json`, commit));
  const value = (await api.text(`plugins/${plugin}/Cargo.toml`, commit)).match(/^version = "([^"]+)"/m)?.[1];
  if (metadata.id !== `sodeck.${plugin}` || !version(value) || !/^\d+(?:\.\d+)*$/.test(metadata.apiVersion ?? "") || !/^[a-z][a-z0-9-]*$/.test(metadata.binary ?? "")) throw new Error(`Invalid ${plugin} source identity/version/API`);
  return { id: metadata.id, version: value, apiVersion: metadata.apiVersion, binary: metadata.binary, commit };
}

export function artifactRecord(item, name) {
  if (!Number.isSafeInteger(item?.id) || item.id <= 0 || item.name !== name || item.expired || !Number.isSafeInteger(item.size_in_bytes) || item.size_in_bytes <= 0 || !/^sha256:[a-f0-9]{64}$/.test(item.digest ?? "")) throw new Error(`Missing, expired or invalid Artifact: ${name}`);
  return { id: item.id, name, size: item.size_in_bytes, sha256: item.digest.slice(7) };
}

async function artifactComponent(api, key, id, wanted, binding = null, candidate = false) {
  const run = await api.get(`/actions/runs/${id}`);
  if (run.path !== workflow(key) || run.status !== "completed" || !sha(run.head_sha) || (run.repository && run.repository.full_name !== api.repository)) {
    if (candidate) return null;
    throw new Error(`Use a completed ${key} package run from this repository: ${id}`);
  }
  if (binding && run.head_sha !== binding.commit) return null;
  const items = await api.list(`/actions/runs/${id}/artifacts`, "artifacts");
  const chosen = Object.fromEntries(targets.map(target => {
    const name = `${key === "core" ? "SoDeck" : `Plugin-${components[key]}`}-${platforms[target]}`;
    const matching = items.filter(item => item.name === name);
    return [target, matching.length === 1 ? matching[0] : null];
  }));
  if (Object.values(chosen).some(item => !item || item.expired)) {
    if (candidate) return null;
    throw new Error(`${key} run ${id}: five platform Artifacts must be unique, available and unexpired`);
  }
  const jobs = await api.list(`/actions/runs/${id}/jobs?filter=latest`, "jobs");
  if (Object.values(platforms).some(name => jobs.filter(job => job.name === name && job.status === "completed" && job.conclusion === "success").length !== 1)) {
    if (candidate) return null;
    throw new Error(`${key} run ${id}: all five effective package jobs must succeed`);
  }
  const config = await sourceConfig(api, key, run.head_sha);
  if (wanted && config.version !== wanted) {
    if (candidate) return null;
    throw new Error(`${key} version ${wanted} does not match run ${id} version ${config.version}`);
  }
  const selected = Object.fromEntries(targets.map(target => {
    const item = chosen[target];
    if (item.workflow_run && (item.workflow_run.id !== Number(id) || item.workflow_run.head_sha !== run.head_sha)) throw new Error(`Artifact source mismatch: ${item.name}`);
    return [target, { file: packageName(config, target), artifact: artifactRecord(item, item.name) }];
  }));
  return { ...config, source: { kind: "artifact", runId: String(id), conclusion: run.conclusion, tag: binding?.tag ?? null }, platforms: selected };
}

async function releaseComponent(api, key, value, release) {
  const tag = tagName(key, value);
  if (release.draft || release.prerelease || release.tag_name !== tag) throw new Error(`Use a formal exact-version Release: ${tag}`);
  const commit = await tagCommit(api, tag);
  if (!commit) throw new Error(`Release source tag is missing: ${tag}`);
  const config = await sourceConfig(api, key, commit);
  if (config.version !== value) throw new Error(`Release tag/source version mismatch: ${tag}`);
  const assets = await api.list(`/releases/${release.id}/assets`);
  const select = name => {
    const found = assets.filter(asset => asset.name === name);
    if (found.length !== 1) throw new Error(`Missing or duplicate Release asset: ${name}`);
    return assetRecord(found[0], name);
  };
  const manifestAsset = select(key === "core" ? "core-update.json" : "plugin-update.json");
  const manifest = JSON.parse((await api.bytes(`/releases/assets/${manifestAsset.id}`, manifestAsset)).toString("utf8"));
  if (manifest.id !== config.id || manifest.version !== value || (key !== "core" && manifest.apiVersion !== config.apiVersion) || !manifest.platforms || Object.keys(manifest.platforms).length !== targets.length) throw new Error(`Release manifest identity/version/API/platform mismatch: ${tag}`);
  const selected = {};
  for (const target of targets) {
    const item = manifest.platforms[target];
    const file = packageName(config, target);
    const archive = select(file);
    if (key === "core") {
      const names = coreNames(value, target);
      const expected = Object.values(names).flat().map(select);
      const updaterName = target.includes("windows") ? names.nsis[0] : target.includes("linux") ? names.appimage[0] : names.macos[0];
      if (item?.url !== downloadUrl(api.repository, tag, updaterName) || typeof item.signature !== "string" || !item.signature.trim()) throw new Error(`Wrong core updater association: ${target}`);
      const signatureAsset = select(`${updaterName}.sig`);
      const signature = (await api.bytes(`/releases/assets/${signatureAsset.id}`, signatureAsset)).toString("utf8").trim();
      if (signature !== item.signature.trim()) throw new Error(`Core updater signature differs: ${target}`);
      selected[target] = { file, assets: expected, signature: item.signature.trim() };
    } else {
      if (item?.url !== downloadUrl(api.repository, tag, file) || item.sha256 !== archive.sha256) throw new Error(`Plugin Release URL/hash mismatch: ${target}`);
      selected[target] = { file, assets: [archive] };
    }
  }
  return { ...config, source: { kind: "release", id: release.id, tag }, manifest: manifestAsset, platforms: selected };
}

export async function resolveInputs(api, inputs, testCommit) {
  if (!sha(testCommit)) throw new Error("Provide the exact verification script commit");
  const selected = {};
  for (const key of Object.keys(components)) {
    const wanted = inputs[`${key}_version`] ?? "", id = inputs[`${key}_run_id`] ?? "";
    if (key === "network" && !wanted && !id) { selected[key] = null; continue; }
    if (wanted && !version(wanted)) throw new Error(`Invalid ${key} version: use major.minor.patch`);
    if (id && !runId(id)) throw new Error(`Invalid ${key} run ID`);
    if (!wanted && !id) throw new Error(`Provide ${key}_version or ${key}_run_id`);
    if (id) { selected[key] = await artifactComponent(api, key, id, wanted); continue; }
    const tag = tagName(key, wanted);
    const release = await api.get(`/releases/tags/${tag}`, true);
    if (release && !release.draft) { selected[key] = await releaseComponent(api, key, wanted, release); continue; }
    const commit = await tagCommit(api, tag);
    const route = `/actions/workflows/${key === "core" ? "build.yml" : "plugin.yml"}/runs?status=completed${commit ? `&head_sha=${commit}` : ""}`;
    const runs = await api.list(route, "workflow_runs");
    const matching = [];
    for (const run of runs) {
      const found = await artifactComponent(api, key, run.id, wanted, commit ? { tag, commit } : null, true);
      if (found) matching.push(found);
    }
    if (matching.length !== 1) throw new Error(matching.length ? `${key} ${wanted}: ambiguous package runs ${matching.map(item => `${item.source.runId}@${item.commit}`).join(", ")}; specify ${key}_run_id` : `${key} ${wanted}: no complete available five-platform package run`);
    selected[key] = matching[0];
  }
  const coreMajor = selected.core.apiVersion.split(".")[0];
  for (const [key, item] of Object.entries(selected)) if (item && item.apiVersion.split(".")[0] !== coreMajor) throw new Error(`${key} API ${item.apiVersion} is incompatible with core API ${selected.core.apiVersion}`);
  return { schemaVersion: 1, repository: api.repository, testCommit, components: selected };
}

export async function validatePinned(api, snapshot) {
  if (snapshot.schemaVersion !== 1 || snapshot.repository !== api.repository || !sha(snapshot.testCommit) || Object.keys(snapshot.components ?? {}).sort().join() !== Object.keys(components).sort().join()) throw new Error("Invalid native verification snapshot");
  for (const key of Object.keys(components)) {
    const item = snapshot.components[key];
    if (!item && key === "network") continue;
    if (!item || !version(item.version) || !sha(item.commit)) throw new Error(`Invalid pinned ${key} component`);
    let fresh;
    if (item.source?.kind === "artifact") {
      if (!runId(item.source.runId) || (item.source.tag && item.source.tag !== tagName(key, item.version))) throw new Error(`Invalid pinned ${key} run/tag`);
      const binding = item.source.tag ? { tag: item.source.tag, commit: await tagCommit(api, item.source.tag) } : null;
      if (binding && binding.commit !== item.commit) throw new Error(`Pinned tag source changed: ${binding.tag}`);
      fresh = await artifactComponent(api, key, item.source.runId, item.version, binding);
    } else if (item.source?.kind === "release") {
      if (!Number.isSafeInteger(item.source.id) || item.source.id <= 0 || item.source.tag !== tagName(key, item.version)) throw new Error(`Invalid pinned ${key} Release`);
      fresh = await releaseComponent(api, key, item.version, await api.get(`/releases/${item.source.id}`));
    } else throw new Error(`Invalid pinned ${key} source`);
    if (!fresh || ["id", "version", "apiVersion", "binary", "commit"].some(field => fresh[field] !== item[field]) || JSON.stringify(fresh.platforms) !== JSON.stringify(item.platforms) || JSON.stringify(fresh.manifest) !== JSON.stringify(item.manifest)) throw new Error(`Pinned ${key} package source changed`);
  }
  const major = snapshot.components.core.apiVersion.split(".")[0];
  if (Object.values(snapshot.components).some(item => item && item.apiVersion.split(".")[0] !== major)) throw new Error("Pinned plugin/core API mismatch");
  return snapshot;
}

export async function readSnapshot(api, id) {
  if (!runId(id)) throw new Error("Invalid verification run ID");
  const run = await api.get(`/actions/runs/${id}`);
  if (run.path !== ".github/workflows/remote-integration.yml" || (run.status !== "completed" && String(id) !== process.env.GITHUB_RUN_ID)) throw new Error("Use this repository's completed native verification run");
  const items = (await api.list(`/actions/runs/${id}/artifacts`, "artifacts")).filter(item => item.name === snapshotName);
  if (items.length !== 1) throw new Error("Verification input snapshot missing or ambiguous");
  const record = artifactRecord(items[0], snapshotName);
  const zip = await api.bytes(`/actions/artifacts/${record.id}/zip`, record, 8 * 1024 * 1024);
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-native-inputs-"));
  try {
    const archive = path.join(directory, "inputs.zip"), unpacked = path.join(directory, "unpacked");
    fs.writeFileSync(archive, zip); fs.mkdirSync(unpacked);
    const result = spawnSync(process.platform === "win32" ? path.join(process.env.SystemRoot, "System32/tar.exe") : "unzip", process.platform === "win32" ? ["-xf", archive, "-C", unpacked] : ["-q", archive, "-d", unpacked], { windowsHide: true, encoding: "utf8" });
    if (result.status !== 0 || fs.readdirSync(unpacked).join() !== "native-inputs.json") throw new Error("Invalid verification snapshot archive");
    const snapshot = JSON.parse(fs.readFileSync(path.join(unpacked, "native-inputs.json"), "utf8"));
    return { snapshot: await validatePinned(api, snapshot), artifactId: record.id };
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}

export function inputSummary(snapshot) {
  return Object.entries(snapshot.components).map(([key, item]) => item ? `${key} ${item.version}: ${item.source.kind} ${item.source.runId ?? item.source.id} · source ${item.commit}` : `${key}: not included`).join("\n");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const api = nativeGithub(process.env.GITHUB_REPOSITORY, process.env.GH_TOKEN);
  const inputs = JSON.parse(process.env.NATIVE_INPUTS ?? "{}");
  let snapshot, artifactId = "";
  const ownId = process.env.GITHUB_RUN_ID;
  const own = Number(process.env.GITHUB_RUN_ATTEMPT ?? 1) > 1 ? (await api.list(`/actions/runs/${ownId}/artifacts`, "artifacts")).some(item => item.name === snapshotName) : false;
  if (own) ({ snapshot, artifactId } = await readSnapshot(api, ownId));
  else if (inputs.verification_run_id) {
    if (Object.keys(components).some(key => inputs[`${key}_version`] || inputs[`${key}_run_id`])) throw new Error("Snapshot replay cannot be combined with version/run inputs");
    ({ snapshot } = await readSnapshot(api, inputs.verification_run_id));
    snapshot = { ...snapshot, selectionCommit: snapshot.testCommit, testCommit: process.env.GITHUB_SHA, replayedFrom: inputs.verification_run_id };
  } else snapshot = await resolveInputs(api, inputs, process.env.GITHUB_SHA);
  fs.writeFileSync("native-inputs.json", JSON.stringify(snapshot, null, 2) + "\n");
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `upload_snapshot=${!artifactId}\nsnapshot_artifact_id=${artifactId}\n`);
  const summary = `## Native verification inputs\n\n\`\`\`text\n${inputSummary(snapshot)}\n\`\`\`\n`;
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  console.log(inputSummary(snapshot));
}
