import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
export const targets = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin"];

export const sha256 = file => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
export function coreNames(version, target) {
  if (!targets.includes(target)) throw new Error("Invalid core target");
  const arm = target.startsWith("aarch64");
  if (target.includes("windows")) {
    const name = `SoDeck_${version}_${arm ? "arm64" : "x64"}-setup.exe`;
    return { nsis: [name, `${name}.sig`] };
  }
  if (target.includes("linux")) {
    const deb = `SoDeck_${version}_${arm ? "arm64" : "amd64"}.deb`;
    const app = `SoDeck_${version}_${arm ? "aarch64" : "amd64"}.AppImage`;
    return { deb: [deb, `${deb}.sig`], appimage: [app, `${app}.sig`] };
  }
  return { dmg: [`SoDeck_${version}_aarch64.dmg`], macos: [`SoDeck_${version}_aarch64.app.tar.gz`, `SoDeck_${version}_aarch64.app.tar.gz.sig`] };
}
export function artifactMetadata(directory, version, target, commit) {
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("Core artifacts require the source commit SHA");
  return { id: "sodeck.core", version, target, commit, files: Object.values(coreNames(version, target)).flat().map(name => {
    const file = path.join(directory, name), stat = fs.statSync(file);
    if (!stat.isFile() || !stat.size) throw new Error(`Empty core artifact: ${name}`);
    return { name, size: stat.size, sha256: sha256(file) };
  }) };
}
export function validateCoreArtifacts(input, config, commit) {
  if (!/^[a-f0-9]{40}$/.test(commit ?? "")) throw new Error("Core artifacts require the source commit SHA");
  if (!input || !fs.existsSync(input)) throw new Error("No verified core platform packages found");
  const assets = [], present = [];
  for (const entry of fs.readdirSync(input, { withFileTypes: true })) {
    if (entry.name === "core-update.json" && entry.isFile()) continue;
    if (!entry.isDirectory() || !targets.includes(entry.name)) throw new Error(`Unexpected core artifact: ${entry.name}`);
    const target = entry.name, directory = path.join(input, target);
    const metadata = JSON.parse(fs.readFileSync(path.join(directory, "core-artifact.json"), "utf8"));
    if (metadata.id !== config.id || metadata.version !== config.version || metadata.target !== target || metadata.commit !== commit) throw new Error(`Core artifact metadata mismatch: ${target}`);
    const names = Object.values(coreNames(config.version, target)).flat();
    if (!Array.isArray(metadata.files) || metadata.files.length !== names.length || new Set(metadata.files.map(item => item.name)).size !== names.length) throw new Error(`Missing or duplicate core files: ${target}`);
    const actual = fs.readdirSync(directory).filter(name => name !== "core-artifact.json");
    if (actual.length !== names.length || actual.some(name => !names.includes(name))) throw new Error(`Unexpected or missing core files: ${target}`);
    for (const item of metadata.files) {
      if (!names.includes(item.name)) throw new Error(`Wrong core package name: ${item.name}`);
      const file = path.join(directory, item.name), stat = fs.statSync(file);
      if (!stat.isFile() || !stat.size || stat.size !== item.size || sha256(file) !== item.sha256) throw new Error(`Core artifact hash mismatch: ${item.name}`);
      if (item.name.endsWith(".sig") && !fs.readFileSync(file, "utf8").trim()) throw new Error(`Empty updater signature: ${item.name}`);
      assets.push(file);
    }
    present.push(target);
  }
  if (!present.length) throw new Error("No verified core platform packages found");
  return { assets, present, missing: targets.filter(target => !present.includes(target)) };
}
