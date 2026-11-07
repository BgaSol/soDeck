import type { Plugin, Snapshot, Update } from "./core";
import type { ViewState } from "./views";
import { button as b, date, escape as e, icon } from "./ui";
import { renderReleaseNotes } from "./release-notes";

export const CORE_ID = "sodeck.core";
export const updateWorking = (u?: Update) => !!u && ["checking", "waiting", "downloading", "preparing"].includes(u.phase);
export const canDownload = (u?: Update) => !!u?.version && (u.phase === "available" || (u.phase === "error" && u.errorStage !== "check"));
export function pluginUpdate(p: Plugin, v: ViewState): Update {
  const u = v.updates[p.manifest.id];
  if (p.pendingVersion) return { ...u, id: p.manifest.id, phase: "ready", version: p.pendingVersion, notes: u?.notes ?? "", downloadedBytes: u?.downloadedBytes ?? 0, totalBytes: u?.totalBytes ?? null, error: null };
  return u ?? { id: p.manifest.id, phase: p.manifest.updateSource ? "idle" : "local", version: null, notes: "", downloadedBytes: 0, totalBytes: null, error: null };
}
function items(s: Snapshot, v: ViewState) {
  return [{ id: CORE_ID, name: "SoDeck", kind: "应用", version: s.version, target: s.target, symbol: "grid", update: v.updates[CORE_ID] }, ...s.plugins.map(p => ({ id: p.manifest.id, name: p.manifest.name, kind: "插件", version: p.manifest.version, target: p.manifest.target, symbol: "plug", update: pluginUpdate(p, v) }))];
}
export function downloadableIds(s: Snapshot, v: ViewState) { return items(s, v).filter(p => canDownload(p.update)).map(p => p.id); }
export function updateCount(s: Snapshot, v: ViewState) { return downloadableIds(s, v).length; }
export function selectedUpdates(s: Snapshot, v: ViewState) { return downloadableIds(s, v).filter(id => !v.excludedUpdates.has(id)); }
export function updateAction(id: string, u?: Update, blocked = false) {
  const attrs = `data-id="${e(id)}" id="update-action-${e(id)}" ${blocked ? "disabled" : ""}`;
  if (canDownload(u)) return b("download-update", u?.phase === "error" ? "重试下载" : "下载更新", undefined, "quiet", attrs);
  if (u?.phase === "error" || u?.phase === "incompatible") return b("check-update", "重试检查", undefined, "text-button", attrs);
  if (!u || u.phase === "idle") return b("check-update", "检查更新", undefined, "text-button", attrs);
  if (u.phase === "unsupported") return b("open-update-release", "前往下载 DEB", undefined, "quiet");
  return "";
}
export function updateStatus(u?: Update) {
  const labels: Record<string, string> = { idle: "尚未检查", checking: "正在检查", waiting: "等待下载", downloading: "正在下载", preparing: "正在校验和准备", current: "已是最新", available: "有新版本", ready: "重启后应用", error: u?.errorStage === "check" ? "暂时无法检查" : "更新未完成", incompatible: "新版不适用于当前环境", unsupported: "使用系统安装方式更新", local: "手动管理" };
  return labels[u?.phase ?? "idle"] ?? "尚未检查";
}
export function updateNotes(id: string, u?: Update) {
  if (!u?.notes.trim()) return "";
  return `<details class="update-release-notes" data-persist="notes-${e(id)}"><summary id="update-notes-${e(id)}">更新说明</summary><div class="release-notes-markdown">${renderReleaseNotes(u.notes)}</div></details>`;
}
function updateRow(p: ReturnType<typeof items>[number], selectable: boolean, v: ViewState) {
  const u = p.update, downloading = u?.phase === "downloading", busy = updateWorking(u);
  const progress = downloading ? `<div class="update-progress"><progress aria-label="${e(p.name)}下载进度" ${u.totalBytes ? `value="${u.downloadedBytes}" max="${u.totalBytes}"` : ""}></progress><span>${(u.downloadedBytes / 1048576).toFixed(1)} MiB${u.totalBytes ? ` / ${(u.totalBytes / 1048576).toFixed(1)} MiB` : ""}</span></div>` : "";
  return `<article class="update-row" data-update-id="${e(p.id)}" id="update-item-${e(p.id)}" tabindex="-1">${selectable ? `<label class="update-select">${canDownload(u) ? `<input type="checkbox" id="update-select-${e(p.id)}" data-update-select="${e(p.id)}" aria-label="选择更新 ${e(p.name)}" ${!v.excludedUpdates.has(p.id) ? "checked" : ""}>` : ""}</label>` : ""}<div class="maintenance-symbol">${icon(p.symbol)}</div><div class="update-content"><div class="maintenance-name"><b>${e(p.name)}</b><span>${p.kind}</span></div><p class="maintenance-meta">${e(p.version)}${u?.version && u.version !== p.version ? ` <span aria-hidden="true">→</span> <strong>${e(u.version)}</strong>` : ""} · <span class="update-state ${u?.phase === "error" || u?.phase === "incompatible" ? "update-error" : u?.phase === "ready" || u?.phase === "current" ? "update-good" : ""}" role="status">${updateStatus(u)}</span></p>${progress}${u?.error ? `<details class="update-error-detail" data-persist="error-${e(p.id)}"><summary>查看原因</summary><p role="alert">${e(u.error)}</p></details>` : ""}${u?.phase === "local" ? '<p class="maintenance-meta">在插件详情中配置更新来源。</p>' : ""}${u?.phase === "unsupported" ? `<p class="maintenance-meta">当前架构 ${e(p.target)} · DEB 由系统安装方式升级，插件仍可在线更新。</p>` : ""}</div><div class="update-row-actions">${!busy ? updateAction(p.id, u, v.updateOverview.busy || v.updateOverview.checking) : ""}${u?.phase === "local" ? b("locate-plugin", "配置来源", undefined, "text-button", `data-id="${e(p.id)}"`) : ""}</div>${updateNotes(p.id, u)}</article>`;
}
export function updatesPage(s: Snapshot, v: ViewState) {
  const all = items(s, v), available = downloadableIds(s, v), selected = selectedUpdates(s, v);
  const busy = v.updateOverview.checking || v.updateOverview.busy || v.busy.has("updates");
  const failed = all.filter(p => ["error", "incompatible"].includes(p.update?.phase ?? ""));
  const ready = all.filter(p => p.update?.phase === "ready");
  const working = all.filter(p => updateWorking(p.update));
  const checkable = all.filter(p => !["local", "unsupported"].includes(p.update?.phase ?? ""));
  const attention = (u?: Update) => !!u && ["available", "waiting", "downloading", "preparing", "ready", "error", "incompatible"].includes(u.phase);
  const plugins = all.slice(1).sort((a,b) => Number(attention(b.update)) - Number(attention(a.update)));
  const summary = v.updateOverview.checking ? "正在检查应用与插件…" : working.length ? `${ready.length} 项已准备好 · ${working.length} 项正在处理` : failed.length ? `${available.length} 项可更新 · ${failed.length} 项需要处理` : available.length ? `有 ${available.length} 项更新可下载` : ready.length ? `${ready.length} 项已准备好，重启后应用` : checkable.length && checkable.every(p => p.update?.phase === "current") ? "可在线检查的项目已是最新" : !checkable.length ? "当前项目需通过安装来源更新" : "检查是否有新版本";
  return `<div class="section-heading maintenance-heading"><div><h2>软件更新</h2><p>统一更新 SoDeck 和已安装的插件。</p></div><div class="actions">${b("check-all-updates", v.updateOverview.checking ? "正在检查…" : "检查更新", "restart", "quiet", `id="check-all-updates" ${busy ? "disabled" : ""}`)}${available.length ? b("download-all-updates", selected.length === available.length ? "下载全部更新" : `下载所选（${selected.length}）`, "download", "primary", selected.length && !busy && !v.busy.has("downloads") ? "" : "disabled") : ""}</div></div><div class="update-summary"><p role="status">${summary}</p><span>${v.updateOverview.lastCheckedAt ? `最近检查 ${e(date(v.updateOverview.lastCheckedAt))}` : "尚未完成检查"}</span></div>${v.error ? `<p class="error-box" role="alert">${e(v.error)}</p>` : ""}<section class="update-group" aria-label="SoDeck 应用"><div class="maintenance-group-heading"><h3>SoDeck 应用</h3><span>当前版本 ${e(s.version)}</span></div>${updateRow(all[0], available.length > 1, v)}</section><section class="update-group" aria-label="插件更新"><div class="maintenance-group-heading"><h3>已安装插件</h3><span>${s.plugins.length} 个</span></div>${plugins.map(p=>updateRow(p, available.length > 1, v)).join("") || '<p class="plugin-list-intro">安装插件后，可在这里检查和下载它们的更新。</p>'}</section><div class="maintenance-footer"><label class="automatic-updates"><input id="automatic-updates" type="checkbox" ${s.preferences.automaticUpdates !== false ? "checked" : ""}>每天自动检查更新</label><span>只检查，不自动下载或重启</span></div>`;
}
export function pendingUpdates(s: Snapshot, v: ViewState) {
  const pending = s.plugins.filter(p => p.pendingRestart);
  const core = v.updates[CORE_ID]?.phase === "ready";
  if (!pending.length && !core && !s.restartRequired) return "";
  const count = pending.length + (core ? 1 : 0);
  const blocked = v.updateOverview.checking || v.updateOverview.busy || v.busy.has("restart");
  const restartLabel = v.updates[CORE_ID]?.errorStage === "install" ? "重试安装并重启" : "重启并应用";
  return `<div class="maintenance-pending" role="status"><div><b>${count ? `${count} 项变更等待重启` : "有变更等待重启"}</b><p>完成手头工作后，再统一重启应用。当前连接和任务会结束。</p><details data-persist="pending-updates"><summary>待应用清单</summary><ul>${core ? `<li>SoDeck → ${e(v.updates[CORE_ID]?.version)}</li>` : ""}${pending.map(p=>`<li>${e(p.manifest.name)}${p.pendingVersion ? ` → ${e(p.pendingVersion)}` : ""} · 重启后${p.enabled ? "启用" : "关闭"}</li>`).join("")}</ul></details></div>${b("restart-app", blocked ? "等待处理完成" : restartLabel, "restart", "primary", blocked ? "disabled" : "")}</div>`;
}
