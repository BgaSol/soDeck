import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { validateCoreArtifacts, sha256, targets } from "./core-artifacts.mjs";
import { verifyBinary } from "./verify-binary.mjs";
import { nativeGithub } from "./native-github.mjs";
import { components, platforms, packageName, validatePinned } from "./resolve-native-inputs.mjs";

const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32/tar.exe") : "tar";
function run(args) {
  const result = spawnSync(tar, args, { windowsHide: true, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`Plugin archive command failed: ${result.stderr}`);
  return result.stdout;
}
export function validatePluginArchive(directory, config, target, artifact = true) {
  const file = packageName(config, target), archive = path.join(directory, file);
  if (artifact) {
    const sidecar = JSON.parse(fs.readFileSync(path.join(directory, `plugin-${target}.json`), "utf8"));
    if (sidecar.id !== config.id || sidecar.version !== config.version || sidecar.apiVersion !== config.apiVersion || sidecar.target !== target || sidecar.file !== file || sidecar.sha256 !== sha256(archive)) throw new Error(`Plugin Artifact metadata/hash mismatch: ${config.id} ${target}`);
    const expected = [file, `plugin-${target}.json`];
    if (fs.readdirSync(directory).some(name => !expected.includes(name))) throw new Error("Unexpected plugin Artifact contents");
  }
  const stem = file.replace(/\.(?:tar\.gz|zip)$/, "");
  const entries = run(["-tf", archive]).trim().split(/\r?\n/);
  if (!entries.length || entries.some(name => {
    const parts = name.replaceAll("\\", "/").split("/").filter(Boolean);
    return name.startsWith("/") || name.includes(":") || parts[0] !== stem || parts.some(part => part === "." || part === "..");
  })) throw new Error("Plugin archive must contain one relative package directory");
  run([target.includes("windows") ? "-xf" : "-xzf", archive, "-C", directory]);
  const root = path.join(directory, stem);
  const manifest = JSON.parse(fs.readFileSync(path.join(root, "plugin.json"), "utf8"));
  if (manifest.id !== config.id || manifest.version !== config.version || manifest.apiVersion !== config.apiVersion || manifest.target !== target || typeof manifest.entry !== "string" || !manifest.entry || manifest.entry.includes("\\") || manifest.entry.includes(":") || manifest.entry.startsWith("/") || manifest.entry.split("/").some(part => !part || part === "." || part === "..")) throw new Error(`Extracted plugin identity/API/entry mismatch: ${config.id}`);
  const entry = fs.realpathSync(path.join(root, manifest.entry));
  const relative = path.relative(fs.realpathSync(root), entry);
  if (relative.startsWith("..") || path.isAbsolute(relative)) throw new Error("Plugin entry escapes its package directory");
  verifyBinary(entry, target);
  return { root: path.resolve(root), file, size: fs.statSync(archive).size, sha256: sha256(archive) };
}

export async function preparePackages(api, snapshot, target, root = "packages") {
  if (!targets.includes(target)) throw new Error("Invalid native target");
  await validatePinned(api, snapshot);
  const result = { schemaVersion: 1, target, platform: platforms[target], testCommit: process.env.GITHUB_SHA ?? snapshot.testCommit, status: "prepared", components: {} };
  const env = { SODECK_EXPECT_CORE_VERSION: snapshot.components.core.version, SODECK_EXPECT_TARGET: target, SODECK_TEST_DATA_DIR: path.join(process.env.RUNNER_TEMP ?? os.tmpdir(), "sodeck-native-data") };
  fs.mkdirSync(env.SODECK_TEST_DATA_DIR, { recursive: true });
  for (const [key, config] of Object.entries(snapshot.components)) {
    if (!config) { result.components[key] = null; continue; }
    const directory = path.resolve(root, key), selected = config.platforms[target];
    fs.mkdirSync(directory, { recursive: true });
    if (config.source.kind === "release") {
      const asset = selected.assets.find(item => item.name === selected.file);
      if (!asset) throw new Error(`Missing selected Release package: ${key}`);
      await api.download(`/releases/assets/${asset.id}`, path.join(directory, asset.name), asset);
    }
    if (key === "core") {
      let file = path.join(directory, selected.file);
      if (config.source.kind === "artifact") {
        const checked = validateCoreArtifacts(directory, config, config.commit);
        if (checked.present.join() !== target) throw new Error("Core Artifact target does not match this runner");
        file = path.join(directory, target, selected.file);
      }
      env.SODECK_CORE_PACKAGE = file;
      result.components[key] = { id: config.id, version: config.version, apiVersion: config.apiVersion, commit: config.commit, source: config.source, file: selected.file, size: fs.statSync(file).size, sha256: sha256(file) };
    } else {
      const checked = validatePluginArchive(directory, config, target, config.source.kind === "artifact");
      env[`SODECK_TEST_${key === "network" ? "NETWORK" : key.toUpperCase()}_PLUGIN`] = checked.root;
      result.components[key] = { id: config.id, version: config.version, apiVersion: config.apiVersion, commit: config.commit, source: config.source, file: checked.file, size: checked.size, sha256: checked.sha256 };
    }
  }
  return { env, result };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, target] = process.argv.slice(2);
  if (!targets.includes(target)) throw new Error("Provide the exact native target");
  const snapshot = JSON.parse(fs.readFileSync("native-inputs.json", "utf8"));
  if (mode === "platform") {
    const output = [];
    for (const key of Object.keys(components)) {
      const item = snapshot.components[key];
      output.push(`${key}_kind=${item?.source.kind ?? "none"}`);
      output.push(`${key}_artifact_id=${item?.platforms[target].artifact?.id ?? ""}`);
      output.push(`${key}_run_id=${item?.source.runId ?? ""}`);
    }
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, output.join("\n") + "\n");
  } else if (mode === "prepare") {
    const api = nativeGithub(process.env.GITHUB_REPOSITORY, process.env.GH_TOKEN);
    const { env, result } = await preparePackages(api, snapshot, target);
    if (Object.values(env).some(value => /[\r\n]/.test(value))) throw new Error("Invalid environment path");
    if (process.env.GITHUB_ENV) fs.appendFileSync(process.env.GITHUB_ENV, Object.entries(env).map(([name, value]) => `${name}=${value}`).join("\n") + "\n");
    fs.writeFileSync("native-prepared.json", JSON.stringify(result, null, 2) + "\n");
    console.log(`Verified packages prepared: ${platforms[target]}`);
  } else throw new Error("Use prepare-native-packages.mjs platform|prepare <target>");
}
