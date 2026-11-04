import { invoke } from "@tauri-apps/api/core";
export interface Manifest {
  id: string;
  name: string;
  version: string;
  apiVersion: string;
  target: string;
  entry: string;
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
}
export interface Snapshot {
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
  snapshot: () => invoke<Snapshot>("snapshot"),
  load: (directory: string) => invoke<Plugin>("load_plugin", { directory }),
  unload: (id: string) => invoke<Plugin>("unload_plugin", { id }),
  forget: (id: string) => invoke<void>("forget_plugin", { id }),
  theme: (theme: string) => invoke<void>("set_theme", { theme }),
  logs: (filter: Filter) => invoke<Log[]>("read_logs", { filter }),
  messages: () => invoke<Message[]>("read_messages"),
  markRead: (id: string | null) => invoke<void>("mark_read", { id }),
  ready: () => invoke<void>("frontend_ready"),
};
