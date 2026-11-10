import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkTag, coreConfig, generate } from "./release-metadata.mjs";
import { sha256, validateCoreArtifacts } from "./core-artifacts.mjs";

export function githubClient(repository, token, request = fetch) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "")) throw new Error("Invalid repository");
  if (!token) throw new Error("GitHub token is required");
  return async (method, route, body, binary = false, optional = false) => {
    const url = route.startsWith("https://uploads.github.com/") ? route : `https://api.github.com/repos/${repository}${route}`;
    const response = await request(url, {
      method, signal: AbortSignal.timeout(120_000),
      headers: { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28", ...(body === undefined ? {} : { "Content-Type": binary ? "application/octet-stream" : "application/json" }) },
      ...(body === undefined ? {} : { body: binary ? body : JSON.stringify(body) }),
    });
    if (optional && response.status === 404) return null;
    if (!response.ok) throw new Error(`GitHub ${method} ${route.split("?")[0]} failed: HTTP ${response.status}`);
    return response.status === 204 ? null : response.json();
  };
}
export async function releaseState(api, config, commit) {
  // Check repository access first; only a genuine missing tag/release is optional.
  await api("GET", "");
  let release = await api("GET", `/releases/tags/${encodeURIComponent(config.tag)}`, undefined, false, true);
  // GitHub's by-tag endpoint returns 404 for drafts, even with write access.
  if (!release) {
    for (let page = 1; ; page++) {
      const items = await api("GET", `/releases?per_page=100&page=${page}`);
      release = items.find(item => item.tag_name === config.tag);
      if (release || items.length < 100) break;
    }
  }
  if (release && !release.draft) throw new Error("Published core versions are immutable; bump the version");
  let ref = await api("GET", `/git/ref/tags/${encodeURIComponent(config.tag)}`, undefined, false, true);
  let object = ref?.object;
  for (let depth = 0; object?.type === "tag" && depth < 5; depth++) object = (await api("GET", `/git/tags/${object.sha}`)).object;
  if (object && (object.type !== "commit" || object.sha !== commit)) throw new Error("Core release tag points to another source commit");
  if (release && !object) throw new Error("Existing core draft has no source tag");
  return release;
}
export async function artifactRun(api, id) {
  if (!/^[1-9]\d*$/.test(id ?? "")) throw new Error("Invalid core Artifact run ID");
  const run = await api("GET", `/actions/runs/${id}`);
  if (run.path !== ".github/workflows/build.yml" || run.status !== "completed" || !/^[a-f0-9]{40}$/.test(run.head_sha ?? "")) throw new Error("Use a completed core package run from this repository");
  const artifacts = await api("GET", `/actions/runs/${id}/artifacts?per_page=100`);
  const core = artifacts.artifacts.filter(item => item.name.startsWith("SoDeck-"));
  if (!core.length || core.some(item => item.expired)) throw new Error("Core Artifacts are missing or expired; rebuild the fixed source");
  return run.head_sha;
}
async function releaseAssets(api, id) {
  const assets = [];
  for (let page = 1; ; page++) {
    const items = await api("GET", `/releases/${id}/assets?per_page=100&page=${page}`);
    assets.push(...items);
    if (items.length < 100) return assets;
  }
}
export async function prepareCoreRelease(api, config, input, commit, repository) {
  const verified = validateCoreArtifacts(input, config, commit);
  const manifest = generate("core", config, input, repository);
  const manifestFile = path.join(input, "core-update.json");
  fs.writeFileSync(manifestFile, JSON.stringify(manifest, null, 2) + "\n");
  return { ...verified, ...await prepareReleaseAssets(api, config, [...verified.assets, manifestFile], commit, "core-update.json", `SoDeck ${config.tag}`) };
}

