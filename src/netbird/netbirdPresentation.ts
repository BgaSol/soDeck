import type { Action, Peer, Snapshot } from "./netbirdApi";
export const actionLabels: Record<Action, string> = {
  up: "连接 NetBird",
  down: "断开 NetBird",
  serviceStart: "启动服务",
  serviceRestart: "重启服务",
};
export function statusLabel(value: string) {
  return (
    (
      {
        connected: "已连接",
        disconnected: "已断开",
        connecting: "连接中",
        idle: "空闲",
        unknown: "未知",
        needsLogin: "需要登录",
        running: "运行中",
        stopped: "未运行",
        notConfigured: "未配置",
        unavailable: "不可用",
        error: "异常",
        direct: "直连",
        relayed: "中继",
        authenticated: "已认证",
      } as Record<string, string>
    )[value] ?? "未知"
  );
}
export function overall(snapshot: Snapshot, connected: boolean) {
  if (!connected) return { label: "SSH 已断开", tone: "muted" };
  if (snapshot.collectionStatus === "collecting" && !snapshot.updatedAt)
    return { label: "正在检测", tone: "muted" };
  if (snapshot.updatedAt && snapshot.collectionStatus !== "ready")
    return { label: "数据待更新", tone: "warning" };
  if (snapshot.installation === "missing")
    return { label: "未检测到客户端", tone: "muted" };
  if (snapshot.service === "stopped")
    return { label: "服务未运行", tone: "muted" };
  if (snapshot.connection === "needsLogin")
    return { label: "需要登录", tone: "warning" };
  if (snapshot.collectionStatus !== "ready")
    return {
      label: snapshot.updatedAt ? "数据待更新" : "状态不可用",
      tone: "warning",
    };
  if (
    snapshot.connection !== "disconnected" &&
    ([snapshot.management, snapshot.signal].some((c) =>
      ["disconnected", "error"].includes(c.state),
    ) ||
      snapshot.dns.state === "error")
  )
    return { label: "连接受限", tone: "warning" };
  return {
    label: statusLabel(snapshot.connection),
    tone: snapshot.connection === "connected" ? "good" : "muted",
  };
}
export function latency(value: number | null) {
  return value == null || !Number.isFinite(value) || value <= 0
    ? "—"
    : `${value < 1 ? value.toFixed(2) : value.toFixed(0)} ms`;
}
export function at(value: number | null) {
  return value == null ? "未知" : new Date(value).toLocaleString();
}
export function bytes(value: string | null) {
  if (value == null || !/^\d+$/.test(value)) return "—";
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  if (n < 1024) return `${value} B`;
  const exponent = Math.min(Math.floor(Math.log(n) / Math.log(1024)), 4);
  return `${(n / 1024 ** exponent).toFixed(1)} ${["B", "KiB", "MiB", "GiB", "TiB"][exponent]}`;
}
export function filterPeers(
  peers: Peer[],
  query: string,
  state: string,
  mode: string,
) {
  const text = query.trim().toLocaleLowerCase();
  return peers.filter(
    (p) =>
      (!state || p.state === state) &&
      (!mode || p.connectionMode === mode) &&
      [p.name, p.fqdn, p.ipv4, p.ipv6].some((v) =>
        v?.toLocaleLowerCase().includes(text),
      ),
  );
}
export function previewPeers(peers: Peer[]) {
  const rank = (p: Peer) =>
    p.state === "connecting" ? 0 : p.state === "connected" ? 1 : 2;
  return [...peers]
    .sort(
      (a, b) =>
        rank(a) - rank(b) ||
        (b.lastHandshakeAt ?? 0) - (a.lastHandshakeAt ?? 0) ||
        a.name.localeCompare(b.name),
    )
    .slice(0, 4);
}
export function activeOperation(phase: string) {
  return ["queued", "running", "verifying"].includes(phase);
}
