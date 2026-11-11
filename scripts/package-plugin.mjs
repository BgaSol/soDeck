import fs from "node:fs";
import path from "node:path";
import { buildPlugin, nativeTarget, run } from "./plugin-build.mjs";
import { cargoPackageVersion } from "./cargo-package.mjs";
import { sha256 } from "./core-artifacts.mjs";
import { spawnSync } from "node:child_process";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

export function parsePluginBuildArgs(args) {
  const { values } = parseArgs({ args, options: {
    plugin: { type: "string", multiple: true },
    target: { type: "string", multiple: true },
    "out-dir": { type: "string", multiple: true },
  } });
  const plugin = values.plugin?.[0];
  if (!plugin) throw new Error("Specify a plugin with --plugin <name>");
  if (!/^[a-z][a-z0-9-]*$/.test(plugin)) throw new Error("Invalid plugin directory name");
  return { plugin, target: values.target?.[0], outDir: values["out-dir"]?.[0] };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { plugin, target: requestedTarget, outDir } = parsePluginBuildArgs(process.argv.slice(2));
  const target = requestedTarget ?? nativeTarget();
  const metadata = JSON.parse(fs.readFileSync(`plugins/${plugin}/release.json`, "utf8"));
  const version = cargoPackageVersion(`plugins/${plugin}/Cargo.toml`);
  const output = path.resolve(outDir ?? "artifacts/plugins");
  const stem = `${metadata.binary}_${version}_${target}`;
  const archive = path.join(output, stem + (target.includes("windows") ? ".zip" : ".tar.gz"));
  const sidecar = path.join(output, `plugin-${target}.json`);
  if (fs.existsSync(archive) || fs.existsSync(sidecar)) throw new Error("Use a clean plugin release output directory");
  const { directory } = await buildPlugin({ plugin, target, output: path.join(output, stem), release: true });
  if (target.includes("apple") && process.platform === "darwin") run("codesign", ["--force", "--sign", "-", path.join(directory, "bin", metadata.binary)]);
  const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32/tar.exe") : "tar";
  run(tar, [target.includes("windows") ? "-a" : "-z", "-cf", archive, "-C", output, stem]);
  const commit = process.env.GITHUB_SHA ?? spawnSync("git", ["rev-parse", "HEAD"], { encoding: "utf8", windowsHide: true }).stdout?.trim();
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("Plugin artifacts require the source commit");
  fs.writeFileSync(sidecar, JSON.stringify({ id: metadata.id, version, apiVersion: metadata.apiVersion, target, commit, runId: process.env.GITHUB_RUN_ID ?? null, file: path.basename(archive), size: fs.statSync(archive).size, sha256: sha256(archive) }, null, 2) + "\n");
  console.log(`Plugin package: ${directory}`);
}
