import { readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const versionFiles = [
  "package.json", "package-lock.json", "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml", "src-tauri/Cargo.lock",
];

export function parseVersion(value) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(value ?? "");
  if (!match || match[0] !== value) throw new Error("Use a release version such as 0.1.1 (tag: v0.1.1).");
  const [, major, minor, patch] = match.map(Number);
  if (major > 255 || minor > 255 || patch > 65535) {
    throw new Error("Windows MSI requires major/minor <= 255 and patch <= 65535.");
  }
  return value;
}

function load(root) {
  return new Map(versionFiles.map(file => [file, readFileSync(resolve(root, file), "utf8")]));
}

function rustVersion(text, lock = false) {
  const pattern = lock
    ? /(\[\[package\]\]\r?\nname = "sodeck"\r?\nversion = ")([^"]+)(")/
    : /(\[package\][\s\S]*?\r?\nversion = ")([^"]+)(")/;
  if (!pattern.test(text)) throw new Error("Cannot locate the sodeck Rust package version.");
  return pattern;
}

export function checkVersion(root = process.cwd(), tag) {
  const files = load(root);
  const pkg = JSON.parse(files.get("package.json"));
  const lock = JSON.parse(files.get("package-lock.json"));
  const version = parseVersion(pkg.version);
  const versions = {
    "package-lock.json": lock.version,
    "package-lock.json packages[root]": lock.packages?.[""]?.version,
    "src-tauri/tauri.conf.json": JSON.parse(files.get("src-tauri/tauri.conf.json")).version,
    "src-tauri/Cargo.toml": files.get("src-tauri/Cargo.toml").match(rustVersion(files.get("src-tauri/Cargo.toml")))[2],
    "src-tauri/Cargo.lock": files.get("src-tauri/Cargo.lock").match(rustVersion(files.get("src-tauri/Cargo.lock"), true))[2],
  };
  for (const [file, actual] of Object.entries(versions)) {
    if (actual !== version) throw new Error(file + ": expected " + version + ", found " + actual);
  }
  if (tag !== undefined && tag !== "v" + version) {
    throw new Error("Tag " + tag + " does not match version v" + version + ". Run npm run release:prepare -- <version> before tagging.");
  }
  return version;
}

export function prepareVersion(version, root = process.cwd()) {
  parseVersion(version);
  const files = load(root);
  // Parse every file before writing so malformed metadata cannot cause a partial update.
  for (const file of versionFiles.filter(file => file.endsWith(".json"))) {
    const data = JSON.parse(files.get(file));
    data.version = version;
    if (file === "package-lock.json") {
      if (!data.packages?.[""]) throw new Error("Missing package-lock.json root package.");
      data.packages[""].version = version;
    }
    files.set(file, JSON.stringify(data, null, 2) + "\n");
  }
  for (const file of ["src-tauri/Cargo.toml", "src-tauri/Cargo.lock"]) {
    files.set(file, files.get(file).replace(rustVersion(files.get(file), file.endsWith(".lock")),
      (_, prefix, previous, suffix) => prefix + version + suffix));
  }
  for (const [file, text] of files) writeFileSync(resolve(root, file), text);
  return checkVersion(root);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [command, argument] = process.argv.slice(2);
    const version = command === "prepare"
      ? prepareVersion(argument)
      : command === "check"
        ? checkVersion(process.cwd(), argument)
        : (() => { throw new Error("Usage: release-version.mjs prepare <version> | check [tag]"); })();
    console.log("Release version: " + version);
    if (process.env.GITHUB_OUTPUT && command === "check" && argument) {
      appendFileSync(process.env.GITHUB_OUTPUT, "version=" + version + "\ntag=v" + version + "\n");
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
