import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
export interface UpdateSnapshot {
  revision: number;
  phase:
    | "idle"
    | "checking"
    | "upToDate"
    | "available"
    | "downloading"
    | "ready"
    | "preparing"
    | "installing"
    | "failed";
  currentVersion: string;
  version: string | null;
  notes: string | null;
  publishedAt: string | null;
  downloaded: number;
  total: number | null;
  message: string | null;
  automaticInstall: boolean;
  installReason: string;
  autoCheck: boolean;
  skippedVersion: string | null;
  lastCheck: number | null;
}
export interface UpdatePreflight {
  blockers: string[];
  interruptions: string[];
}
export const updateApi = {
  snapshot: () => invoke<UpdateSnapshot>("update_snapshot"),
  check: () => invoke<UpdateSnapshot>("update_check"),
  download: () => invoke<UpdateSnapshot>("update_download"),
  preferences: (autoCheck: boolean, skippedVersion: string | null) =>
    invoke<UpdateSnapshot>("update_preferences", { autoCheck, skippedVersion }),
  preflight: () => invoke<UpdatePreflight>("update_preflight"),
  install: () => invoke<void>("update_install", { confirmed: true }),
  openRelease: () => invoke<void>("update_open_release"),
  focusMain: () => invoke<void>("update_focus_main"),
  subscribe: (receive: (s: UpdateSnapshot) => void) =>
    listen<UpdateSnapshot>("app-update-changed", ({ payload }) =>
      receive(payload),
    ),
};
export function updateBusy(s: UpdateSnapshot) {
  return ["checking", "downloading", "preparing", "installing"].includes(
    s.phase,
  );
}
export function updateBadge(s: UpdateSnapshot | null) {
  return (
    !!s?.version &&
    s.version !== s.skippedVersion &&
    ["available", "downloading", "ready"].includes(s.phase)
  );
}
