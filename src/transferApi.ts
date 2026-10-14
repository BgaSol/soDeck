import { invoke } from "@tauri-apps/api/core";
export type ConflictPolicy = "rename" | "skip" | "overwrite";
export interface TransferDraft {
  id: string;
  sourceName: string;
  sourcePaths: string[];
  count: number;
  targetName: string;
  targetPath: string;
}
export interface TransferTask {
  id: string;
  runId: string;
  kind: "download" | "upload" | "direct";
  sourceName: string;
  sourcePath: string;
  targetName: string;
  targetPath: string;
  conflict: ConflictPolicy;
  status:
    | "queued"
    | "checking"
    | "scanning"
    | "running"
    | "committing"
    | "cancelling"
    | "completed"
    | "failed"
    | "partialFailed"
    | "cancelled"
    | "unknown"
    | "interrupted";
  cleanupPending?: boolean;
  bytesDone: number;
  bytesTotal: number;
  filesDone: number;
  filesTotal: number;
  skipped: number;
  error: string | null;
  warnings: string[];
}
export const transferApi = {
  create: (
    sessionId: string,
    parentPath: string,
    name: string,
    kind: "file" | "directory",
  ) =>
    invoke<string>("create_remote_entry", {
      sessionId,
      parentPath,
      name,
      kind,
    }),
  pickFiles: (sessionId: string, parentPath: string) =>
    invoke<TransferDraft | null>("pick_upload_files", {
      sessionId,
      parentPath,
    }),
  beginDrag: (sessionId: string, paths: string[]) =>
    invoke<string>("begin_file_drag", { sessionId, paths }),
  claimDrop: (token: string, sessionId: string, parentPath: string) =>
    invoke<TransferDraft>("claim_file_drop", { token, sessionId, parentPath }),
  start: (draftId: string, conflict: ConflictPolicy) =>
    invoke<string>("start_transfer", { draftId, conflict }),
  list: () => invoke<TransferTask[]>("list_transfers"),
  cancel: (id: string) => invoke<void>("cancel_transfer", { id }),
  retry: (id: string) => invoke<void>("retry_transfer", { id }),
  reconcile: (id: string) => invoke<void>("reconcile_transfer", { id }),
};
export function notifyTransferStarted() {
  window.dispatchEvent(new Event("workspace-transfer-started"));
}
export function transferActive(task: TransferTask) {
  return [
    "queued",
    "checking",
    "scanning",
    "running",
    "committing",
    "cancelling",
  ].includes(task.status);
}
