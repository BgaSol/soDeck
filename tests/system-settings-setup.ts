import { execFileSync } from "node:child_process";

export default function setup() {
  execFileSync(
    "cargo",
    [
      "run",
      "--quiet",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--example",
      "browser-policy-fixture",
      "--",
      ".qa/system-settings/browser-policy.json",
    ],
    { stdio: "inherit" },
  );
}
