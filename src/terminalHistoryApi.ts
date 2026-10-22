import { invokeDiagnosed as invoke } from "./diagnostics";
export interface HistorySource {
  id: string;
  serverId: string;
  serverName: string;
  address: string;
  username: string;
  containerName: string | null;
  containerId: string | null;
  endpoint: string | null;
}
export interface HistorySession {
  id: string;
  workspaceId: string; // Legacy storage name: stable terminal group, never the server workspace id.
  source: HistorySource;
  startedAt: number;
  endedAt: number | null;
  state: string;
  savedSeq: string;
  indexedSeq: string;
  lastRecord: string;
  bytes: number;
  error: string | null;
}
export interface HistoryBinding {
  sessionId: string;
  sourceId: string;
  workspaceId: string; // Legacy storage name: stable terminal group, never the server workspace id.
}
export interface HistoryPosition {
  sessionId: string;
  rowId?: string;
}
export interface HistoryRow {
  id: string;
  sessionId: string;
  seq: string;
  offset: number;
  time: number;
  segment: number;
  kind: "text" | "revision" | "screen" | "marker";
  text: string;
  ansi: string;
}
export interface HistoryQuery {
  sessionId?: string;
  sourceId?: string;
  workspaceId?: string;
  keyword?: string;
  caseSensitive?: boolean;
  from?: number;
  to?: number;
  before?: string;
  after?: string;
  snapshot?: string;
  limit: number;
  requestId?: string;
}
export interface HistoryPage {
  rows: HistoryRow[];
  hasMore: boolean;
  snapshot: string;
}
export interface SearchHit {
  session: HistorySession;
  row: HistoryRow;
}
export interface SearchPage {
  hits: SearchHit[];
  cursor: string | null;
  snapshot: string;
}
export interface ReplayEvent {
  seq: string;
  time: number;
  kind: string;
  data: string;
  cols: number;
  rows: number;
}
export const terminalHistoryApi = {
  sessions: (query: Partial<HistoryQuery> = {}) =>
    invoke<HistorySession[]>("terminal_history_sessions", { query }),
  status: (sessionId: string) =>
    invoke<HistorySession>("terminal_history_status", { sessionId }),
  page: (query: HistoryQuery) =>
    invoke<HistoryPage>("terminal_history_page", { query }),
  search: (query: HistoryQuery, cursor?: string) =>
    invoke<SearchPage>("terminal_history_search", {
      query,
      cursor: cursor ?? null,
    }),
  cancel: (requestId: string) =>
    invoke<void>("terminal_history_cancel", { requestId }),
  replay: (
    sessionId: string,
    after: string,
    through?: string,
    offset?: number,
  ) =>
    invoke<{ events: ReplayEvent[]; after: string; hasMore: boolean }>(
      "terminal_history_replay",
      { sessionId, after, through: through ?? null, offset: offset ?? null },
    ),
  copy: (sessionId: string, from: string, to: string) =>
    invoke<string>("terminal_history_copy", { sessionId, from, to }),
  export: (sessionId: string, raw = false, from?: string, to?: string) =>
    invoke<string | null>("terminal_history_export", {
      sessionId,
      raw,
      from: from ?? null,
      to: to ?? null,
    }),
  open: () => invoke<void>("open_terminal_history_window"),
};
export function historyTitle(session: HistorySession) {
  return `${session.source.serverName}${session.source.containerName ? ` / ${session.source.containerName}` : ""} · ${new Date(session.startedAt).toLocaleString()}`;
}
export function boundedHistory(rows: HistoryRow[], keepEnd: boolean) {
  let bytes = 0;
  const result: HistoryRow[] = [];
  for (const row of keepEnd ? [...rows].reverse() : rows) {
    const size = (row.text.length + row.ansi.length) * 2 + 256;
    if (
      result.length &&
      (result.length >= 5000 || bytes + size > 16 * 1024 * 1024)
    )
      break;
    bytes += size;
    result.push(row);
  }
  return keepEnd ? result.reverse() : result;
}
