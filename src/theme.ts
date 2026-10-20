/// <reference types="vite/client" />
export type Theme = "light" | "dark";
export type ThemePreference = "system" | Theme;
export const themeOptions = [
  ["system", "跟随系统"],
  ["light", "浅色主题"],
  ["dark", "深色主题"],
] as const;
const storageKey = "sodeck.theme";
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();
function readPreference(): ThemePreference {
  try {
    const value = localStorage.getItem(storageKey);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}
let preference = readPreference();
let theme: Theme;
let appliedPreference: ThemePreference;
function applyTheme() {
  const next: Theme =
    preference === "system"
      ? systemTheme.matches
        ? "dark"
        : "light"
      : preference;
  const preferenceChanged = preference !== appliedPreference;
  appliedPreference = preference;
  if (theme !== next) document.documentElement.dataset.theme = next;
  if (theme === next && !preferenceChanged) return;
  theme = next;
  listeners.forEach((l) => l());
}
export function getTheme() {
  return theme;
}
export function getThemePreference() {
  return preference;
}
export function subscribeTheme(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
export function setThemePreference(next: ThemePreference) {
  preference = next;
  try {
    if (next === "system") localStorage.removeItem(storageKey);
    else localStorage.setItem(storageKey, next);
  } catch {}
  applyTheme();
}
export function setTheme(next: Theme) {
  setThemePreference(next);
}
function syncPreference(event: StorageEvent) {
  if (event.key !== storageKey && event.key !== null) return;
  preference = readPreference();
  applyTheme();
}
applyTheme();
systemTheme.addEventListener("change", applyTheme);
window.addEventListener("storage", syncPreference);
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    systemTheme.removeEventListener("change", applyTheme);
    window.removeEventListener("storage", syncPreference);
  });
