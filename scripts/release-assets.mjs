import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { checkVersion, parseVersion } from "./release-version.mjs";

export const platforms = {
  "windows-x64": [[".exe", "windows_x64-setup.exe"], [".msi", "windows_x64.msi"]],
  "macos-arm64": [[".dmg", "macos_arm64.dmg"]],
  "macos-x64": [[".dmg", "macos_x64.dmg"]],
  "linux-x64": [[".deb", "linux_x64.deb"], [".AppImage", "linux_x64.AppImage"]],
};

export function assetNames(version) {
  parseVersion(version);
  return Object.values(platforms).flat().map(([, suffix]) => "SoDeck_" + version + "_" + suffix).sort();
}

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : entry.isFile() ? [file] : [];
  });
}

export function collectAssets(platform, source, destination, version) {
  parseVersion(version);
  const formats = platforms[platform];
  if (!formats) throw new Error("Unknown platform: " + platform);
  const files = walk(source);
  const selected = formats.map(([extension, suffix]) => {
    const matches = files.filter(file => file.endsWith(extension) && file.includes("_" + version + "_"));
    if (matches.length !== 1 || statSync(matches[0]).size === 0) {
      throw new Error("Expected exactly one nonempty " + extension + " installer for " + platform + " " + version);
    }
    return [matches[0], "SoDeck_" + version + "_" + suffix];
  });
  mkdirSync(destination, { recursive: true });
  for (const [sourceFile, name] of selected) copyFileSync(sourceFile, join(destination, name));
  return selected.map(([, name]) => name);
}

export function writeChecksums(directory, version) {
  const expected = assetNames(version);
  const actual = readdirSync(directory).filter(name => name !== "SHA256SUMS.txt").sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error("Release installers are incomplete or unexpected. Expected: " + expected.join(", ") + "; found: " + actual.join(", "));
  }
  const lines = expected.map(name => {
    const file = join(directory, name);
    if (!statSync(file).isFile() || statSync(file).size === 0) throw new Error("Empty or invalid installer: " + name);
    return createHash("sha256").update(readFileSync(file)).digest("hex") + "  " + name;
  });
  writeFileSync(join(directory, "SHA256SUMS.txt"), lines.join("\n") + "\n");
  return [...expected, "SHA256SUMS.txt"];
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [command, ...args] = process.argv.slice(2);
    const version = checkVersion();
    if (command === "collect") {
      console.log(collectAssets(args[0], args[1], args[2], version).join("\n"));
    } else if (command === "checksums") {
      console.log(writeChecksums(args[0], version).join("\n"));
    } else throw new Error("Usage: release-assets.mjs collect <platform> <bundle-dir> <output-dir> | checksums <output-dir>");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
