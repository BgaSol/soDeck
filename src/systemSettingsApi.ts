import type {
  TerminalClipboardPreferences,
  TerminalPlatform,
} from "./terminalClipboard/types";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface SystemSettings {
  terminalClipboard: TerminalClipboardPreferences;
  platform: TerminalPlatform;
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
  saveClipboard: (
    preferences: TerminalClipboardPreferences,
    expectedRevision: number,
  ) =>
    invoke<SystemSettings>("set_terminal_clipboard_settings", {
      preferences,
      expectedRevision,
    }),
  subscribe: (receive: (settings: SystemSettings) => void) =>
    listen<SystemSettings>("system-settings-changed", ({ payload }) =>
      receive(payload),
    ),
  openDevtools: () => invoke<void>("open_window_devtools"),
};
