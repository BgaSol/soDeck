import assert from "node:assert/strict";
import { targets } from "./core-artifacts.mjs";

export function upgradeInputs(env) {
  const target = env.SODECK_UPGRADE_TARGET, component = env.SODECK_UPGRADE_COMPONENT;
  const from = env.SODECK_UPGRADE_FROM, to = env.SODECK_UPGRADE_TO;
  const valid = v => /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(v ?? "");
  assert(targets.includes(target), "Provide a supported native target");
  assert(["core", "terminal", "files", "network-proxy", "system-status", "docker"].includes(component), "Select one component");
  assert(valid(from) && valid(to), "Provide exact old/new versions");
  const oldParts = from.split(".").map(BigInt), newParts = to.split(".").map(BigInt);
  const changed = newParts.findIndex((value, index) => value !== oldParts[index]);
  assert(changed >= 0 && newParts[changed] > oldParts[changed], "New version must be greater than old version");
  const current = component === "core" ? from : env.SODECK_UPGRADE_CORE;
  assert(valid(current), "Plugin upgrades require a fixed core version");
  const discovery = env.SODECK_UPGRADE_DISCOVERY ?? "false";
  assert(["true", "false"].includes(discovery), "Discovery verification must be true or false");
  return { target, component, from, to, current, version: component === "core" ? to : current, verifyDiscovery: discovery === "true" };
}
