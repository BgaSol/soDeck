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
  const target = endpoint(settings.targetHost, settings.targetPort);
  const serverName = hops.at(-1)?.name || server?.name || "未配置服务器";
  return {
    settings,
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
  return (Object.keys(a) as (keyof ForwardSettings)[]).every(
    (key) => a[key] === b[key],
  );
}
export function runningTime(startedAt: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - startedAt) / 60000));
  return minutes === 0
    ? "刚刚启动"
    : minutes < 60
      ? `已运行 ${minutes} 分钟`
      : `已运行 ${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分钟`;
}