export async function prepareReleaseAssets(api, config, desired, commit, manifestName, title) {
  let release = await releaseState(api, config, commit);
  let existing = release ? await releaseAssets(api, release.id) : [];
  const names = desired.map(file => path.basename(file));
  if (existing.some(asset => !names.includes(asset.name)) || new Set(existing.map(asset => asset.name)).size !== existing.length) throw new Error("Core draft contains unexpected or duplicate assets; do not mix build inputs");
  // Verify all existing package bytes before changing the draft at all.
  for (const asset of existing.filter(asset => asset.name !== manifestName && asset.state === "uploaded")) {
    const file = desired.find(file => path.basename(file) === asset.name);
    if (asset.size !== fs.statSync(file).size || asset.digest !== `sha256:${sha256(file)}`) throw new Error(`Existing core draft asset differs: ${asset.name}`);
  }
  if (!release) {
    const ref = await api("GET", `/git/ref/tags/${encodeURIComponent(config.tag)}`, undefined, false, true);
    if (!ref) await api("POST", "/git/refs", { ref: `refs/tags/${config.tag}`, sha: commit });
    release = await api("POST", "/releases", { tag_name: config.tag, target_commitish: commit, name: title, body: config.notes, draft: true, make_latest: "false" });
  }
  const checkDraft = async () => {
    // The list/by-tag endpoints can lag immediately after draft creation.
    const current = await api("GET", `/releases/${release.id}`);
    if (!current?.draft || current.id !== release.id || current.tag_name !== config.tag) throw new Error("Release changed while uploading");
  };
  for (const file of desired) {
    const name = path.basename(file), digest = `sha256:${sha256(file)}`, asset = existing.find(asset => asset.name === name);
    if (asset?.state === "uploaded" && asset.digest === digest && asset.size === fs.statSync(file).size) continue;
    await checkDraft();
    if (asset) {
      if (name !== manifestName && asset.state === "uploaded") throw new Error(`Existing core draft asset differs: ${name}`);
      await api("DELETE", `/releases/assets/${asset.id}`);
    }
    const uploaded = await api("POST", `${release.upload_url.split("{")[0]}?name=${encodeURIComponent(name)}`, fs.readFileSync(file), true);
    if (uploaded.digest !== digest || uploaded.size !== fs.statSync(file).size || uploaded.state !== "uploaded") throw new Error(`Uploaded core asset hash mismatch: ${name}`);
  }
  await checkDraft();
  await api("PATCH", `/releases/${release.id}`, { tag_name: config.tag, name: title, body: config.notes });
  existing = await releaseAssets(api, release.id);
  if (existing.length !== desired.length || desired.some(file => !existing.some(asset => asset.name === path.basename(file) && asset.digest === `sha256:${sha256(file)}` && asset.state === "uploaded"))) throw new Error("Core draft asset verification failed");
  return { url: release.html_url };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2], config = coreConfig();
  let commit = process.env.CORE_SOURCE_SHA || process.env.GITHUB_SHA;
  const repository = process.env.GITHUB_REPOSITORY ?? "BgaSol/soDeck";
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("Provide GITHUB_SHA for the exact build source");
  const publish = process.env.CORE_PUBLISH === "true" || process.env.GITHUB_REF_TYPE === "tag";
  if (process.env.GITHUB_REF_TYPE === "tag") checkTag(config, process.env.GITHUB_REF_NAME);
  if (mode === "preflight") {
    if (process.env.CORE_RUN_ID) commit = await artifactRun(githubClient(repository, process.env.GH_TOKEN), process.env.CORE_RUN_ID);
    if (publish) await releaseState(githubClient(repository, process.env.GH_TOKEN), config, commit);
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `tag=${config.tag}\nversion=${config.version}\npublish=${publish}\nsource_sha=${commit}\nrun_id=${process.env.CORE_RUN_ID || process.env.GITHUB_RUN_ID || ""}\n`);
    console.log(`Core ${config.tag}: ${publish ? "prepare draft" : "Artifact only"}\nPackage source: run ${process.env.CORE_RUN_ID || process.env.GITHUB_RUN_ID || "local"} · commit ${commit}`);
  } else if (mode === "prepare") {
    const runId = process.env.CORE_RUN_ID || process.env.GITHUB_RUN_ID;
    if (runId && !/^[1-9]\d*$/.test(runId)) throw new Error("Invalid core Artifact run ID");
    const input = process.argv[3];
    const verified = validateCoreArtifacts(input, config, commit);
    const manifest = generate("core", config, input, repository);
    fs.writeFileSync(path.join(input, "core-update.json"), JSON.stringify(manifest, null, 2) + "\n");
    const result = publish ? await prepareCoreRelease(githubClient(repository, process.env.GH_TOKEN), config, input, commit, repository) : verified;
    const source = runId ? `[run ${runId}](https://github.com/${repository}/actions/runs/${runId})` : "local packages";
    const summary = `Core ${config.tag}\n\nPackage source: ${source} · [commit ${commit}](https://github.com/${repository}/commit/${commit})\n\nVerified: ${result.present.join(", ")}\n\nMissing/failed: ${result.missing.join(", ") || "none"}\n\n${result.url ? `Draft: ${result.url}\n\nConfirm the relevant checks for the package source commit, then publish these draft assets without rebuilding.` : "Artifact only; no Release created.\n\nTo prepare a draft later, run Package SoDeck with publish=true and core_run_id set to the original package run above."}\n`;
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(summary);
  } else throw new Error("Use core-release.mjs preflight or prepare <artifact directory>");
}
