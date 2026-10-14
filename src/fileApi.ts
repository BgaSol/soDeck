import { invoke } from "@tauri-apps/api/core";

export type DownloadConflict = "rename" | "overwrite" | "skip";
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
  openEditor: (sessionId: string, path: string) =>
    invoke<string>("open_editor", { sessionId, path }),
  loadEditor: () => invoke<EditorDocument>("editor_load"),
  setEditorDirty: (dirty: boolean) =>
    invoke<void>("editor_set_dirty", { dirty }),
  saveEditor: (text: string, version: string, force = false) =>
    invoke<EditorDocument>("editor_save", { text, version, force }),
  startDownload: (
    sessionId: string,
    path: string,
    isDir: boolean,
    conflict: DownloadConflict,
  ) =>
    invoke<string | null>("start_download", {
      sessionId,
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
