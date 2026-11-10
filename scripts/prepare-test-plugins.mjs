import fs from "node:fs";
import path from "node:path";
import { buildPlugin, nativeTarget, run } from "./plugin-build.mjs";

const args = process.argv.slice(2), matrixIndex = args.indexOf("--matrix");
const names = ["terminal", "files", "network-proxy", "system-status", "docker"];
const fields = { "network-proxy": "network", "system-status": "status" };
const matrix = matrixIndex < 0 ? null : JSON.parse(args[matrixIndex + 1]);
const selected = matrix ? names.filter(name => matrix[fields[name] ?? name] || (matrix.communication && ["terminal", "files", "system-status"].includes(name))) : args.filter(arg => arg !== "--ui-only");
if (!selected.length || selected.some(name => !names.includes(name))) throw new Error("Select explicit test plugins or a CI matrix");
const root = path.resolve("artifacts/test-fixtures");
for (const plugin of selected) {
  const pluginRoot = path.resolve("plugins", plugin);
  if (fs.existsSync(path.join(pluginRoot, "package.json"))) {
    if (!process.env.npm_execpath) throw new Error("Use npm run test:prepare -- <plugins>");
    run(process.execPath, [process.env.npm_execpath, "ci", "--ignore-scripts"], { cwd: pluginRoot });
  }
  if (args.includes("--ui-only")) continue;
  const output = path.join(root, plugin);
  if (path.dirname(output) !== root) throw new Error("Invalid test fixture output");
  fs.rmSync(output, { recursive: true, force: true });
  await buildPlugin({ plugin, target: nativeTarget(), output, release: false, installDependencies: false });
}
