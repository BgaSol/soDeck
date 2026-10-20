import { Channel, invoke } from "@tauri-apps/api/core";
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
  start: (output: Channel<LogEvent>) =>
    invoke<string>("logs_start", { output }),
  stop: (collectorId: string) => invoke<void>("logs_stop", { collectorId }),
  retry: () => invoke<boolean>("logs_retry"),
  timeline: (after = 0, before?: number) =>
    invoke<LogTimeline>("logs_events_page", { after, before: before ?? null }),
  snapshot: (requestId: string) =>
    invoke<LogSnapshot>("logs_snapshot_create", { requestId }),
  release: (snapshotId: string) =>
    invoke<void>("logs_snapshot_release", { snapshotId }),
  cancel: (requestId: string) => invoke<void>("logs_cancel_job", { requestId }),
  exportScope: (
    scope: ExportScope,
    requestId: string,
    output: Channel<ExportProgress>,
  ) => invoke<string | null>("logs_export_start", { scope, requestId, output }),
  replacements: () => invoke<LogReplacement[]>("logs_replacements"),
  openReplacement: (containerId: string) =>
    invoke<void>("open_log_replacement", { containerId }),
  info: () => invoke<LogInfo>("logs_info"),
  page: (query: LogQuery, snapshotId?: string) =>
    invoke<LogPage>("logs_page", { query, snapshotId: snapshotId ?? null }),
  archives: () => invoke<LogArchive[]>("list_log_archives"),
  openArchives: () => invoke<void>("open_log_archives_window"),
  open: (sourceId: string) => invoke<void>("open_log_archive", { sourceId }),
  export: (from?: number, to?: number) =>
    invoke<string | null>("logs_export", {
      from: from ?? null,
      to: to ?? null,
    }),
  copy: (from: number, to: number, snapshotId?: string) =>
    invoke<string>("logs_copy_range", {
      from,
      to,
      snapshotId: snapshotId ?? null,
    }),
};

export interface ContainerObservation {
  state: string;
  startedAt: string | null;
  finishedAt: string | null;
  restartCount: number;
  observedAt: number;
}
export type CollectionPhase =
  | "connecting"
  | "syncingHistory"
  | "streaming"
  | "waitingContainer"
  | "reconnecting"
  | "failed"
  | "offline";
export interface CollectionState {
  phase: CollectionPhase;
  reason?: string | null;
  retryAt?: number | null;
  error?: string | null;
  container?: ContainerObservation | null;
  tty?: boolean | null;
  collectorId?: string;
  eventRevision?: number;
}
export type LogEvent =
  | ({ type: "state" } & CollectionState)
  | { type: "saved"; lastSeq: number; changedFrom: number | null }
  | { type: "lifecycle" }
  | { type: "notice"; error: string }
  | { type: "end"; error: string | null };
export interface LogLifecycleEvent {
  id: number;
  kind: string;
  observedAt: number;
  actualAt: string | null;
  anchorSeq: number;
  detail: string;
}
export interface LogBoot {
  id: string;
  startedAt: string;
  observedAt: number;
  finishedAt: string | null;
}
export interface LogTimeline {
  events: LogLifecycleEvent[];
  boots: LogBoot[];
  nextCursor: number;
  hasMore: boolean;
}
export interface LogSnapshot {
  id: string;
  lastSeq: number;
  eventCursor: number;
}
export interface ExportProgress {
  records: number;
  bytes: number;
}
export type ExportScope =
  | { type: "all" }
  | { type: "selection"; from: number; to: number; snapshotId?: string }
  | { type: "query"; snapshotId: string; query: LogQuery };
export interface LogReplacement {
  id: string;
  name: string;
  created: string;
  project: string;
  service: string;
  instance: string;
}
