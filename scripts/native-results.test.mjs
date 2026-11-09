import { test } from "node:test";
import assert from "node:assert/strict";
import { recordResult, summarizeResults } from "./native-results.mjs";
import { targets } from "./core-artifacts.mjs";
import { platforms, verificationMatrix } from "./resolve-native-inputs.mjs";

const commit = "c".repeat(40), target = targets[0];
const snapshot = { components: { core: { version: "0.9.1" }, network: {} } };
const report = { completed: true, workspaceNative: true, sshNative: true, version: "0.9.1", target };
const prepared = { status: "prepared", target, components: {} };
test("a platform retry keeps the default five-target matrix and reports only its selected scope", () => {
  assert.deepEqual(verificationMatrix().include.map(item => item.target), targets);
  const selected = verificationMatrix("windows-aarch64").include;
  assert.deepEqual(selected, [{ name: "windows-aarch64", runner: "windows-11-arm", target: "aarch64-pc-windows-msvc" }]);
  assert.throws(() => verificationMatrix("unknown"));
  const jobs = [{ name: selected[0].name, conclusion: "success", status: "completed" }];
  const results = [{ schemaVersion: 1, target: selected[0].target, platform: selected[0].name, testCommit: commit, status: "success", stage: "complete" }];
  const summary = summarizeResults(snapshot, jobs, results, commit, "windows-aarch64");
  assert.equal(summary.success, true);
  assert.equal(summary.scope, "windows-aarch64");
  assert.equal(summary.platforms.length, 1);
  assert.equal(summarizeResults(snapshot, jobs, results, commit).success, false);
  assert.equal(summarizeResults(snapshot, jobs, [], commit, "windows-aarch64").success, false);
});
test("a result requires actual runtime identity, remote protocol, optional network and successful test execution", () => {
  const record = (r = report, network = { completed: true }, steps = { windows_test: { outcome: "success" } }) => recordResult(snapshot, target, prepared, steps, r, network, "success", commit);
  assert.equal(record().status, "success");
  for (const value of [{ ...report, version: "0.9.2" }, { ...report, target: targets[1] }, { ...report, sshNative: false }, null]) assert.equal(record(value).status, "failure");
  assert.equal(record(report, null).status, "failure");
  assert.equal(record(report, { completed: true }, {}).status, "failure");
  const skipped = recordResult({ components: { core: { version: "0.9.1" }, network: null } }, target, prepared, { windows_test: { outcome: "success" } }, report, null, "success", commit);
  assert.equal(skipped.status, "success");
});
test("failed preparation and cancelled jobs retain explicit stages without a runtime report", () => {
  assert.equal(recordResult(snapshot, target, null, { prepared: { outcome: "failure" } }, null, null, "failure", commit).stage, "prepared");
  assert.equal(recordResult(null, target, null, {}, null, null, "cancelled", commit).status, "cancelled");
});
test("requested native file verification requires the independent provider and exact runtime identity", () => {
  const desktop = {...report, desktopFilesNative:true, independentProvider:true};
  const record = value => recordResult(snapshot, target, prepared, {windows_test:{outcome:'success'}}, report, {completed:true}, 'success', commit, value, true);
  assert.equal(record(desktop).status,'success');
  for(const value of [null,{...desktop,version:'0.9.2'},{...desktop,target:targets[1]},{...desktop,desktopFilesNative:false},{...desktop,independentProvider:false}])assert.equal(record(value).status,'failure');
});
test("five-platform summary rejects missing, duplicated, stale or failed results even if a job says success", () => {
  const jobs = targets.map(target => ({ name: platforms[target], conclusion: "success", status: "completed" }));
  const results = targets.map(target => ({ schemaVersion: 1, target, platform: platforms[target], testCommit: commit, status: "success", stage: "complete" }));
  assert.equal(summarizeResults(snapshot, jobs, results, commit).success, true);
  for (const values of [results.slice(1), [...results, results[0]], results.map((item, i) => i ? item : { ...item, testCommit: "wrong" }), results.map((item, i) => i ? item : { ...item, status: "failure" })]) assert.equal(summarizeResults(snapshot, jobs, values, commit).success, false);
  jobs[0].conclusion = "cancelled";
  assert.equal(summarizeResults(snapshot, jobs, results, commit).platforms[0].status, "cancelled");
});
