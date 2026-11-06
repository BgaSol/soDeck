import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { artifactMetadata, coreNames } from "./core-artifacts.mjs";
const target = process.argv[2];
const { version } = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const names = coreNames(version, target);
const commit = process.env.GITHUB_SHA ?? spawnSync("git", ["rev-parse", "HEAD"], { encoding: "utf8", windowsHide: true }).stdout?.trim();
const root = `src-tauri/target/${target}/release/bundle`;
const output = `artifacts/core/${target}`;
if (fs.existsSync(output)) throw new Error(`Core artifact output already exists: ${output}`);
fs.mkdirSync(output, { recursive: true });
for (const [kind, files] of Object.entries(names)) {
  for (const name of files) {
    const sourceName = kind === "macos" ? name.replace(`_${version}_aarch64.app.tar.gz`, ".app.tar.gz") : name;
    fs.copyFileSync(path.join(root, kind, sourceName), path.join(output, name));
  }
}
fs.writeFileSync(path.join(output, "core-artifact.json"), JSON.stringify(artifactMetadata(output, version, target, commit), null, 2) + "\n");
