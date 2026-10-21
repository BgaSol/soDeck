import { useEffect, useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";

export interface DesktopDragEvent {
  intent?: "copy" | "move";
  phase: "over" | "drop" | "leave";
  kind: "localFiles" | "remoteFiles" | "workspaceTab";
  token: string;
  x: number;
  y: number;
  count: number;
  label: string;
}
export function useDesktopDrag(callback: (event: DesktopDragEvent) => void) {
  const current = useRef(callback);
  current.current = callback;
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let off: (() => void) | undefined;
    void getCurrentWindow()
      .listen<DesktopDragEvent>("desktop-drag", ({ payload }) =>
        current.current(payload),
      )
      .then((cleanup) => {
        if (disposed) cleanup();
        else off = cleanup;
      });
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
}
export function useNativeDragStart(onError: (error: unknown) => void) {
  const pending = useRef<{
    x: number;
    y: number;
    start: () => Promise<unknown>;
  } | null>(null);
  const suppressedUntil = useRef(0);
  const error = useRef(onError);
  error.current = onError;
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    function move(event: PointerEvent) {
      const drag = pending.current;
      if (
        !drag ||
        Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 6
      )
        return;
      pending.current = null;
      suppressedUntil.current = Infinity;
      void drag.start().catch((e) => {
        suppressedUntil.current = Date.now() + 300;
        error.current(e);
      });
    }
    function end() {
      pending.current = null;
      if (suppressedUntil.current === Infinity)
        suppressedUntil.current = Date.now() + 600;
    }
    function key(event: KeyboardEvent) {
      if (event.key === "Escape") {
        pending.current = null;
        void invoke("cancel_native_drag").catch(() => {});
      }
    }
    if (desktop) {
      void getCurrentWindow()
        .listen("desktop-drag-ended", end)
        .then((cleanup) => {
          if (disposed) cleanup();
          else off = cleanup;
        });
    }
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", end);
    document.addEventListener("pointercancel", end);
    document.addEventListener("keydown", key);
    return () => {
      disposed = true;
      off?.();
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", end);
      document.removeEventListener("pointercancel", end);
      document.removeEventListener("keydown", key);
    };
  }, []);
  return {
    down(event: ReactPointerEvent, start: () => Promise<unknown>) {
      if (!desktop || event.button !== 0) return;
      pending.current = { x: event.clientX, y: event.clientY, start };
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    suppressClick: () => Date.now() < suppressedUntil.current,
  };
}
export function fileDropTarget(
  panel: HTMLElement,
  x: number,
  y: number,
): string | null {
  const element = document.elementFromPoint(x, y);
  if (!element || !panel.contains(element)) return null;
  const row = element.closest<HTMLElement>(".file-row");
  if (row && !row.dataset.dropDirectory) return null;
  return (
    element.closest<HTMLElement>("[data-drop-directory]")?.dataset
      .dropDirectory ?? null
  );
}
