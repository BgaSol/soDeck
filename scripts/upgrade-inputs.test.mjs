import { test } from "node:test";
import assert from "node:assert/strict";
import { upgradeInputs } from "./upgrade-inputs.mjs";

test("upgrade inputs pin one increasing component and a fixed host without historical constants", () => {
  const env = { SODECK_UPGRADE_TARGET: "aarch64-apple-darwin", SODECK_UPGRADE_COMPONENT: "core", SODECK_UPGRADE_FROM: "0.23.0", SODECK_UPGRADE_TO: "0.23.1" };
  assert.equal(upgradeInputs(env).version, "0.23.1");
  for (const value of [undefined, "latest", "v0.23.1", "0.023.1", "0.23.0", "0.22.9"]) assert.throws(() => upgradeInputs({ ...env, SODECK_UPGRADE_TO: value }));
  const plugin = { ...env, SODECK_UPGRADE_COMPONENT: "docker", SODECK_UPGRADE_CORE: "0.23.1", SODECK_UPGRADE_FROM: "0.1.2", SODECK_UPGRADE_TO: "0.1.3" };
  assert.equal(upgradeInputs(plugin).current, upgradeInputs(plugin).version);
  assert.throws(() => upgradeInputs({ ...plugin, SODECK_UPGRADE_CORE: undefined }));
  assert.throws(() => upgradeInputs({ ...env, SODECK_UPGRADE_TARGET: "unknown" }));
});
