import type { Snapshot, NetworkTotal } from "./core";
import { button, escape, icon, statusText } from "./ui";
import { format } from "date-fns";
import { date } from "./datetime";

export interface StatusViewState {
  paused: boolean;
  onlyErrors: boolean;
  updatedAt: number;
  networkStale: boolean;
}

export function health(s: Snapshot) {
  return s.storageErrors.length || s.plugins.some(p => p.status === "failed" || p.updateError)
    || s.network?.connections?.some(c => c.state === "failed") ? "需关注" : "正常";
}

export function byteSize(value: string | number) {
  const n = Number(value);
  return n < 1024 ? `${n.toFixed(0)} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KiB`
    : n < 1073741824 ? `${(n / 1048576).toFixed(1)} MiB` : `${(n / 1073741824).toFixed(2)} GiB`;
}

function needsAttention(s: Snapshot, serverId: string) {
  return s.network?.connections?.some(c => c.serverId === serverId && c.state === "failed")
    || s.network?.plugins.some(p => p.serverId === serverId && s.plugins.some(v => v.manifest.id === p.pluginId && (v.status === "failed" || v.updateError)));
}

function connectionState(s: Snapshot, serverId: string, stale: boolean) {
  if (stale) return { text: "状态待更新", kind: "unknown", endpoint: "" };
  const c = s.network?.connections?.find(c => c.serverId === serverId);
  return {
    text: c ? ({ connected: "已连接", connecting: "正在连接", failed: "连接失败", cancelled: "已取消", disconnected: "已断开" }[c.state] ?? "已断开") : "连接状态待更新",
    kind: c?.state === "connected" ? "good" : c?.state === "failed" ? "warning" : "unknown",
    endpoint: c?.endpoint ?? "",
  };
}

function networkCard(s: Snapshot, total: NetworkTotal, view: StatusViewState) {
  const state = connectionState(s, total.serverId, view.networkStale);
  const rows = s.network?.plugins.filter(p => p.serverId === total.serverId) ?? [];
  const rate = (value: number) => view.networkStale ? "待更新" : `${byteSize(value)}/s`;
  const reading = (name: string, direction: string, speed: number, bytes: string) =>
    `<div class="status-transfer"><span>${icon("arrow").replace('<svg ', `<svg class="status-${direction}" `)}${name}</span><strong>${rate(speed)}</strong><small>本次运行 ${byteSize(bytes)}</small></div>`;
  return `<article class="status-server" data-dom-key="status-server:${escape(total.serverId)}" data-dom-scope="status-server:${escape(total.serverId)}"><div class="status-server-heading"><span class="status-server-icon">${icon("server")}</span><div class="status-server-identity"><h3>${escape(total.name || total.serverId)}</h3><p>${state.endpoint ? escape(state.endpoint) : "本次运行传输记录"}</p></div><span class="status-connection ${state.kind}">${state.kind === "good" ? icon("check") : icon("info")}${state.text}</span></div>
    <div class="status-bandwidth">${reading("接收", "down", total.receivedPerSecond, total.received)}${reading("发送", "up", total.sentPerSecond, total.sent)}</div>
    <details data-network-server="${escape(total.serverId)}"><summary><span>插件流量 <small>${rows.length} 个来源</small></span>${icon("chevron")}</summary><div class="status-traffic-table"><table aria-label="${escape(total.name || total.serverId)} 插件有效载荷"><thead><tr><th>来源</th><th>接收</th><th>发送</th></tr></thead><tbody>${rows.map(p => {
      const source = s.plugins.find(v => v.manifest.id === p.pluginId);
      return `<tr><th scope="row">${escape(source?.manifest.name || p.pluginId)}${source?.status === "failed" ? '<small class="warning">已退出</small>' : ""}</th><td>${byteSize(p.received)}<small>${rate(p.receivedPerSecond)}</small></td><td>${byteSize(p.sent)}<small>${rate(p.sentPerSecond)}</small></td></tr>`;
    }).join("")}</tbody></table>${rows.length ? "" : '<p class="status-hint">尚无插件有效载荷记录。</p>'}</div></details></article>`;
}

