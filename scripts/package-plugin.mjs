import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { verifyBinary } from "./verify-binary.mjs";
// Git Bash's GNU tar treats C: as a remote host; Windows ships a native bsdtar.
const tar = process.platform === "win32" ? path.join(process.env.SystemRoot, "System32", "tar.exe") : "tar";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: "inherit", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} failed (${result.status})`);
}
const pluginArg = process.argv.indexOf("--plugin");
const plugin = pluginArg === -1 ? "hello" : process.argv[pluginArg + 1];
if (!/^[a-z][a-z0-9-]*$/.test(plugin ?? "")) throw new Error("Invalid plugin directory name");
const pluginRoot = `plugins/${plugin}`;
const metadata = JSON.parse(fs.readFileSync(`${pluginRoot}/release.json`, "utf8"));
const targetArg = process.argv.indexOf("--target");
const target =
  targetArg === -1
    ? spawnSync("rustc", ["-vV"], { encoding: "utf8" }).stdout.match(
        /^host: (.+)$/m,
      )?.[1]
    : process.argv[targetArg + 1];
const targets = [
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "aarch64-apple-darwin",
];
if (!targets.includes(target)) throw new Error(`Unsupported target: ${target}`);
run("cargo", ["build", "--release", "--locked", "--target", target], {
  cwd: pluginRoot,
});
const cargo = fs.readFileSync(`${pluginRoot}/Cargo.toml`, "utf8");
const version = cargo.match(/^version = "([^"]+)"/m)[1];
const stem = `${metadata.binary}_${version}_${target}`;
const outputArg = process.argv.indexOf("--out-dir");
const output =
  outputArg === -1 ? "artifacts/plugins" : process.argv[outputArg + 1];
if (!output) throw new Error("--out-dir requires a directory");
const directory = path.resolve(output, stem);
fs.mkdirSync(path.join(directory, "bin"), { recursive: true });
const executable = `${metadata.binary}${target.includes("windows") ? ".exe" : ""}`;
const destination = path.join(directory, "bin", executable);
if (fs.existsSync(`${pluginRoot}/ui`)) fs.cpSync(`${pluginRoot}/ui`, path.join(directory, "ui"), { recursive: true });
fs.copyFileSync(
  `${pluginRoot}/target/${target}/release/${executable}`,
  destination,
);
verifyBinary(destination, target);
if (!target.includes("windows")) fs.chmodSync(destination, 0o755);
if (target.includes("apple") && process.platform === "darwin")
  run("codesign", ["--force", "--sign", "-", destination]);
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
    },
    null,
    2,
  ) + "\n",
);
if (target.includes("windows"))
  run(tar, [
    "-a",
    "-c",
    "-f",
    `${directory}.zip`,
    "-C",
    path.dirname(directory),
    stem,
  ]);
else
  run(tar, [
    "-czf",
    `${directory}.tar.gz`,
    "-C",
    path.dirname(directory),
    stem,
  ]);
console.log(`Plugin package: ${directory}`);
const archive = `${directory}${target.includes("windows") ? ".zip" : ".tar.gz"}`;
fs.writeFileSync(path.resolve(output, `plugin-${target}.json`), JSON.stringify({ id: metadata.id, version, apiVersion: metadata.apiVersion, target, file: path.basename(archive), sha256: createHash("sha256").update(fs.readFileSync(archive)).digest("hex") }, null, 2) + "\n");
