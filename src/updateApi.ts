import { invokeDiagnosed as invoke } from "./diagnostics";
import { listen } from "@tauri-apps/api/event";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "verifying"
  | "ready"
  | "preparing"
  | "installing"
  | "failed";
export interface UpdateSnapshot {
  revision: number;
  phase: UpdatePhase;
  currentVersion: string;
  release: {
    id: string;
    version: string;
    publishedAt: string | null;
    notes: string;
  } | null;
  install: {
    mode: "inApp" | "manual" | "unavailable";
    reason: string | null;
    target: string | null;
    manualFormat: string | null;
  };
  operation: { id: string; source: "automatic" | "manual" } | null;
  progress: { downloaded: number; total: number | null } | null;
  preferences: { revision: number; autoCheck: boolean; autoDownload: boolean };
  lastSuccessfulCheckAt: number | null;
  suppression: "cancelled" | "failed" | "skipped" | null;
  error: {
    stage: string;
    code: string;
    retry: "check" | "download" | "install" | "none";
  } | null;
  result: {
    id: string;
    targetVersion: string;
    outcome: "completed" | "incomplete";
  } | null;
  message: string | null;
  skippedVersion: string | null;
}
export interface InstallReview {
  releaseId: string;
  reviewToken: string | null;
  expiresAt: number;
  blockers: Array<{ kind: string; count: number; action: string | null }>;
  interruptions: Array<{
    kind: string;
    count: number;
    connections: number | null;
  }>;
}
export type InstallSubmission =
  { status: "accepted" } | { status: "reviewChanged"; review: InstallReview };

export const updateApi = {
  snapshot: () => invoke<UpdateSnapshot>("update_snapshot"),
  check: () => invoke<UpdateSnapshot>("update_check"),
  download: (releaseId: string) =>
    invoke<UpdateSnapshot>("update_download", { releaseId }),
  cancelDownload: (operationId: string) =>
    invoke<UpdateSnapshot>("update_cancel_download", { operationId }),
  preferences: (
    revision: number,
    patch: { autoCheck?: boolean; autoDownload?: boolean },
  ) =>
    invoke<UpdateSnapshot>("update_preferences", {
      expectedRevision: revision,
      patch,
    }),
  skipVersion: (releaseId: string, skip: boolean) =>
    invoke<UpdateSnapshot>("update_skip_version", { releaseId, skip }),
  preflight: (releaseId: string) =>
    invoke<InstallReview>("update_preflight", { releaseId }),
  install: (reviewToken: string) =>
    invoke<InstallSubmission>("update_install", { reviewToken }),
  ackNotice: (releaseId: string) =>
    invoke<UpdateSnapshot>("update_ack_notice", { releaseId }),
  ackResult: (resultId: string) =>
    invoke<UpdateSnapshot>("update_ack_result", { resultId }),
  openRelease: (releaseId?: string) =>
    invoke<void>("update_open_release", { releaseId }),
  openManualAsset: (releaseId: string) =>
    invoke<void>("update_open_manual_asset", { releaseId }),
  restartRecovery: () => invoke<void>("update_restart_recovery"),
  focusMain: () => invoke<void>("update_focus_main"),
  subscribe: (receive: (s: UpdateSnapshot) => void) =>
    listen<UpdateSnapshot>("app-update-changed", ({ payload }) =>
      receive(payload),
    ),
};

export function updateBadge(s: UpdateSnapshot | null) {
  return (
    !!s?.release &&
    s.release.version !== s.skippedVersion &&
    ["available", "downloading", "verifying", "ready"].includes(s.phase)
  );
}
