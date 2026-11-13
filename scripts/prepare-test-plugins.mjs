import fs from "node:fs";
import {buildFileEditor} from "./plugin-file-editor-assets.mjs";
import path from "node:path";
import { buildPlugin, nativeTarget, run } from "./plugin-build.mjs";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const names = ["terminal", "files", "network-proxy", "system-status", "docker"];
const fields = { "network-proxy": "network", "system-status": "status" };

export function parseTestPrepareArgs(args) {
  const { values, positionals } = parseArgs({ args, allowPositionals: true, options: {
    matrix: { type: "string", multiple: true },
    "ui-only": { type: "boolean" },
  } });
  const raw = values.matrix?.[0];
  const matrix = raw === undefined ? undefined : JSON.parse(raw);
  if (raw !== undefined && (matrix === null || typeof matrix !== "object" || Array.isArray(matrix))) throw new Error("--matrix must be a JSON object");
  const plugins = raw === undefined ? positionals : names.filter(name => matrix[fields[name] ?? name] || (matrix.communication && ["terminal", "files", "system-status"].includes(name)));
  if (!plugins.length || plugins.some(name => !names.includes(name))) throw new Error("Select explicit test plugins or a CI matrix");
  if (plugins.includes("docker") && !plugins.includes("terminal")) plugins.unshift("terminal");
  return { plugins, uiOnly: values["ui-only"] ?? false };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { plugins, uiOnly } = parseTestPrepareArgs(process.argv.slice(2));
  const root = path.resolve("artifacts/test-fixtures");
  for (const plugin of plugins) {
    const pluginRoot = path.resolve("plugins", plugin);
    if (fs.existsSync(path.join(pluginRoot, "package.json"))) {
      if (!process.env.npm_execpath) throw new Error("Use npm run test:prepare -- <plugins>");
      run(process.execPath, [process.env.npm_execpath, "ci", "--ignore-scripts"], { cwd: pluginRoot });
    }
    if(JSON.parse(fs.readFileSync(path.join(pluginRoot,"release.json"),"utf8")).build?.fileEditor)buildFileEditor();
    if (uiOnly) continue;
    const output = path.join(root, plugin);
    if (path.dirname(output) !== root) throw new Error("Invalid test fixture output");
    fs.rmSync(output, { recursive: true, force: true });
    await buildPlugin({ plugin, target: nativeTarget(), output, release: false, installDependencies: false });
  }
}
