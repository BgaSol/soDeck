import { useSyncExternalStore } from "react";
import { desktop } from "./api";
import { systemSettingsApi, type SystemSettings } from "./systemSettingsApi";
import {
  defaultPreferences,
  type TerminalPlatform,
} from "./terminalClipboard/types";
export const browserPlatform: TerminalPlatform = /Mac/i.test(navigator.platform)
  ? "macos"
  : /Win/i.test(navigator.platform)
    ? "windows"
    : "linux";
const browserSettings: SystemSettings = {
  developerMode: false,
  activeDeveloperMode: false,
  revision: 0,
  loadError: null,
  platform: browserPlatform,
  terminalClipboard: defaultPreferences,
};
let snapshot: SystemSettings | null = desktop ? null : browserSettings;
const listeners = new Set<() => void>();
let stop: (() => void) | undefined;
let generation = 0;
export function receiveSystemSettings(value: SystemSettings) {
  if (!value || (snapshot && value.revision < snapshot.revision)) return;
  snapshot = {
    ...value,
    platform: value.platform ?? browserPlatform,
    terminalClipboard: value.terminalClipboard ?? defaultPreferences,
  };
  listeners.forEach((listener) => listener());
}
export async function refreshSystemSettings() {
  if (desktop) receiveSystemSettings(await systemSettingsApi.get());
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1 && desktop) {
    const token = ++generation;
    const refresh = () => {
      void refreshSystemSettings().catch(() => {});
    };
    window.addEventListener("focus", refresh);
    let unlisten: (() => void) | undefined;
    void systemSettingsApi
      .subscribe((value) => {
        if (token === generation) receiveSystemSettings(value);
      })
      .then((off) => {
        if (token !== generation) off();
        else {
          unlisten = off;
          refresh();
        }
      })
      .catch(refresh);
    stop = () => {
      ++generation;
      unlisten?.();
      window.removeEventListener("focus", refresh);
    };
  }
  return () => {
    listeners.delete(listener);
    if (!listeners.size) {
      stop?.();
      stop = undefined;
    }
  };
}
export function useSystemSettings() {
  return useSyncExternalStore(subscribe, () => snapshot);
}
