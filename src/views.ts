import type { Snapshot, Plugin, Log, Message, Filter } from "./core";
import { button as b, date, empty, escape as e, icon, statusText } from "./ui";

export interface ViewState {
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
}
const pair = (name: string, value: unknown) =>
  `<div class="pair"><dt>${name}</dt><dd>${e(value)}</dd></div>`;
export function health(s: Snapshot) {
  return s.storageErrors.length || s.plugins.some((p) => p.status === "failed")
    ? "需关注"
    : "正常";
}

export function status(s: Snapshot) {
  const failed = s.plugins.filter((p) => p.status === "failed");
  return `<div class="status-summary"><span class="health-dot ${health(s) === "正常" ? "good" : "warning"}"></span><div><h3>SoDeck ${health(s) === "正常" ? "运行正常" : "需要关注"}</h3><p>应用与插件的当前状态</p></div><span class="version">v${e(s.version)}</span></div>
    <div class="metrics"><div><strong>${s.plugins.filter((p) => p.status === "running").length}</strong><span>运行插件</span></div><div><strong>${s.plugins.filter((p) => ["starting", "stopping"].includes(p.status)).length}</strong><span>正在处理</span></div><div><strong>${failed.length}</strong><span>异常插件</span></div></div>
    <dl>${pair("平台", `${s.os} · ${s.arch}`)}${pair("运行时间", `${Math.floor(s.uptimeMs / 3600000)} 小时 ${Math.floor(s.uptimeMs / 60000) % 60} 分钟`)}${pair("启动时间", date(s.startedAt))}${pair("日志与消息", s.storageErrors.length ? "存储异常" : "存储可用")}${pair("数据目录", s.dataDirectory)}</dl>
    ${s.storageErrors.map((error) => `<p class="error-box">${e(error)}</p>`).join("")}${failed.map((p) => `<div class="error-box"><b>${e(p.manifest.name)}</b><p>${e(p.error)}</p>${b("locate-plugin", "查看插件", undefined, "text-button", `data-id="${e(p.manifest.id)}"`)}</div>`).join("")}
    <div class="panel-footer">${b("logs", "查看日志", "log")}${b("copy-status", "复制状态", "copy", "quiet")}</div>`;
}

export function settings(s: Snapshot, v: ViewState) {
  const nav = [
    ["plugins", "插件管理", "plug"],
    ["appearance", "外观", "sun"],
    ["about", "应用与数据", "info"],
  ];
  return `<aside class="settings-nav"><div class="nav-label">偏好设置</div>${nav.map(([id, label, i]) => b("section", label, i, id === v.section ? "nav-item active" : "nav-item", `data-id="${id}" aria-current="${id === v.section ? "page" : "false"}"`)).join("")}<div class="nav-bottom">SoDeck <span>v${e(s.version)}</span></div></aside><section class="settings-page" data-scroll="settings">${v.section === "plugins" ? plugins(s, v) : v.section === "appearance" ? appearance(s) : about(s)}</section>`;
}

