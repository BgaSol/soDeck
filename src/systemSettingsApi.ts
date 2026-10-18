import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface SystemSettings {
  developerMode: boolean;
  activeDeveloperMode: boolean;
  revision: number;
  loadError: string | null;
}

export const systemSettingsApi = {
  get: () => invoke<SystemSettings>("get_system_settings"),
  save: (developerMode: boolean, expectedRevision: number) =>
    invoke<SystemSettings>("set_developer_mode", {
      developerMode,
      expectedRevision,
    }),
  subscribe: (receive: (settings: SystemSettings) => void) =>
    listen<SystemSettings>("system-settings-changed", ({ payload }) =>
      receive(payload),
    ),
  openDevtools: () => invoke<void>("open_window_devtools"),
};
