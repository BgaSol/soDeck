import { themes } from './theme';
import type { Snapshot, Log, Message, Filter, Update, UpdateOverview, Manifest, OfficialPlugin, UpdateNetwork, MirrorDiagnostic } from "./core";
import { updatesPage, pendingUpdates, updateCount } from "./updates";
import { pluginsPage } from "./plugin-settings";
import { button as b, empty, escape as e, icon, statusText } from "./ui";
import { date, localTime } from "./datetime";
export { status, health, byteSize } from "./status";

export interface ViewState {
  officialPlugins: OfficialPlugin[];
  officialFetchedAt: number;
  officialLoading: boolean;
  officialError: string;
  section: string;
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
const pair = (name: string, value: unknown) =>
  `<div class="pair"><dt>${name}</dt><dd>${e(value)}</dd></div>`;
export function settings(s: Snapshot, v: ViewState, componentNavigation = "") {
  const nav = (id: string, label: string, symbol: string) => b("section",label,symbol,id === v.section ? "nav-item active" : "nav-item",`id="settings-nav-${id}" data-id="${id}" aria-current="${id === v.section ? "page" : "false"}"`);
  return `<aside class="settings-nav" aria-label="设置导航">${nav("appearance","外观","sun")}${componentNavigation}${nav("about","应用与数据","info")}<div class="nav-bottom">SoDeck <span>v${e(s.version)}</span></div></aside><div class="settings-main"><section class="settings-page" data-scroll="settings" data-settings-section="${e(v.section)}" ${v.section === "components" ? 'id="component-settings-page"' : ""}>${v.section === "components" ? "" : v.section === "plugins" ? pluginsPage(s,v) : v.section === "appearance" ? appearance(s) : v.section === "updates" ? updatesPage(s,v) : about(s)}</section>${["plugins","updates"].includes(v.section) ? pendingUpdates(s,v) : ""}</div>`;
}
function appearance(s: Snapshot) {
  return `<div class="section-heading"><div><h2>外观</h2><p>选择适合当前环境的显示主题。</p></div></div><h3 class="field-title">应用主题</h3><div class="themes">${[["system", "跟随系统"], ...themes.map(t => [t.id, t.name])]
    .map(
      ([id, label]) =>
        `<button id="theme-${id}" data-action="theme" data-id="${id}" aria-pressed="${s.preferences.theme === id}" class="theme-option ${s.preferences.theme === id ? "selected" : ""}"><div class="theme-preview ${id}"><i></i><div><span></span><span></span></div></div><span>${label}</span>${s.preferences.theme === id ? icon("check") : ""}</button>`,
    )
    .join("")}</div><p class="hint">立即生效，重新打开应用后保留。</p>`;
}
function about(s: Snapshot) {
  return `<div class="section-heading"><div><h2>应用与数据</h2><p>应用信息和本地数据位置。</p></div></div><div class="about-brand"><div class="brand-mark">S</div><div><h3>SoDeck</h3><p>面向运维的远程服务器工作台</p></div></div><dl>${pair("应用版本", s.version)}${pair("构建目标", s.target)}${pair("数据目录", s.dataDirectory)}${pair("日志目录", s.logDirectory)}</dl><div class="actions">${b("usage-help", "使用帮助", "info")}${b("copy-data", "复制数据路径", "copy")}${b("copy-logs", "复制日志路径", "copy", "quiet")}</div><p class="hint">插件文件保留在你选择的外部目录。卸载或覆盖安装核心不会主动清理这些数据。</p>`;
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
