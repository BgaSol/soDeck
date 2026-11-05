import { invoke } from "@tauri-apps/api/core";
export interface Manifest {
  id: string;
  name: string;
  version: string;
  apiVersion: string;
  target: string;
  entry: string;
  updateSource?: { repository: string; tagPrefix: string };
}
export interface Plugin {
  directory: string;
  manifest: Manifest;
  status: "unloaded" | "starting" | "running" | "stopping" | "failed";
  instanceId: string | null;
  pid: number | null;
  startedAt: number | null;
  error: string | null;
  cleanupComplete: boolean;
  enabled: boolean;
  pendingRestart: boolean;
  pendingVersion: string | null;
  updateError: string | null;
}
export interface Snapshot {
  connectedServers: number;
  connectingServers: number;
  version: string;
  target: string;
  os: string;
  arch: string;
  startedAt: number;
  uptimeMs: number;
  dataDirectory: string;
  logDirectory: string;
  preferences: { theme: string };
  plugins: Plugin[];
  unreadCount: number;
  storageErrors: string[];
  restartRequired: boolean;
}
export interface Update {
  id: string;
  phase: string;
  version: string | null;
  notes: string;
  downloadedBytes: number;
  totalBytes: number | null;
  error: string | null;
}
export interface Log {
  id: string;
  receivedAt: number;
  source: string;
  pluginId: string | null;
  instanceId: string | null;
  level: string;
  message: string;
  fields: unknown;
}
export interface Message {
  id: string;
  receivedAt: number;
  source: string;
  pluginId: string | null;
  instanceId: string | null;
  level: string;
  title: string;
  body: string;
  read: boolean;
  count: number;
}
export interface Filter {
  pluginId: string;
  level: string;
  query: string;
  since?: number;
  until?: number;
}
export const core = {
  close: () => invoke<void>("close_app"),
  snapshot: () => invoke<Snapshot>("snapshot"),
  add: (directory: string) => invoke<string>("add_plugin", { directory }),
  enabled: (id: string, enabled: boolean) => invoke<void>("set_plugin_enabled", { id, enabled }),
  updateSource: (id: string, source: Manifest["updateSource"] | null) => invoke<void>("set_plugin_update_source", { id, source }),
  updates: () => invoke<Record<string, Update>>("update_status"),
  checkUpdate: (id: string) => invoke<void>("check_update", { id }),
  download: (id: string) => invoke<boolean>("download_update", { id }),
  restart: (installCore = false) => invoke<void>("restart_app", { installCore }),
  forget: (id: string) => invoke<void>("forget_plugin", { id }),
  theme: (theme: string) => invoke<void>("set_theme", { theme }),
  logs: (filter: Filter) => invoke<Log[]>("read_logs", { filter }),
  messages: () => invoke<Message[]>("read_messages"),
  markRead: (id: string | null) => invoke<void>("mark_read", { id }),
  ready: () => invoke<void>("frontend_ready"),
};
