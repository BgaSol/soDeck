import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { verifyBinary } from "./verify-binary.mjs";

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} failed (${result.status})`);
}
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
run("cargo", [
  "build",
  "--manifest-path",
  "plugins/hello/Cargo.toml",
  "--release",
  "--locked",
  "--target",
  target,
]);
const cargo = fs.readFileSync("plugins/hello/Cargo.toml", "utf8");
const version = cargo.match(/^version = "([^"]+)"/m)[1];
const stem = `sodeck-hello_${version}_${target}`;
const directory = path.resolve("artifacts/plugins", stem);
fs.mkdirSync(path.join(directory, "bin"), { recursive: true });
const executable = `sodeck-hello${target.includes("windows") ? ".exe" : ""}`;
const destination = path.join(directory, "bin", executable);
fs.copyFileSync(
  `plugins/hello/target/${target}/release/${executable}`,
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
      id: "sodeck.hello",
      name: "Hello 插件",
      version,
      apiVersion: "1",
      target,
      entry: `bin/${executable}`,
    },
    null,
    2,
  ) + "\n",
);
if (target.includes("windows"))
  run("tar", [
    "-a",
    "-c",
    "-f",
    `${directory}.zip`,
    "-C",
    path.dirname(directory),
    stem,
  ]);
else
  run("tar", [
    "-czf",
    `${directory}.tar.gz`,
    "-C",
    path.dirname(directory),
    stem,
  ]);
console.log(`Plugin package: ${directory}`);
