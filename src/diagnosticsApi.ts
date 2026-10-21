import { invoke } from "@tauri-apps/api/core";
export interface DiagnosticsStatus {
  directory: string;
  runId: string;
  startedAt: number;
  bytes: number;
  runs: number;
  dropped: number;
  incomplete: boolean;
  fallback: boolean;
  unavailable: boolean;
  error: string | null;
  exportPhase: string;
  exportCompleted: number;
  exportTotal: number;
}
export type DiagnosticsScope = "recent" | "current" | "all";
export const diagnosticsApi = {
  status: () => invoke<DiagnosticsStatus>("get_diagnostics_status"),
  open: () => invoke<void>("open_diagnostics_directory"),
  export: (scope: DiagnosticsScope) =>
    invoke<{ path: string; warnings: number } | null>("export_diagnostics", {
      scope,
    }),
  cancel: () => invoke<void>("cancel_diagnostics_export"),
  clear: () => invoke<void>("clear_diagnostics"),
  reveal: () => invoke<void>("reveal_diagnostics_export"),
};
