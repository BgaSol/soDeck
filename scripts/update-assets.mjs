import { createHash, createPublicKey, verify } from "node:crypto";
import {
  copyFileSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { parseVersion } from "./release-version.mjs";
export const updateTargets = {
  "windows-x64": {
    target: "windows-x86_64-nsis",
    suffix: "windows_x64-setup.exe",
    extension: ".exe",
  },
  "macos-arm64": {
    target: "darwin-aarch64",
    suffix: "macos_arm64.app.tar.gz",
    extension: ".app.tar.gz",
  },
  "macos-x64": {
    target: "darwin-x86_64",
    suffix: "macos_x64.app.tar.gz",
    extension: ".app.tar.gz",
  },
  "linux-x64": {
    target: "linux-x86_64",
    suffix: "linux_x64.AppImage",
    extension: ".AppImage",
  },
};
export function updateAssetNames(version) {
  parseVersion(version);
  return [
    ...new Set(
      Object.values(updateTargets).flatMap((t) => {
        const name = "SoDeck_" + version + "_" + t.suffix;
        return [name, name + ".sig"];
      }),
    ),
  ].sort();
}
export function verifyUpdateSignature(
  data,
  encodedSignature,
  encodedKey,
  version,
) {
  const keyLines = Buffer.from(encodedKey.trim(), "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const lines = Buffer.from(encodedSignature.trim(), "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const key = Buffer.from(keyLines[1] ?? "", "base64"),
    sig = Buffer.from(lines[1] ?? "", "base64");
  if (
    key.length !== 42 ||
    sig.length !== 74 ||
    !lines[2]?.startsWith("trusted comment: ") ||
    !sig.subarray(2, 10).equals(key.subarray(2, 10))
  )
    throw new Error("Invalid updater signature or signing key");
  const algorithm = sig.subarray(0, 2).toString();
  if (!["ED", "Ed"].includes(algorithm))
    throw new Error("Unsupported updater signature algorithm");
  const publicKey = createPublicKey({
    key: Buffer.concat([
      Buffer.from("302a300506032b6570032100", "hex"),
      key.subarray(10),
    ]),
    format: "der",
    type: "spki",
  });
  const payload =
    algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  const signature = sig.subarray(10),
    comment = lines[2].slice("trusted comment: ".length);
  if (
    !verify(null, payload, publicKey, signature) ||
    !verify(
      null,
      Buffer.concat([signature, Buffer.from(comment)]),
      publicKey,
      Buffer.from(lines[3] ?? "", "base64"),
    )
  )
    throw new Error("Updater signature verification failed");
  if (
    version &&
    comment
      .split("\t")
      .find((p) => p.startsWith("version:"))
      ?.slice(8)
      .replace(/^v/, "") !== version
  )
    throw new Error("Updater signature version does not match release");
  return comment;
}
function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory()
      ? walk(join(dir, e.name))
      : e.isFile()
        ? [join(dir, e.name)]
        : [],
  );
}
export function collectUpdateAssets(
  platform,
  source,
  destination,
  version,
  publicKey,
) {
  const target = updateTargets[platform];
  if (!target) throw new Error("Unsupported updater platform");
  const candidates = walk(source).filter(
    (p) =>
      p.endsWith(target.extension) &&
      (target.extension === ".app.tar.gz" || p.includes("_" + version + "_")),
  );
  if (candidates.length !== 1)
    throw new Error("Expected exactly one updater artifact for " + platform);
  const file = candidates[0],
    data = readFileSync(file),
    signature = readFileSync(file + ".sig", "utf8").trim();
  if (!data.length) throw new Error("Empty updater artifact");
  verifyUpdateSignature(data, signature, publicKey, version);
  const name = "SoDeck_" + version + "_" + target.suffix;
  copyFileSync(file, join(destination, name));
  writeFileSync(join(destination, name + ".sig"), signature + "\n");
  return [name, name + ".sig"];
}
export function writeUpdateManifest(
  directory,
  version,
  repo,
  notes,
  publishedAt,
  publicKey,
) {
  parseVersion(version);
  if (
    !/^[A-Za-z0-9_.-]+$/.test(repo.owner) ||
    !/^[A-Za-z0-9_.-]+$/.test(repo.repo)
  )
    throw new Error("Invalid release repository");
  const platforms = {};
  for (const { target, suffix } of Object.values(updateTargets)) {
    const name = "SoDeck_" + version + "_" + suffix;
    const file = join(directory, name);
    if (!statSync(file).size)
      throw new Error("Empty updater artifact: " + name);
    const signature = readFileSync(file + ".sig", "utf8").trim();
    verifyUpdateSignature(readFileSync(file), signature, publicKey, version);
    platforms[target] = {
      url:
        "https://github.com/" +
        repo.owner +
        "/" +
        repo.repo +
        "/releases/download/v" +
        version +
        "/" +
        name,
      signature,
    };
  }
  const manifest = { version, notes, pub_date: publishedAt, platforms };
  writeFileSync(
    join(directory, "latest.json"),
    JSON.stringify(manifest, null, 2) + "\n",
  );
  return manifest;
}