function plugins(s: Snapshot, v: ViewState) {
  const p = s.plugins.find((p) => p.manifest.id === v.plugin);
  return `<div class="section-heading"><div><h2>插件管理</h2><p>从本机目录加载，随时加载或卸载。</p></div>${b("choose-plugin", "加载插件目录", "folder", "primary", v.busy.has("choose") ? "disabled" : "")}</div>${v.error ? `<div role="alert" class="error-box">${e(v.error)}</div>` : ""}
    ${s.plugins.length ? `<div class="plugin-layout ${p ? "has-selection" : ""}"><div class="plugin-list" data-scroll="plugin-list">${s.plugins.map((p) => pluginRow(p, v)).join("")}</div><div class="plugin-detail" data-scroll="plugin-detail">${p ? detail(p, v) : empty("选择一个插件", "在左侧查看运行状态、路径和日志。")}</div></div>` : empty("尚未加载插件", "选择已解压的插件目录开始。插件独立下载，核心安装包不包含插件。")}`;
}
function pluginRow(p: Plugin, v: ViewState) {
  return `<button data-action="select-plugin" data-id="${e(p.manifest.id)}" class="plugin-row ${p.manifest.id === v.plugin ? "selected" : ""}"><div class="plugin-symbol">${icon("plug")}</div><div><b>${e(p.manifest.name)}</b><small>${e(p.manifest.id)}</small><span class="badge ${p.status}">${statusText[p.status]}</span></div><span class="plugin-version">${e(p.manifest.version)}</span></button>`;
}
function detail(p: Plugin, v: ViewState) {
  const active = !p.cleanupComplete;
  const busy =
    v.busy.has(p.manifest.id) || ["starting", "stopping"].includes(p.status);
  return `${b("back-plugins", "返回列表", undefined, "text-button back-plugins")}<div class="detail-heading"><div class="plugin-symbol large">${icon("plug")}</div><h3>${e(p.manifest.name)}</h3><span class="badge ${p.status}">${statusText[p.status]}</span></div>
    <div class="actions">${b(active ? "unload-plugin" : "load-plugin", busy ? statusText[p.status] : active ? "卸载" : p.status === "failed" ? "重新加载" : "加载", undefined, active ? "" : "primary", `${busy ? "disabled" : ""} data-id="${e(p.manifest.id)}"`)}${b("plugin-logs", "查看日志", "log", "quiet", `data-id="${e(p.manifest.id)}"`)}</div><p class="hint">卸载会停止插件，保留插件文件和数据。</p>
    ${p.error ? `<div class="error-box" role="alert">${e(p.error)}</div>` : ""}<dl>${pair("插件 ID", p.manifest.id)}${pair("版本", p.manifest.version)}${pair("核心 API", p.manifest.apiVersion)}${pair("适用平台", p.manifest.target)}${pair("插件目录", p.directory)}${pair("入口", p.manifest.entry)}${p.pid ? pair("进程", p.pid) : ""}${p.startedAt ? pair("启动时间", date(p.startedAt)) : ""}${p.instanceId ? pair("实例", p.instanceId) : ""}</dl>
    <div class="detail-bottom">${b("forget-plugin", "移除记录", undefined, "quiet danger", `${active || busy ? "disabled" : ""} data-id="${e(p.manifest.id)}"`)}<span class="hint">不会删除磁盘文件</span></div>`;
}
function appearance(s: Snapshot) {
  return `<div class="section-heading"><div><h2>外观</h2><p>选择适合当前环境的显示主题。</p></div></div><h3 class="field-title">应用主题</h3><div class="themes">${[
    ["system", "跟随系统"],
    ["light", "浅色"],
    ["dark", "深色"],
  ]
    .map(
      ([id, label]) =>
        `<button data-action="theme" data-id="${id}" aria-pressed="${s.preferences.theme === id}" class="theme-option ${s.preferences.theme === id ? "selected" : ""}"><div class="theme-preview ${id}"><i></i><div><span></span><span></span></div></div><span>${label}</span>${s.preferences.theme === id ? icon("check") : ""}</button>`,
    )
    .join("")}</div><p class="hint">立即生效，重新打开应用后保留。</p>`;
}
function about(s: Snapshot) {
  return `<div class="section-heading"><div><h2>应用与数据</h2><p>应用信息和本地数据位置。</p></div></div><div class="about-brand"><div class="brand-mark">S</div><div><h3>SoDeck</h3><p>面向运维的远程服务器工作台</p></div></div><dl>${pair("应用版本", s.version)}${pair("构建目标", s.target)}${pair("数据目录", s.dataDirectory)}${pair("日志目录", s.logDirectory)}</dl><div class="actions">${b("copy-data", "复制数据路径", "copy")}${b("copy-logs", "复制日志路径", "copy", "quiet")}</div><p class="hint">插件文件保留在你选择的外部目录。卸载或覆盖安装核心不会主动清理这些数据。</p>`;
}

