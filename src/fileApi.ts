import { contextReference } from "./toolContext";
import { invokeDiagnosed as invoke } from "./diagnostics";

export type DownloadConflict = "rename" | "overwrite" | "skip";
export interface DeleteEntry {
  path: string;
  kind: "file" | "directory" | "symlink" | "missing";
  status:
    "pending" | "deleted" | "missing" | "partial" | "failed" | "notExecuted";
  deletedCount: number;
  missingCount: number;
  failures: {
    path: string;
    message: string;
    uncertain: boolean;
    currentState: "missing" | "exists" | null;
  }[];
}
export interface DeletePreview {
  id: string;
  serverName: string;
  entries: DeleteEntry[];
  parentTaskId?: string | null;
}
export interface DeleteTask {
  id: string;
  sessionId: string; // Legacy persisted field; new tasks store their tool capability id here.
  serverName: string;
  status:
    | "checking"
    | "scanning"
    | "running"
    | "cancelling"
    | "completed"
    | "partialFailed"
    | "failed"
    | "cancelled"
    | "unknown"
    | "interrupted";
  revision: number;
  createdAt: number;
  lastStartedAt: number;
  finishedAt: number | null;
  sortOrder: number;
  entryCount: number;
  parentTaskId: string | null;
  lastCheckedAt: number | null;
  currentPath: string | null;
  deletedCount: number;
  missingCount: number;
  entries: DeleteEntry[];
  affectedDirectories: string[];
  error: string | null;
}
export function isDeleteActive(task: DeleteTask) {
  return ["checking", "scanning", "running", "cancelling"].includes(
    task.status,
  );
}
export interface EditorDocument {
  serverName: string;
  path: string;
  text: string;
  version: string;
  size: number;
}
export interface DownloadTask {
  id: string;
  serverName: string;
  remotePath: string;
  localPath: string;
  isDir: boolean;
  conflict: DownloadConflict;
  status:
    "queued" | "scanning" | "running" | "completed" | "failed" | "cancelled";
  bytesDone: number;
  bytesTotal: number;
  filesDone: number;
  filesTotal: number;
  skipped: number;
  error: string | null;
}
export const fileApi = {
  prepareDelete: (contextId: string, paths: string[]) =>
    invoke<DeletePreview>("prepare_remote_delete", {
      context: contextReference(contextId),
      paths,
    }),
  prepareDeleteRetry: (taskId: string) =>
    invoke<DeletePreview>("prepare_delete_retry", { taskId }),
  startDelete: (draftId: string) =>
    invoke<DeleteTask>("start_remote_delete", { draftId }),
  deletes: () => invoke<DeleteTask[]>("list_remote_deletes"),
  cancelDelete: (taskId: string) =>
    invoke<void>("cancel_remote_delete", { taskId }),
  reconcileDelete: (taskId: string) =>
    invoke<DeleteTask>("reconcile_remote_delete", { taskId }),
  openEditor: (contextId: string, path: string) =>
    invoke<string>("open_editor", {
      context: contextReference(contextId),
      path,
    }),
  loadEditor: () => invoke<EditorDocument>("editor_load"),
  setEditorDirty: (dirty: boolean) =>
    invoke<void>("editor_set_dirty", { dirty }),
  saveEditor: (text: string, version: string, force = false) =>
    invoke<EditorDocument>("editor_save", { text, version, force }),
  startDownload: (
    contextId: string,
    path: string,
    isDir: boolean,
    conflict: DownloadConflict,
  ) =>
    invoke<string | null>("start_download", {
      context: contextReference(contextId),
      path,
      isDir,
      conflict,
    }),
  downloads: () => invoke<DownloadTask[]>("list_downloads"),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  retryDownload: (id: string) => invoke<void>("retry_download", { id }),
};

export function isDownloadActive(task: DownloadTask) {
  return (
    task.status === "queued" ||
    task.status === "scanning" ||
    task.status === "running"
  );
}

export function notifyDownloadStarted() {
  window.dispatchEvent(new Event("workspace-download-started"));
}
