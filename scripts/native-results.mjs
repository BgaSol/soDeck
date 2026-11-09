import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { nativeGithub } from "./native-github.mjs";
import { platforms, inputSummary, verificationMatrix } from "./resolve-native-inputs.mjs";
import { targets } from "./core-artifacts.mjs";

export function recordResult(snapshot, target, prepared, steps, report, networkReport, jobStatus, testCommit, desktopReport = null, requireDesktop = false) {
  if (!targets.includes(target)) throw new Error("Invalid result platform");
  const executed = ["windows_test", "linux_test", "macos_test"].some(id => steps[id]?.outcome === "success");
  const identity = report?.completed === true && report.workspaceNative === true && report.sshNative === true && report.version === snapshot?.components.core.version && report.target === target;
  const desktop = desktopReport?.completed === true && desktopReport.desktopFilesNative === true && desktopReport.independentProvider === true && desktopReport.version === snapshot?.components.core.version && desktopReport.target === target;
  const complete = prepared?.status === "prepared" && prepared.target === target && executed && identity && (!snapshot.components.network || networkReport?.completed === true) && (!requireDesktop || desktop);
  const failed = Object.entries(steps).find(([, item]) => item.outcome === "failure" || item.outcome === "cancelled");
  return {
    schemaVersion: 1, target, platform: platforms[target], testCommit,
    status: jobStatus === "cancelled" ? "cancelled" : jobStatus === "success" && complete ? "success" : "failure",
    stage: failed?.[0] ?? (complete ? "complete" : executed ? "runtime-report" : "not-executed"),
    components: prepared?.components ?? snapshot?.components ?? null,
    runtime: report ? { version: report.version, target: report.target, completed: report.completed, workspaceNative: report.workspaceNative, sshNative: report.sshNative, networkNative: networkReport?.completed === true, desktopFilesNative: desktop } : null,
  };
}

export function summarizeResults(snapshot, jobs, results, testCommit, platform = "all") {
  const rows = verificationMatrix(platform).include.map(({ target }) => {
    const platform = platforms[target];
    const matches = jobs.filter(job => job.name === platform);
    const job = matches.length === 1 ? matches[0] : null;
    const matching = results.filter(item => item.platform === platform);
    const result = matching.length === 1 ? matching[0] : null;
    const valid = result?.schemaVersion === 1 && result.target === target && result.testCommit === testCommit;
    const success = job?.conclusion === "success" && valid && result.status === "success";
    return { platform, target, status: success ? "success" : job?.conclusion === "cancelled" ? "cancelled" : job?.status && job.status !== "completed" ? "not-completed" : "failure", stage: valid ? result.stage : "missing-or-invalid-result", url: job?.html_url ?? "", result: valid ? result : null };
  });
  return { schemaVersion: 1, testCommit, scope: platform, success: Boolean(snapshot) && rows.every(row => row.status === "success"), inputs: snapshot ?? null, platforms: rows };
}

const read = file => fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : null;
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, target] = process.argv.slice(2);
  const snapshot = read("native-inputs.json"), testCommit = process.env.GITHUB_SHA;
  if (mode === "record") {
    const data = process.env.SODECK_TEST_DATA_DIR;
    const result = recordResult(snapshot, target, read("native-prepared.json"), JSON.parse(process.env.NATIVE_STEPS ?? "{}"), data ? read(path.join(data, "smoke-result.json")) : null, data ? read(path.join(data, "network-smoke-result.json")) : null, process.env.NATIVE_JOB_STATUS, testCommit, read(`artifacts/native-desktop-files/${target}.json`), process.env.NATIVE_DESKTOP_FILES === "true");
    fs.writeFileSync(`native-result-${platforms[target]}.json`, JSON.stringify(result, null, 2) + "\n");
    console.log(`${result.platform}: ${result.status} (${result.stage})`);
  } else if (mode === "summary") {
    const api = nativeGithub(process.env.GITHUB_REPOSITORY, process.env.GH_TOKEN);
    const jobs = await api.list(`/actions/runs/${process.env.GITHUB_RUN_ID}/jobs?filter=latest`, "jobs");
    const directory = "native-results";
    const results = fs.existsSync(directory) ? fs.readdirSync(directory).filter(file => /^native-result-[a-z0-9_-]+\.json$/.test(file)).map(file => read(path.join(directory, file))) : [];
    const result = summarizeResults(snapshot, jobs, results, testCommit, process.env.NATIVE_PLATFORM || "all");
    fs.writeFileSync("native-summary.json", JSON.stringify(result, null, 2) + "\n");
    const summary = `## Native package verification\n\n${snapshot ? `\`\`\`text\n${inputSummary(snapshot)}\n\`\`\`\n\n` : "Input resolution failed; no package combination was selected.\n\n"}| Platform | Result | Stage | Job |\n| --- | --- | --- | --- |\n${result.platforms.map(row => `| ${row.platform} | ${row.status} | ${row.stage} | ${row.url ? `[logs](${row.url})` : "not started"} |`).join("\n")}\n\nTest scripts: \`${testCommit}\`. Retry failed jobs with the pinned inputs, or replay this run using verification_run_id.\n`;
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(summary);
    if (!result.success) process.exitCode = 1;
  } else throw new Error("Use native-results.mjs record <target>|summary");
}
