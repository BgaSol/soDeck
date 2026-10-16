import { Channel, invoke } from "@tauri-apps/api/core";
import type { ResourcePreferences } from "./resourcePresentation";
import type { MonitorApi, MonitorEvent } from "./resourceMonitoring";
import { monitorApi } from "./MonitorProvider";

export interface MonitorWindowInfo {
  serverName: string;
  preferences: Partial<ResourcePreferences>;
}
export const monitorWindowApi = {
  open: (sessionId: string, preferences: ResourcePreferences) =>
    invoke<void>("open_monitor_window", { sessionId, preferences }),
  info: () => invoke<MonitorWindowInfo>("monitor_window_info"),
};
export const windowMonitorApi: MonitorApi = {
  ...monitorApi,
  subscribe: (_sessionId, interest, receive) =>
    invoke<string>("subscribe_monitor_window", {
      interest,
      events: new Channel<MonitorEvent>(receive),
    }),
};
