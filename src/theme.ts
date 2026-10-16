/// <reference types="vite/client" />

export type Theme = "light" | "dark";

const storageKey = "sodeck.theme";
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

function readPreference(): Theme | null {
  try {
    const value = localStorage.getItem(storageKey);
    return value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

let preference = readPreference();
let theme: Theme;

function applyTheme() {
  const next = preference ?? (systemTheme.matches ? "dark" : "light");
  document.documentElement.dataset.theme = next;
  if (theme === next) return;
  theme = next;
  listeners.forEach((listener) => listener());
}

export function getTheme() {
  return theme;
}

export function subscribeTheme(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function setTheme(next: Theme) {
  preference = next;
  try {
    localStorage.setItem(storageKey, next);
  } catch {
    // The current window can still switch when persistent storage is unavailable.
  }
  applyTheme();
}

function syncPreference(event: StorageEvent) {
  if (event.key !== storageKey && event.key !== null) return;
  // Read the latest value so rapidly switching windows cannot replay stale events.
  preference = readPreference();
  applyTheme();
}

// Apply before React renders, including when opening an auxiliary window.
applyTheme();
systemTheme.addEventListener("change", applyTheme);
window.addEventListener("storage", syncPreference);

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    systemTheme.removeEventListener("change", applyTheme);
    window.removeEventListener("storage", syncPreference);
  });
}
