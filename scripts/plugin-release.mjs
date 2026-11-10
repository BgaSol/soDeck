import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { githubClient, releaseState, prepareReleaseAssets } from "./core-release.mjs";
import { pluginConfig, generate, checkTag } from "./release-metadata.mjs";
import { targets, sha256 } from "./core-artifacts.mjs";

export function pluginArtifacts(input, config, commit) {
  const assets = [];
  for (const target of targets) {
    const item = JSON.parse(fs.readFileSync(path.join(input, `plugin-${target}.json`), "utf8"));
    if (item.id !== config.id || item.version !== config.version || item.apiVersion !== config.apiVersion || item.target !== target || item.commit !== commit || path.basename(item.file) !== item.file) throw new Error("Plugin artifact source mismatch");
    const archive = path.join(input, item.file);
    if (sha256(archive) !== item.sha256 || fs.statSync(archive).size !== item.size) throw new Error("Plugin artifact bytes mismatch");
    assets.push(archive);
  }
  return assets;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, requested, input] = process.argv.slice(2);
  const plugin = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME?.match(/^plugin-(.+)-v\d+\.\d+\.\d+$/)?.[1] : requested;
  const config = pluginConfig(plugin);
  const commit = process.env.GITHUB_SHA, repository = process.env.GITHUB_REPOSITORY ?? "BgaSol/soDeck";
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("Require the exact plugin source commit");
  const publish = process.env.PLUGIN_PUBLISH === "true" || process.env.GITHUB_REF_TYPE === "tag";
  if (process.env.GITHUB_REF_TYPE === "tag") checkTag(config, process.env.GITHUB_REF_NAME);
  if (mode === "preflight") {
    if (publish) await releaseState(githubClient(repository, process.env.GH_TOKEN), config, commit);
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `plugin=${plugin}\ntag=${config.tag}\npublish=${publish}\n`);
  } else if (mode === "prepare") {
    const assets = pluginArtifacts(input, config, commit), manifest = path.join(input, "plugin-update.json");
    fs.writeFileSync(manifest, JSON.stringify(generate("plugin", config, input, repository), null, 2) + "\n");
    const release = publish ? await prepareReleaseAssets(githubClient(repository, process.env.GH_TOKEN), config, [...assets, manifest], commit, "plugin-update.json", `${plugin} ${config.version}`) : null;
    const summary = `${config.tag}\n\nSource: ${commit}\nPackage run: https://github.com/${repository}/actions/runs/${process.env.GITHUB_RUN_ID}\nFive native package checks passed.\n${release?.url ?? "Artifact only"}\n`;
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(summary);
  } else throw new Error("Use preflight or prepare");
}
