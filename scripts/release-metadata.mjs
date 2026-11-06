import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { cargoPackageVersion } from "./cargo-package.mjs";

import { targets, validateCoreArtifacts } from "./core-artifacts.mjs";
export { targets };
export function pluginConfig(plugin, root = ".") {
  if (typeof plugin !== "string" || !/^[a-z][a-z0-9-]*$/.test(plugin)) throw new Error("Invalid plugin name");
  const directory = path.join(root, "plugins", plugin);
  const metadata = JSON.parse(fs.readFileSync(path.join(directory, "release.json"), "utf8"));
  const version = cargoPackageVersion(path.join(directory, "Cargo.toml"));
  return { ...metadata, version, tag: `${metadata.tagPrefix}${version}`, notes: fs.readFileSync(path.join(directory, "CHANGELOG.md"), "utf8") };
}
export function coreConfig(root = ".") {
  const { version } = JSON.parse(fs.readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
  if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("Invalid core release version");
  const notes = fs.readFileSync(path.join(root, `docs/releases/v${version}.md`), "utf8");
  if (!notes.trim()) throw new Error("Missing core release notes");
  return { id: "sodeck.core", version, tag: `v${version}`, notes };
}
export function checkTag(config, tag) {
  if (tag !== config.tag) throw new Error(`Tag ${tag} must match ${config.tag}`);
}
function filesIn(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => entry.isDirectory() ? filesIn(path.join(directory, entry.name)) : [path.join(directory, entry.name)]);
}
export function generate(kind, config, input, repository = "BgaSol/soDeck") {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("Invalid repository");
  const files = filesIn(input);
  const platforms = {};
  const url = file => `https://github.com/${repository}/releases/download/${encodeURIComponent(config.tag)}/${encodeURIComponent(path.basename(file))}`;
  if (kind === "plugin") {
    for (const file of files.filter(p => /^plugin-.*\.json$/.test(path.basename(p)) && path.basename(p) !== "plugin-update.json")) {
      const item = JSON.parse(fs.readFileSync(file, "utf8"));
      if (item.id !== config.id || item.version !== config.version || item.apiVersion !== config.apiVersion || !targets.includes(item.target) || platforms[item.target]) throw new Error("Plugin artifact metadata mismatch");
      const archive = files.find(p => path.basename(p) === item.file);
      if (!archive || createHash("sha256").update(fs.readFileSync(archive)).digest("hex") !== item.sha256) throw new Error("Plugin archive missing or hash mismatch");
      platforms[item.target] = { url: url(archive), sha256: item.sha256 };
    }
  } else if (kind === "core") {
    for (const target of targets) {
      const marker = `${target}/`;
      const updaterExtension = target.includes("windows") ? ".exe" : target.includes("darwin") ? ".app.tar.gz" : ".AppImage";
      const signatures = files.filter(file => file.replaceAll("\\", "/").includes(marker) && file.endsWith(`${updaterExtension}.sig`));
      if (!signatures.length) {
        if (files.some(file => file.replaceAll("\\", "/").includes(marker))) throw new Error(`Missing updater artifact for ${target}`);
        continue;
      }
      if (signatures.length !== 1) throw new Error(`Ambiguous updater artifact for ${target}`);
      const signatureFile = signatures[0];
      const artifact = signatureFile.slice(0, -4);
      if (!fs.existsSync(artifact)) throw new Error(`Missing signed artifact: ${artifact}`);
      const signature = fs.readFileSync(signatureFile, "utf8").trim();
      if (!signature) throw new Error("Empty updater signature");
      platforms[target] = { url: url(artifact), signature };
    }
  } else throw new Error("Unknown release kind");
  if (!Object.keys(platforms).length) throw new Error("No successful platform packages found");
  return { id: config.id, version: config.version, ...(kind === "plugin" ? { apiVersion: config.apiVersion } : {}), notes: config.notes, platforms };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const arg = name => { const index = process.argv.indexOf(`--${name}`); return index < 0 ? undefined : process.argv[index + 1]; };
  const kind = arg("kind");
  const tag = arg("tag") ?? process.env.GITHUB_REF_NAME;
  const plugin = tag?.match(/^plugin-([a-z][a-z0-9-]*)-v\d+\.\d+\.\d+$/)?.[1] ?? arg("plugin");
  if (kind === "plugin" && !plugin) throw new Error("Specify a plugin with --plugin <directory> or a plugin release tag");
  const config = kind === "plugin" ? pluginConfig(plugin) : coreConfig();
  if (arg("tag") || process.env.GITHUB_REF_TYPE === "tag") checkTag(config, tag);
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `${kind === "plugin" ? `plugin=${plugin}\n` : ""}tag=${config.tag}\n`);
  if (arg("input")) {
    if (kind === "core") {
      validateCoreArtifacts(arg("input"), config, process.env.GITHUB_SHA);
    }
    const manifest = generate(kind, config, arg("input"));
    fs.writeFileSync(path.join(arg("input"), `${kind}-update.json`), JSON.stringify(manifest, null, 2) + "\n");
  }
  console.log(config.tag);
}
