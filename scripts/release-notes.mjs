import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseVersion } from "./release-version.mjs";
export function readReleaseNotes(version, root = process.cwd()) {
  parseVersion(version);
  let text;
  try {
    text = readFileSync(
      join(root, "docs/release-notes", version + ".md"),
      "utf8",
    ).trim();
  } catch {
    throw new Error(
      "Missing user-facing release notes: docs/release-notes/" +
        version +
        ".md",
    );
  }
  if (!text || Buffer.byteLength(text, "utf8") > 64 * 1024)
    throw new Error("Release notes must be nonempty and at most 64 KiB");
  return text;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  try {
    readReleaseNotes(process.argv[2]);
    console.log("Release notes verified.");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
