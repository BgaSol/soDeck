import { useCallback, useEffect, useRef, useState } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";

export type WindowChrome = "custom" | "overlay" | "native" | "browser";
let chromeRequest: Promise<WindowChrome> | undefined;

export function useDesktopWindow(onError: (error: unknown) => void) {
  const report = useRef(onError);
  report.current = onError;
  const [chrome, setChrome] = useState<WindowChrome | "loading">(
    desktop ? "loading" : "browser",
  );
  const [maximized, setMaximized] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [focused, setFocused] = useState(true);

  useEffect(() => {
    if (!desktop) return;
    const window = getCurrentWindow();
    let disposed = false;
    let revision = 0;
    const cleanups: (() => void)[] = [];
    const fail = (error: unknown) => {
      if (!disposed) report.current(error);
    };
    const refresh = async () => {
      const current = ++revision;
      try {
        const [max, full] = await Promise.all([
          window.isMaximized(),
          window.isFullscreen(),
        ]);
        if (!disposed && current === revision) {
          setMaximized(max);
          setFullscreen(full);
        }
      } catch (error) {
        fail(error);
      }
    };
    const subscribe = (pending: Promise<() => void>) => {
      void pending
        .then((stop) => {
          if (disposed) stop();
          else cleanups.push(stop);
        })
        .catch(fail);
    };
    chromeRequest ??= invoke<WindowChrome>("window_chrome").catch((error) => {
      chromeRequest = undefined;
      throw error;
    });
    void chromeRequest
      .then((mode) => {
        if (!disposed) setChrome(mode);
      })
      .catch((error) => {
        // Keep a usable close button even if platform discovery fails.
        if (!disposed) setChrome("custom");
        fail(error);
      });
    subscribe(window.onResized(() => void refresh()));
    subscribe(
      window.onFocusChanged(({ payload }) => {
        if (!disposed) setFocused(payload);
        void refresh();
      }),
    );
    void window
      .isFocused()
      .then((value) => {
        if (!disposed) setFocused(value);
      })
      .catch(fail);
    void refresh();
    return () => {
      disposed = true;
      cleanups.forEach((stop) => stop());
    };
  }, []);

  const run = useCallback(
    async (action: "minimize" | "toggleMaximize" | "close") => {
      if (!desktop) return;
      try {
        const window = getCurrentWindow();
        await window[action]();
        if (action === "toggleMaximize")
          setMaximized(await window.isMaximized());
      } catch (error) {
        report.current(error);
      }
    },
    [],
  );
  return { chrome, maximized, fullscreen, focused, run };
}