export function status(s: Snapshot, view: StatusViewState = { paused: false, onlyErrors: false, updatedAt: 0, networkStale: false }) {
  const failed = s.plugins.filter(p => p.status === "failed" || p.updateError);
  const failedConnections = s.network?.connections?.filter(c => c.state === "failed") ?? [];
  const attention = health(s) !== "正常" || view.networkStale;
  const totals = s.network?.servers ?? [];
  const shown = view.onlyErrors ? totals.filter(t => needsAttention(s, t.serverId)) : totals;
  const working = s.plugins.filter(p => p.status === "starting" || p.status === "stopping").length;
  const issues = [s.storageErrors.length ? "日志或消息存储异常" : "", failed.length ? `${failed.length} 个插件需关注` : "", failedConnections.length ? `${failedConnections.length} 条连接失败` : "", view.networkStale ? "网络计量读取失败" : ""].filter(Boolean);
  const subtitle = issues.length ? issues.join("；") : s.connectingServers ? `${s.connectingServers} 条连接正在建立。` : s.connectedServers ? "应用、远程连接与插件运行正常。" : "应用与插件运行正常，目前没有远程连接。";
  const metric = (value: string | number, label: string, note: string) => `<div class="status-fact"><strong>${value}</strong><span>${label}</span><small>${escape(note)}</small></div>`;
  const appPair = (label: string, value: string) => `<div><dt>${label}</dt><dd>${escape(value)}</dd></div>`;
  return `<section class="status-overview" data-dom-key="status-overview" data-dom-scope="status-overview" aria-label="运行状态"><header class="status-page-heading"><div><h1>运行状态</h1><p>应用、连接与插件，一目了然。</p></div><div class="status-page-actions" data-dom-scope="status-actions">${button("pause-status", view.paused ? "恢复刷新" : "暂停刷新", undefined, "quiet", `id="pause-status" aria-pressed="${view.paused}"`)}<details class="status-menu" data-persist="status-actions"><summary aria-label="更多状态操作">${icon("more")}更多</summary><div>${button("status-about", "应用信息", "info", "quiet")}${button("copy-status", "复制状态", "copy", "quiet")}</div></details></div></header>
    <section class="status-health ${attention ? "has-warning" : ""}" aria-label="整体健康"><span class="status-health-icon">${icon(attention ? "info" : "check")}</span><div><h2>${attention ? "有事项需要关注" : "一切运行正常"}</h2><p>${escape(subtitle)}</p></div><span class="status-updated">${view.paused ? "刷新已暂停" : view.updatedAt ? `更新于 ${format(view.updatedAt, "HH:mm:ss")}` : ""}</span></section>
    <div class="status-facts">${metric(s.connectedServers ?? 0, "远程会话", s.connectingServers ? `${s.connectingServers} 正在连接` : "无正在建立的连接")}${metric(s.plugins.filter(p => p.status === "running").length, "运行插件", failed.length ? `${failed.length} 个插件需关注` : `${s.plugins.length} 个已安装`)}${metric(working, "正在处理", working ? "插件正在启动或停止" : "无启动或停止任务")}${metric(`${Math.floor(s.uptimeMs / 60000)}<em> 分钟</em>`, "运行时间", `${format(s.startedAt, "HH:mm:ss")} 启动`)}</div>
    ${s.storageErrors.map(error => `<div class="error-box" role="alert">${escape(error)}</div>`).join("")}${failed.map(p => `<div class="status-issue" data-dom-key="status-issue:${escape(p.manifest.id)}" data-dom-scope="status-issue:${escape(p.manifest.id)}"><div><h3>${escape(p.manifest.name)}${p.status === "failed" ? "运行失败" : "更新失败"}</h3><p>${escape(p.updateError ?? p.error ?? "查看插件详情以定位原因。")}</p></div>${button("locate-plugin", "查看插件", undefined, "quiet", `data-id="${escape(p.manifest.id)}"`)}</div>`).join("")}${failedConnections.map(c => `<div class="status-issue" data-dom-key="status-connection-issue:${escape(c.serverId)}" data-dom-scope="status-connection-issue:${escape(c.serverId)}"><div><h3>连接失败</h3><p>${escape(c.endpoint)}${c.error ? `：${escape(c.error.message)}` : ""}</p></div>${button("connections", "查看连接", "server", "quiet")}</div>`).join("")}
    <div class="status-work-grid" data-dom-key="status-work-grid"><section class="status-network"><header class="status-section-heading"><div><h2>服务器网络</h2><span>${shown.length} 个服务器</span></div><label class="status-filter"><input type="checkbox" id="status-errors" ${view.onlyErrors ? "checked" : ""}>只看异常</label></header>${shown.length ? shown.map(t => networkCard(s, t, view)).join("") : `<div class="status-empty">${icon(view.onlyErrors ? "check" : "server")}<h3>${view.onlyErrors ? "没有匹配的异常服务器" : view.networkStale ? "网络计量暂不可用" : "尚无服务器传输记录"}</h3><p>${view.onlyErrors ? "可取消筛选查看本次运行的全部传输记录。" : view.networkStale ? "恢复刷新后重新读取，远程会话不受暂停显示影响。" : "连接服务器后，这里会显示速率与插件流量。"}</p>${view.onlyErrors ? button("clear-status-filter", "查看全部服务器", undefined, "quiet") : ""}</div>`}</section>
    <aside class="status-side" data-dom-scope="status-side"><section><header class="status-section-heading"><h2>插件运行</h2><span>${s.plugins.filter(p => p.status === "running").length} / ${s.plugins.length}</span></header><div class="status-plugin-list">${s.plugins.map(p => button("locate-plugin", `<span>${escape(p.manifest.name)}</span><small class="${p.status === "failed" || p.updateError ? "warning" : ""}">${p.updateError ? "更新失败" : statusText[p.status]}${p.pendingRestart ? " · 待重启" : ""}</small>`, "plug", "status-plugin-row", `data-id="${escape(p.manifest.id)}"`)).join("") || '<p class="status-hint">尚未安装插件。</p>'}</div>${button("manage-plugins", "管理插件", "arrow", "quiet")}</section><section><h2>应用信息</h2><dl class="status-app-facts">${appPair("平台", `${s.os} ${s.arch}`)}${appPair("应用版本", s.version)}${appPair("启动时间", date(s.startedAt))}${appPair("日志与消息", s.storageErrors.length ? "存储异常" : "存储可用")}</dl>${button("status-about", "查看详细信息", "arrow", "quiet")}</section></aside></div>
    <footer class="status-footer" data-dom-key="status-footer" data-dom-scope="status-footer"><details data-persist="status-note"><summary>${icon("info")}统计说明</summary><p>${escape(s.network?.note ?? "网络计量尚未取得。")}</p><p>连接传输与插件有效载荷使用不同口径，不直接相减计算未归属流量；累计记录在断线后保留。</p></details><div>${button("logs", "查看日志", "log", "quiet")}${button("copy-status", "复制状态", "copy", "quiet")}</div></footer></section>`;
}
