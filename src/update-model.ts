import type { Plugin, Snapshot, Update } from './core';
import { packageIcon } from './plugin-icons';
type UpdateState = { updates: Record<string, Update>; excludedUpdates: Set<string> };

export const CORE_ID = "sodeck.core";
export const updateWorking = (u?: Update) => !!u && ["checking", "waiting", "downloading", "preparing"].includes(u.phase);
export const canDownload = (u?: Update) => !!u?.version && (u.phase === "available" || (u.phase === "error" && u.errorStage !== "check"));
export function pluginUpdate(p: Plugin, v: UpdateState): Update {
  const u = v.updates[p.manifest.id];
  return u ?? { id: p.manifest.id, phase: p.manifest.updateSource ? "idle" : "local", version: null, notes: "", downloadedBytes: 0, totalBytes: null, error: null };
}
export function updateItems(s: Snapshot, v: UpdateState) {
  return [{ id: CORE_ID, name: "SoDeck", kind: "应用", version: s.version, target: s.target, symbol: "grid", update: v.updates[CORE_ID] }, ...s.plugins.map(p => ({ id: p.manifest.id, name: p.manifest.name, kind: "插件", version: p.manifest.version, target: p.manifest.target, symbol: packageIcon(p.manifest), update: pluginUpdate(p, v) }))];
}
export function downloadableIds(s: Snapshot, v: UpdateState) { return updateItems(s, v).filter(p => canDownload(p.update)).map(p => p.id); }
export function updateCount(s: Snapshot, v: UpdateState) { return downloadableIds(s, v).length; }
export function selectedUpdates(s: Snapshot, v: UpdateState) { return downloadableIds(s, v).filter(id => !v.excludedUpdates.has(id)); }

export function updateNotice(s: Snapshot, v: UpdateState) {
  const count = updateCount(s, v);
  const pending = s.restartRequired || v.updates[CORE_ID]?.phase === 'ready';
  return { count, pending, label: [count ? '更新 ' + count : '', pending ? '待重启' : ''].filter(Boolean).join(' · ') };
}
