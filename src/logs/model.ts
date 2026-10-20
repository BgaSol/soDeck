import type { CollectionState, LogRow } from "../logApi";
export type PauseReason = "browse" | "manual" | "selection";
export type ReadState =
  { mode: "follow" } | { mode: "paused"; reason: PauseReason };
export type ReadAction =
  | { type: "pause"; reason: PauseReason }
  | { type: "resume" }
  | {
      type: "bottom";
      userInitiated: boolean;
      hasSelection: boolean;
      live: boolean;
    };
export function readReducer(state: ReadState, action: ReadAction): ReadState {
  if (action.type === "resume") return { mode: "follow" };
  if (action.type === "pause") {
    if (state.mode === "paused" && state.reason === action.reason) return state;
    if (
      state.mode === "paused" &&
      state.reason !== "browse" &&
      action.reason === "browse"
    )
      return state;
    return { mode: "paused", reason: action.reason };
  }
  return action.live &&
    action.userInitiated &&
    !action.hasSelection &&
    state.mode === "paused" &&
    state.reason === "browse"
    ? { mode: "follow" }
    : state;
}
export function collectionLabel(s: CollectionState, now = Date.now()) {
  if (s.phase === "failed" && s.reason === "containerMissing")
    return "原容器已不存在";
  if (s.phase === "reconnecting")
    return (
      "正在重连" +
      (s.retryAt
        ? " · " + Math.max(0, Math.ceil((s.retryAt - now) / 1000)) + " 秒后重试"
        : "")
    );
  if (s.phase === "waitingContainer" && s.container?.state === "restarting")
    return "容器正在重启";
  return {
    connecting: "正在连接",
    syncingHistory: "正在同步历史",
    streaming: "实时接收",
    waitingContainer: "等待容器启动",
    failed: "采集已停止",
    offline: "本地存档",
  }[s.phase];
}
export const MAX_ROWS = 5000,
  MAX_BYTES = 16 * 1024 * 1024,
  PAGE_SIZE = 500;
export function bounded(rows: LogRow[], keepEnd = true) {
  let bytes = 0;
  const kept: LogRow[] = [];
  for (const row of keepEnd ? [...rows].reverse() : rows) {
    const size = (row.raw.length + row.text.length + row.prefix.length) * 2;
    if (
      kept.length >= MAX_ROWS ||
      (kept.length > 0 && bytes + size > MAX_BYTES)
    )
      break;
    bytes += size;
    kept.push(row);
  }
  return keepEnd ? kept.reverse() : kept;
}
export function mergeRows(previous: LogRow[], incoming: LogRow[]) {
  const map = new Map(previous.map((r) => [r.seq, r]));
  for (const row of incoming) map.set(row.seq, row);
  return [...map.values()].sort((a, b) => a.seq - b.seq);
}
export function validRange(from: string, to: string) {
  const a = Number(from),
    b = Number(to);
  return Number.isSafeInteger(a) && Number.isSafeInteger(b) && a >= 1 && b >= a;
}
