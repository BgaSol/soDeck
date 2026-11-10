import fs from "node:fs";
import path from "node:path";

export function testPluginDirectory(plugin) {
  if (!/^[a-z][a-z0-9-]*$/.test(plugin)) throw new Error("Invalid test plugin");
  const key = plugin === "network-proxy" ? "NETWORK" : plugin.toUpperCase().replaceAll("-", "_");
  const root = path.resolve(process.env[`SODECK_TEST_${key}_PLUGIN`] || `artifacts/test-fixtures/${plugin}`);
  if (!fs.existsSync(path.join(root, "plugin.json"))) throw new Error(`Missing ${plugin} test fixture; run npm run test:prepare -- ${plugin}`);
  return root;
}
