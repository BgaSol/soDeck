import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { verifyBinary } from "./verify-binary.mjs";
import { packageSettingsAssets } from "./plugin-settings-assets.mjs";
import { cargoPackageVersion } from "./cargo-package.mjs";
import { packageMenuAssets } from "./plugin-menu-assets.mjs";
import { targets } from "./core-artifacts.mjs";
export function run(command, args, options = {}) {
  const result = spawnSync(command, args, { windowsHide: true, stdio: "inherit", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status})`);
}
export function nativeTarget() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error("Native Rust toolchain unavailable");
  const target = result.stdout.match(/^host: (.+)$/m)?.[1];
  if (!targets.includes(target)) throw new Error("Unsupported native Rust target");
  return target;
}
export async function buildPlugin({ plugin, target, output, release, installDependencies = true }) {
  if (!/^[a-z][a-z0-9-]*$/.test(plugin ?? "") || !targets.includes(target)) throw new Error("Invalid plugin or target");
  const directory = path.resolve(output);
  if (fs.existsSync(directory)) throw new Error(`Plugin output already exists: ${directory}`);
  const pluginRoot = `plugins/${plugin}`;
  const metadata = JSON.parse(fs.readFileSync(`${pluginRoot}/release.json`, "utf8"));
  const version = cargoPackageVersion(`${pluginRoot}/Cargo.toml`);
  if (fs.existsSync(`${pluginRoot}/package.json`)) {
    // Explicit UI build, isolated from the core and other plugins; no arbitrary release hooks.
    let npmCli=process.env.npm_execpath;
    if(process.platform==='win32'&&!npmCli){
      const location=spawnSync('where.exe',['npm.cmd'],{encoding:'utf8',windowsHide:true});
      if(location.status!==0)throw new Error('Node.js/npm is required to build this plugin UI');
      npmCli=path.join(path.dirname(location.stdout.trim().split(/\r?\n/)[0]),'node_modules/npm/bin/npm-cli.js');
    }
    const npm=(args)=>npmCli?run(process.execPath,[npmCli,...args],{cwd:pluginRoot}):run('npm',args,{cwd:pluginRoot});
    if (installDependencies) npm(["ci", "--ignore-scripts"]);
    npm(["run", "build"]);
  }
  run("cargo", ["build", ...(release ? ["--release"] : []), "--locked", "--target", target], {
    cwd: pluginRoot,
  });
  fs.mkdirSync(path.join(directory, "bin"), { recursive: true });
  const executable = `${metadata.binary}${target.includes("windows") ? ".exe" : ""}`;
  const destination = path.join(directory, "bin", executable);
  if (fs.existsSync(`${pluginRoot}/ui`)) fs.cpSync(`${pluginRoot}/ui`, path.join(directory, "ui"), { recursive: true });
  if (metadata.ui) fs.copyFileSync("src/ui.css", path.join(directory, "ui/shared.css"));
  if (metadata.ui) fs.copyFileSync("plugins/shared/clipboard.js", path.join(directory, "ui/clipboard-bridge.js"));
  if (['terminal','files'].includes(plugin)) fs.copyFileSync("plugins/shared/state.js", path.join(directory, "ui/state.js"));
  if (metadata.ui) await packageMenuAssets(directory);
  packageSettingsAssets(metadata, directory);
  fs.copyFileSync(
    `${pluginRoot}/target/${target}/${release ? "release" : "debug"}/${executable}`,
    destination,
  );
  verifyBinary(destination, target);
  if (!target.includes("windows")) fs.chmodSync(destination, 0o755);
  fs.writeFileSync(
    path.join(directory, "plugin.json"),
    JSON.stringify(
      {
        id: metadata.id,
        name: metadata.name,
        version,
        apiVersion: metadata.apiVersion,
        target,
        entry: `bin/${executable}`,
        updateSource: { repository: "BgaSol/soDeck", tagPrefix: metadata.tagPrefix },
        ...(metadata.ui ? { ui: metadata.ui } : {}),
        ...(metadata.settings ? { settings: metadata.settings } : {}),
      },
      null,
      2,
    ) + "\n",
  );

  return { directory, metadata, version };
}
