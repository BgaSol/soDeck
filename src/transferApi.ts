import { contextReference } from "./toolContext";
import { invokeDiagnosed as invoke } from "./diagnostics";
export type ConflictPolicy = "rename" | "skip" | "overwrite";
export type WriteKind = "upload" | "direct" | "serverCopy" | "serverMove";
export interface RemoteClipboard {
  id: string;
  paths: string[];
  intent: "copy" | "move";
  sameServer: boolean;
  reserved: boolean;
}
export interface TransferDraft {
  kind: WriteKind;
  id: string;
  sourceName: string;
  sourcePaths: string[];
  count: number;
  targetName: string;
  targetPath: string;
  directEndpoint: { host: string; port: number; username: string } | null;
  endpointRequired: boolean;
  savedPasswordAvailable?: boolean;
}
export interface TransferAuth {
  mode: "savedPassword" | "password" | "key";
  password?: string;
  keyPath?: string;
}
export interface CopyTarget {
  token: string;
  serverName: string;
  path: string;
}
export interface TransferTask {
  id: string;
  runId: string;
  createdAt?: number | null;
  lastStartedAt?: number | null;
  finishedAt?: number | null;
  sortOrder?: number;
  sourcePaths?: string[];
  lastAttempt?: {
    runId: string;
    startedAt: number | null;
    status: TransferTask["status"];
    error: string | null;
  } | null;
  kind: "download" | "docker" | WriteKind;
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
    | "verifying"
    | "committing"
    | "cancelling"
    | "completed"
    | "failed"
    | "partialFailed"
    | "sourceRetained"
    | "cancelled"
    | "unknown"
    | "interrupted"
    | "needsAttention";
  directAddress?: string | null;
  temporaryPaths?: string[];
  canResume?: boolean;
  items?: {
    source: string;
    target: string | null;
    status: string;
    error: string | null;
  }[];
  cleanupPending?: boolean;
  bytesDone: number;
  bytesTotal: number;
  currentFile?: string | null;
  currentBytes?: number;
  committedBytes?: number;
  waitingReason?: string | null;
  filesDone: number;
  filesTotal: number;
  skipped: number;
  error: string | null;
  warnings: string[];
}
export const transferApi = {
  clipboard: (contextId: string) =>
    invoke<RemoteClipboard | null>("get_remote_clipboard", {
      context: contextReference(contextId),
    }),
  copy: (contextId: string, paths: string[], intent: "copy" | "move") =>
    invoke<void>("set_remote_clipboard", {
      context: contextReference(contextId),
      paths,
      intent,
    }),
  clearClipboard: (id: string) =>
    invoke<void>("clear_remote_clipboard", { id }),
  paste: (contextId: string, parentPath: string) =>
    invoke<TransferDraft>("prepare_remote_paste", {
      context: contextReference(contextId),
      parentPath,
    }),
  create: (
    contextId: string,
    parentPath: string,
    name: string,
    kind: "file" | "directory",
  ) =>
    invoke<string>("create_remote_entry", {
      context: contextReference(contextId),
      parentPath,
      name,
      kind,
    }),
  pickFiles: (contextId: string, parentPath: string) =>
    invoke<TransferDraft | null>("pick_upload_files", {
      context: contextReference(contextId),
      parentPath,
    }),
  beginDrag: (contextId: string, paths: string[]) =>
    invoke<string>("begin_file_drag", {
      context: contextReference(contextId),
      paths,
    }),
  claimDrop: (token: string, contextId: string, parentPath: string) =>
    invoke<TransferDraft>("claim_file_drop", {
      token,
      context: contextReference(contextId),
      parentPath,
    }),
  start: (
    draftId: string,
    conflict: ConflictPolicy,
    auth?: TransferAuth,
    verifySha256 = false,
  ) =>
    invoke<string>("start_transfer", { draftId, conflict, auth, verifySha256 }),
  setEndpoint: (draftId: string, host: string, port: number) =>
    invoke<TransferDraft>("set_transfer_endpoint", { draftId, host, port }),
  registerView: (
    contextId: string,
    viewId: string,
    path: string,
    revision: number,
  ) =>
    invoke<void>("register_file_view", {
      context: contextReference(contextId),
      viewId,
      path,
      revision,
    }),
  unregisterView: (viewId: string) =>
    invoke<void>("unregister_file_view", { viewId }),
  copyTargets: (contextId: string) =>
    invoke<CopyTarget[]>("list_copy_targets", {
      context: contextReference(contextId),
    }),
  prepareCopy: (contextId: string, paths: string[], targetToken: string) =>
    invoke<TransferDraft>("prepare_server_copy", {
      context: contextReference(contextId),
      paths,
      targetToken,
    }),
  list: () => invoke<TransferTask[]>("list_transfers"),
  cancel: (id: string) => invoke<void>("cancel_transfer", { id }),
  retry: (id: string, auth?: TransferAuth, resume = false) =>
    invoke<void>("retry_transfer", { id, auth, resume }),
  reconcile: (id: string) => invoke<void>("reconcile_transfer", { id }),
  cleanup: (id: string) => invoke<void>("cleanup_transfer", { id }),
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
    "verifying",
    "committing",
    "cancelling",
  ].includes(task.status);
}
