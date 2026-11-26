import type { Snapshot, Log, Message, Filter, Update, UpdateOverview, Manifest, OfficialPlugin, OfficialPreparation, UpdateNetwork, MirrorDiagnostic } from "./core";
import { updatesPage, pendingUpdates } from "./updates";
import { pluginsPage } from "./plugin-settings";
import { button as b, empty, escape as e, icon, statusText } from "./ui";
import { date, localTime } from "./datetime";
export { status, health, byteSize } from "./status";

export interface ViewState {
  officialPlugins: OfficialPlugin[];
  officialFetchedAt: number;
  officialLoading: boolean;
  officialError: string;
  officialPreparation?: OfficialPreparation | null;
  plugin: string;
  messages: Message[];
  unread: boolean;
  selectedMessage: string;
  logs: Log[];
  filter: Filter;
  paused: boolean;
  error: string;
  busy: Set<string>;
  updates: Record<string, Update>;
  updateOverview: UpdateOverview;
  excludedUpdates: Set<string>;
  pluginTab: "installed" | "official" | null;
  sourceDrafts: Record<string, NonNullable<Manifest["updateSource"]>>;
  networkDraft?: UpdateNetwork;
  networkMirrorKeys?: { revision: number; keys: string[] };
  networkError?: string;
  networkNotice?: string;
  networkTesting?: string;
  networkResults?: { endpoint: string; results: MirrorDiagnostic[] };
  networkGeneration?: number;
}
const pair = (name: string, value: unknown) => `<div class="pair"><dt>${name}</dt><dd>${e(value)}</dd></div>`;
export function maintenance(s:Snapshot, v:ViewState, page:"plugins"|"updates") {
  return `<div class="maintenance-main"><section class="maintenance-page" data-scroll="${page}">${page === "plugins" ? pluginsPage(s,v) : updatesPage(s,v)}</section>${pendingUpdates(s,v)}</div>`;
}
export function logs(s: Snapshot, v: ViewState) {
  return `<div class="log-toolbar"><label>来源<select id="log-source"><option value="">全部来源</option><option value="core" ${v.filter.pluginId === "core" ? "selected" : ""}>核心</option>${s.plugins.map((p) => `<option value="${e(p.manifest.id)}" ${v.filter.pluginId === p.manifest.id ? "selected" : ""}>${e(p.manifest.name)}</option>`).join("")}${v.filter.pluginId && v.filter.pluginId !== "core" && !s.plugins.some((p) => p.manifest.id === v.filter.pluginId) ? `<option selected value="${e(v.filter.pluginId)}">${e(v.filter.pluginId)}</option>` : ""}</select></label><label>级别<select id="log-level"><option value="">全部级别</option>${["debug", "info", "warn", "error"].map((l) => `<option ${v.filter.level === l ? "selected" : ""}>${l}</option>`).join("")}</select></label><label class="search-label">搜索<input id="log-query" type="search" placeholder="搜索日志正文" value="${e(v.filter.query)}"></label>${b("clear-log-filters", "清除筛选", undefined, "quiet", 'id="clear-log-filters"')}${b("pause-logs", v.paused ? "继续显示" : "暂停显示", undefined, "quiet")}${b("copy-logs-content", "复制记录", "copy", "quiet")}</div><div class="log-times"><label>开始 <input id="log-since" type="datetime-local" value="${localTime(v.filter.since)}"></label><label>结束 <input id="log-until" type="datetime-local" value="${localTime(v.filter.until)}"></label><span class="hint">最近 500 条 · ${v.paused ? "显示已暂停，后台继续记录" : "实时更新"}</span></div><div id="log-results" class="log-results" data-scroll="logs">${logResults(v.logs)}</div>`;
}
export function logResults(records: Log[]) {
  return records.length
    ? records
        .map(
          (l) =>
            `<article class="log-row"><time>${e(date(l.receivedAt))}</time><span class="log-level ${e(l.level)}">${e(l.level)}</span><div><div class="log-meta">${e(l.source === "core" ? "核心" : "插件")} · ${e(l.pluginId ?? "SoDeck")}${l.instanceId ? ` · ${e(l.instanceId)}` : ""}</div><pre>${e(l.message)}${l.fields ? `\n${e(JSON.stringify(l.fields))}` : ""}</pre></div></article>`,
        )
        .join("")
    : empty("没有匹配的日志", "尝试调整来源、级别、时间或搜索条件。", "log");
}

export function messages(v: ViewState) {
  const list = v.messages.filter((m) => !v.unread || !m.read);
  const selected = v.messages.find((m) => m.id === v.selectedMessage);
  return `<div class="messages-toolbar"><div class="segmented">${b("all-messages", "全部", undefined, v.unread ? "" : "selected", `id="messages-all" aria-pressed="${!v.unread}"`)}${b("unread-messages", "未读", undefined, v.unread ? "selected" : "", `id="messages-unread" aria-pressed="${v.unread}"`)}</div>${b("read-all", "全部标为已读", undefined, "quiet")}</div><div class="messages-layout"><div class="message-list" data-scroll="messages">${list.length ? list.map((m) => `<button data-action="select-message" id="message-${e(m.id)}" data-id="${e(m.id)}" data-motion="item" class="message-row ${m.id === v.selectedMessage ? "selected" : ""} ${m.read ? "" : "unread"}"><div><span class="badge ${e(m.level)}">${({ info: "信息", success: "成功", warning: "注意", error: "错误" } as Record<string, string>)[m.level] ?? e(m.level)}</span><time>${e(date(m.receivedAt))}</time></div><h3>${e(m.title)}${m.count > 1 ? ` <small>×${m.count}</small>` : ""}</h3><p>${e(m.body)}</p><small>${e(m.pluginId ?? "SoDeck 核心")}</small></button>`).join("") : empty(v.messages.length ? "没有未读消息" : "暂无消息", v.messages.length ? "所有重要消息均已读，可切换到全部查看。" : "核心和插件的重要消息会保留在这里。", "bell")}</div><div class="message-detail" data-scroll="message-detail">${selected ? `<article data-motion="reveal" data-dom-key="detail:${e(selected.id)}"><h2>${e(selected.title)}</h2><p class="hint">${e(date(selected.receivedAt))} · ${selected.count} 次</p><p class="message-body">${e(selected.body)}</p><dl>${pair("来源", selected.source === "core" ? "核心" : "插件")}${pair("插件", selected.pluginId ?? "—")}${pair("实例", selected.instanceId ?? "—")}</dl>${selected.pluginId ? b("locate-plugin", "查看插件", "plug", "quiet", `data-id="${e(selected.pluginId)}"`) : ""}</article>` : empty("消息详情", "选择一条消息查看完整内容。", "bell")}</div></div>`;
}
