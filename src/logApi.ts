import { invoke } from "@tauri-apps/api/core";
export interface LogStats {
  records: number;
  lastSeq: number;
  bytes: number;
  lastTimestamp: string | null;
  runs: number;
  interruptedRuns: number;
}
export interface LogSource {
  sourceId: string;
  serverId: string;
  serverName: string;
  containerName: string;
  containerId: string;
  endpoint: string;
  host: string;
  port: number;
  tty: boolean;
}
export interface LogInfo extends LogSource {
  canCollect: boolean;
  directory: string;
  stats: LogStats;
}
export interface LogArchive extends LogSource, LogStats {}
export interface LogRow {
  seq: number;
  stream: "stdout" | "stderr";
  raw: string;
  text: string;
  timestamp: string | null;
  level: string;
  complete: boolean;
  continuation: boolean;
  prefix: string;
}
export interface LogQuery {
  before?: number;
  after?: number;
  snapshot?: number;
  limit: number;
  keyword?: string;
  caseSensitive?: boolean;
  stream?: string;
  from?: string;
  to?: string;
}
export interface LogPage {
  rows: LogRow[];
  nextCursor: number;
  hasMore: boolean;
  scanned: number;
  snapshot: number;
}
export const logApi = {
  info: () => invoke<LogInfo>("logs_info"),
  page: (query: LogQuery) => invoke<LogPage>("logs_page", { query }),
  archives: () => invoke<LogArchive[]>("list_log_archives"),
  open: (sourceId: string) => invoke<void>("open_log_archive", { sourceId }),
  export: (from?: number, to?: number) =>
    invoke<string | null>("logs_export", {
      from: from ?? null,
      to: to ?? null,
    }),
  copy: (from: number, to: number) =>
    invoke<string>("logs_copy_range", { from, to }),
};