export function logs(s: Snapshot, v: ViewState) {
  return `<div class="log-toolbar"><label>来源<select id="log-source"><option value="">全部来源</option><option value="core" ${v.filter.pluginId === "core" ? "selected" : ""}>核心</option>${s.plugins.map((p) => `<option value="${e(p.manifest.id)}" ${v.filter.pluginId === p.manifest.id ? "selected" : ""}>${e(p.manifest.name)}</option>`).join("")}${v.filter.pluginId && v.filter.pluginId !== "core" && !s.plugins.some((p) => p.manifest.id === v.filter.pluginId) ? `<option selected value="${e(v.filter.pluginId)}">${e(v.filter.pluginId)}</option>` : ""}</select></label><label>级别<select id="log-level"><option value="">全部级别</option>${["debug", "info", "warn", "error"].map((l) => `<option ${v.filter.level === l ? "selected" : ""}>${l}</option>`).join("")}</select></label><label class="search-label">搜索<input id="log-query" type="search" placeholder="搜索日志正文" value="${e(v.filter.query)}"></label>${b("pause-logs", v.paused ? "继续显示" : "暂停显示", undefined, "quiet")}${b("copy-logs-content", "复制记录", "copy", "quiet")}</div><div class="log-times"><label>开始 <input id="log-since" type="datetime-local" value="${localTime(v.filter.since)}"></label><label>结束 <input id="log-until" type="datetime-local" value="${localTime(v.filter.until)}"></label><span class="hint">最近 500 条 · ${v.paused ? "显示已暂停，后台继续记录" : "实时更新"}</span></div><div id="log-results" class="log-results" data-scroll="logs">${logResults(v.logs)}</div>`;
}
function localTime(value?: number) {
  if (!value) return "";
  const d = new Date(value);
  return new Date(value - d.getTimezoneOffset() * 60000)
    .toISOString()
    .slice(0, 16);
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
  return `<div class="messages-toolbar"><div class="segmented">${b("all-messages", "全部", undefined, v.unread ? "" : "selected")}${b("unread-messages", "未读", undefined, v.unread ? "selected" : "")}</div>${b("read-all", "全部标为已读", undefined, "quiet")}</div><div class="messages-layout"><div class="message-list" data-scroll="messages">${list.length ? list.map((m) => `<button data-action="select-message" data-id="${e(m.id)}" class="message-row ${m.id === v.selectedMessage ? "selected" : ""} ${m.read ? "" : "unread"}"><div><span class="badge ${e(m.level)}">${({ info: "信息", success: "成功", warning: "注意", error: "错误" } as Record<string, string>)[m.level] ?? e(m.level)}</span><time>${e(date(m.receivedAt))}</time></div><h3>${e(m.title)}${m.count > 1 ? ` <small>×${m.count}</small>` : ""}</h3><p>${e(m.body)}</p><small>${e(m.pluginId ?? "SoDeck 核心")}</small></button>`).join("") : empty(v.messages.length ? "没有未读消息" : "暂无消息", "插件发布的重要消息会保留在这里。", "bell")}</div><div class="message-detail" data-scroll="message-detail">${selected ? `<h2>${e(selected.title)}</h2><p class="hint">${e(date(selected.receivedAt))} · ${selected.count} 次</p><p class="message-body">${e(selected.body)}</p><dl>${pair("来源", selected.source === "core" ? "核心" : "插件")}${pair("插件", selected.pluginId ?? "—")}${pair("实例", selected.instanceId ?? "—")}</dl>${selected.pluginId ? b("locate-plugin", "查看插件", "plug", "quiet", `data-id="${e(selected.pluginId)}"`) : ""}` : empty("消息详情", "选择一条消息查看完整内容。", "bell")}</div></div>`;
}
