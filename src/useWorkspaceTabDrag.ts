import {
  useEffect,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
  type RefObject,
} from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { useDesktopDrag, useNativeDragStart } from "./nativeDrag";

const dragType = "application/x-ssh-workspace-tab";
type DropTarget = { beforeId: string | null; append: boolean };

export function useWorkspaceTabDrag({
  strip,
  appendDropRef,
  onAppendDropChange,
  onError,
}: {
  strip: RefObject<HTMLDivElement | null>;
  appendDropRef?: RefObject<HTMLDivElement | null>;
  onAppendDropChange?: (active: boolean) => void;
  onError: (error: unknown) => void;
}) {
  const [over, setOver] = useState<string | null | undefined>();
  const callbacks = useRef({ onAppendDropChange, onError });
  callbacks.current = { onAppendDropChange, onError };
  const frame = useRef(0);
  const point = useRef<{ x: number; y: number } | null>(null);
  const lastDrop = useRef("");
  const drag = useRef<{
    token: string;
    ready: Promise<unknown>;
    cancelled: boolean;
  } | null>(null);
  const native = useNativeDragStart((error) =>
    callbacks.current.onError(error),
  );

  function contains(rect: DOMRect | undefined, x: number, y: number) {
    return (
      rect &&
      x >= rect.left &&
      x <= rect.right &&
      y >= rect.top &&
      y <= rect.bottom
    );
  }
  function targetAt(x: number, y: number): DropTarget | null {
    if (contains(strip.current?.getBoundingClientRect(), x, y)) {
      const next = Array.from(
        strip.current!.querySelectorAll<HTMLElement>("[data-tab-id]"),
      ).find((el) => {
        const r = el.getBoundingClientRect();
        return x < r.left + r.width / 2;
      });
      return { beforeId: next?.dataset.tabId ?? null, append: false };
    }
    if (contains(appendDropRef?.current?.getBoundingClientRect(), x, y))
      return { beforeId: null, append: true };
    return null;
  }
  function show(target: DropTarget | null) {
    setOver(target ? target.beforeId : undefined);
    callbacks.current.onAppendDropChange?.(target?.append ?? false);
  }
  function clear() {
    point.current = null;
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    show(null);
  }
  function autoScroll() {
    if (frame.current) return;
    let previous = performance.now();
    const tick = (now: number) => {
      frame.current = 0;
      const viewport = strip.current;
      const cursor = point.current;
      if (!viewport || !cursor) return;
      const bounds = viewport.getBoundingClientRect();
      if (!contains(bounds, cursor.x, cursor.y)) return;
      const zone = Math.min(30, bounds.width / 3);
      const direction =
        cursor.x < bounds.left + zone
          ? -1
          : cursor.x > bounds.right - zone
            ? 1
            : 0;
      const before = viewport.scrollLeft;
      viewport.scrollLeft +=
        direction * Math.max(1, Math.min(32, now - previous) * 0.6);
      previous = now;
      show(targetAt(cursor.x, cursor.y));
      if (direction && viewport.scrollLeft !== before)
        frame.current = requestAnimationFrame(tick);
    };
    frame.current = requestAnimationFrame(tick);
  }
  function hover(x: number, y: number) {
    point.current = { x, y };
    const target = targetAt(x, y);
    if (!target) {
      clear();
      return null;
    }
    show(target);
    autoScroll();
    return target;
  }
  function drop(token: string, x: number, y: number) {
    const target = targetAt(x, y);
    clear();
    if (!target || lastDrop.current === token) return;
    lastDrop.current = token;
    void invoke("drop_tab", { token, beforeId: target.beforeId }).catch(
      (error) => callbacks.current.onError(error),
    );
  }
  useDesktopDrag((event) => {
    if (event.kind !== "workspaceTab") return;
    if (event.phase === "leave") clear();
    else if (event.phase === "drop") drop(event.token, event.x, event.y);
    else hover(event.x, event.y);
  });

  // The adjacent window-drag area is also an append target. Starting a window
  // drag still requires a fresh pointer-down there; an incoming tab never does.
  const handlers = useRef({ hover, drop, clear });
  handlers.current = { hover, drop, clear };
  useEffect(() => {
    const blank = appendDropRef?.current;
    if (!blank) return;
    const over = (event: DragEvent) => {
      if (!event.dataTransfer?.types.includes(dragType)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      handlers.current.hover(event.clientX, event.clientY);
    };
    const drop = (event: DragEvent) => {
      const token = event.dataTransfer?.getData(dragType);
      if (!token) return;
      event.preventDefault();
      handlers.current.drop(token, event.clientX, event.clientY);
    };
    const leave = (event: DragEvent) => {
      if (
        !(event.relatedTarget instanceof Node) ||
        !blank.contains(event.relatedTarget)
      )
        handlers.current.clear();
    };
    blank.addEventListener("dragover", over);
    blank.addEventListener("drop", drop);
    blank.addEventListener("dragleave", leave);
    return () => {
      blank.removeEventListener("dragover", over);
      blank.removeEventListener("drop", drop);
      blank.removeEventListener("dragleave", leave);
    };
  }, [appendDropRef]);

  useEffect(() => {
    let live = true;
    const cleanups: (() => void)[] = [];
    const subscribe = (pending: Promise<() => void>) => {
      void pending
        .then((off) => (live ? cleanups.push(off) : off()))
        .catch((error) => live && callbacks.current.onError(error));
    };
    if (desktop) {
      subscribe(
        getCurrentWindow().listen<{
          token: string;
          kind: string;
          cancelled: boolean;
        }>("desktop-drag-ended", ({ payload }) => {
          if (payload.kind !== "workspaceTab") return;
          handlers.current.clear();
          drag.current = null;
          void invoke("finish_tab_drag", {
            token: payload.token,
            cancelled: payload.cancelled,
          }).catch((error) => callbacks.current.onError(error));
        }),
      );
      subscribe(
        getCurrentWindow().listen<{ message: string }>(
          "desktop-drag-error",
          ({ payload }) => {
            handlers.current.clear();
            callbacks.current.onError(payload);
          },
        ),
      );
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (drag.current) drag.current.cancelled = true;
      handlers.current.clear();
    };
    document.addEventListener("keydown", escape);
    return () => {
      live = false;
      cleanups.forEach((off) => off());
      document.removeEventListener("keydown", escape);
      cancelAnimationFrame(frame.current);
    };
  }, []);

  return {
    over,
    native,
    beginNative(tabId: string) {
      const token = crypto.randomUUID();
      const ready = invoke("begin_workspace_drag", { token, tabId });
      drag.current = { token, ready, cancelled: false };
      return ready;
    },
    beginHtml(event: ReactDragEvent, tabId: string, label: string) {
      const token = crypto.randomUUID();
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData(dragType, token);
      event.dataTransfer.setData("text/plain", label);
      const ready = invoke("begin_tab_drag", { token, tabId });
      // Attach an error handler immediately, even if dragend never arrives.
      void ready.catch((error) => callbacks.current.onError(error));
      drag.current = { token, ready, cancelled: false };
    },
    endHtml() {
      clear();
      const pending = drag.current;
      drag.current = null;
      if (pending)
        void pending.ready
          .then(() =>
            invoke("finish_tab_drag", {
              token: pending.token,
              cancelled: pending.cancelled,
            }),
          )
          .catch((error) => callbacks.current.onError(error));
    },
    htmlHandlers: {
      onDragOver(event: ReactDragEvent) {
        if (!event.dataTransfer.types.includes(dragType)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        hover(event.clientX, event.clientY);
      },
      onDragLeave(event: ReactDragEvent) {
        if (
          !(event.relatedTarget instanceof Node) ||
          !event.currentTarget.contains(event.relatedTarget)
        )
          clear();
      },
      onDrop(event: ReactDragEvent) {
        const token = event.dataTransfer.getData(dragType);
        if (!token) return;
        event.preventDefault();
        drop(token, event.clientX, event.clientY);
      },
    },
  };
}
