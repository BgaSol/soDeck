import { afterEach, beforeEach, expect, it, vi } from "vitest";
let dark = false,
  stored: string | null = null,
  failWrite = false;
let mediaListener: () => void,
  storageListener: (e: { key: string | null }) => void;
const dataset: Record<string, string> = {};
beforeEach(() => {
  vi.resetModules();
  dark = false;
  stored = null;
  failWrite = false;
  vi.stubGlobal("matchMedia", () => ({
    get matches() {
      return dark;
    },
    addEventListener: (_: string, fn: () => void) => {
      mediaListener = fn;
    },
    removeEventListener: vi.fn(),
  }));
  vi.stubGlobal("localStorage", {
    getItem: () => stored,
    setItem: (_: string, value: string) => {
      if (failWrite) throw Error("denied");
      stored = value;
    },
    removeItem: () => {
      if (failWrite) throw Error("denied");
      stored = null;
    },
  });
  vi.stubGlobal("document", { documentElement: { dataset } });
  vi.stubGlobal("window", {
    addEventListener: (_: string, fn: typeof storageListener) => {
      storageListener = fn;
    },
    removeEventListener: vi.fn(),
  });
});
afterEach(() => vi.unstubAllGlobals());
it("defaults to system and follows OS changes", async () => {
  const t = await import("./theme");
  expect(t.getThemePreference()).toBe("system");
  expect(t.getTheme()).toBe("light");
  dark = true;
  mediaListener();
  expect(dataset.theme).toBe("dark");
});
it("preserves old preferences and notifies when only preference changes", async () => {
  stored = "light";
  const t = await import("./theme"),
    fn = vi.fn();
  const off = t.subscribeTheme(fn);
  t.setThemePreference("system");
  expect(fn).toHaveBeenCalledTimes(1);
  expect(stored).toBeNull();
  off();
  t.setThemePreference("dark");
  expect(fn).toHaveBeenCalledTimes(1);
});
it("ignores system color changes while explicit and restores automatic mode", async () => {
  stored = "dark";
  const t = await import("./theme");
  mediaListener();
  expect(t.getTheme()).toBe("dark");
  t.setThemePreference("system");
  expect(t.getTheme()).toBe("light");
});
it("synchronizes storage updates including removal and treats invalid values as system", async () => {
  const t = await import("./theme");
  stored = "dark";
  storageListener({ key: "sodeck.theme" });
  expect(t.getThemePreference()).toBe("dark");
  stored = "invalid";
  storageListener({ key: null });
  expect(t.getThemePreference()).toBe("system");
  expect(t.getTheme()).toBe("light");
});
it("keeps local switching functional when storage writes fail", async () => {
  const t = await import("./theme"),
    fn = vi.fn();
  t.subscribeTheme(fn);
  failWrite = true;
  t.setThemePreference("light");
  expect(t.getThemePreference()).toBe("light");
  expect(fn).toHaveBeenCalledTimes(1);
  t.setThemePreference("dark");
  expect(t.getTheme()).toBe("dark");
});
