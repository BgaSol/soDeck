import { spawnSync } from "node:child_process";

const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error("Provide the packaged application or its launcher");

const result = spawnSync(command, [...args, "--smoke-test"], {
  stdio: "inherit",
  timeout: 30_000,
});
if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error(`Packaged application failed: exit=${result.status}, signal=${result.signal}`);
}
console.log("Packaged application loaded its webview and exited successfully.");
