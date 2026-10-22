import {
  endpoint,
  forwardActive,
  type ForwardSettings,
  type ForwardView,
} from "./portForwardApi";
import type { Server } from "./types";

/** All running UI uses one snapshot, including searches and server scope. */
export function forwardPresentation(view: ForwardView, servers: Server[]) {
  const active = forwardActive(view);
  const settings = active ? view.runtime!.settings : view.rule.settings;
  const server = servers.find((s) => s.id === settings.serverId);
  const jump = servers.find((s) => s.id === server?.jumpServerId);
  const hops =
    active && view.runtime?.connection
      ? view.runtime.connection.hops.map((h) => h.server)
      : [...(jump ? [jump] : []), ...(server ? [server] : [])];
  const address = active ? view.runtime?.actualAddress : null;
  const local = address || localDescription(settings);
  const proxy = settings.service.kind === "networkProxy";
  const target = settings.service.kind === "portForward" ? endpoint(settings.service.targetHost, settings.service.targetPort) : "现场网络 · 由请求指定目标";
  const serverName = hops.at(-1)?.name || server?.name || "未配置服务器";
  return {
    settings,
    proxy,
    hops,
    local,
    target,
    serverName,
    address,
    copyable: view.runtime?.phase === "ready" && !!address,
    search: [
      view.rule.name,
      local,
      target,
      serverName,
      proxy ? "内网代理 HTTP SOCKS5" : "固定端口转发",
      ...hops.map((s) => s.host),
    ]
      .join(" ")
      .toLowerCase(),
  };
}

export function localDescription(settings: ForwardSettings) {
  return settings.listenPort === null
    ? "启动后自动分配"
    : endpoint(settings.listenAddress, settings.listenPort);
}
export function sameSettings(a: ForwardSettings, b: ForwardSettings) {
  return a.serverId === b.serverId && a.listenAddress === b.listenAddress && a.listenPort === b.listenPort && a.autoReconnect === b.autoReconnect && a.service.kind === b.service.kind &&
    (a.service.kind !== "portForward" || (b.service.kind === "portForward" && a.service.targetHost === b.service.targetHost && a.service.targetPort === b.service.targetPort));
}
export function runningTime(startedAt: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - startedAt) / 60000));
  return minutes === 0
    ? "刚刚启动"
    : minutes < 60
      ? `已运行 ${minutes} 分钟`
      : `已运行 ${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分钟`;
}
