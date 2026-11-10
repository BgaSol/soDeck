import fs from "node:fs";
import path from "node:path";
import { sha256, targets } from "./core-artifacts.mjs";
import { fileURLToPath } from "node:url";

export function verifyReports(component, input, reports, commit) {
  for (const target of targets) {
    const report = JSON.parse(fs.readFileSync(path.join(reports, `${component}-${target}.json`), "utf8"));
    if (report.passed !== true || report.id !== `sodeck.${component}` || report.target !== target || report.commit !== commit) throw new Error(`Missing or failed ${component} verification: ${target}`);
    const metadata = JSON.parse(fs.readFileSync(component === "core" ? path.join(input, target, "core-artifact.json") : path.join(input, `plugin-${target}.json`), "utf8"));
    if (report.version !== metadata.version || metadata.commit !== commit) throw new Error("Verification source/version mismatch");
    const files = component === "core" ? metadata.files : [{ name: metadata.file, sha256: metadata.sha256 }];
    for (const file of files) {
      const expected = component === "core" ? report.files?.find(item => item.name === file.name)?.sha256 : report.sha256;
      if (expected !== file.sha256 || sha256(path.join(input, ...(component === "core" ? [target] : []), file.name)) !== expected) throw new Error("Verified package bytes changed");
    }
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) verifyReports(...process.argv.slice(2));
