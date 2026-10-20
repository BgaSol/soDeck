import { useLayoutEffect, useRef } from "react";
import type { ClipboardViewState } from "./types";
export function TerminalContextMenu({
  x,
  y,
  state,
  labels,
  readonly,
  onAction,
  onClose,
}: {
  x: number;
  y: number;
  state: ClipboardViewState;
  labels: { copy: string; paste: string };
  readonly: boolean;
  onAction(
    action:
      | "copy"
      | "paste"
      | "preview"
      | "selectAll"
      | "ctrlC"
      | "ctrlV"
      | "settings",
  ): void;
  onClose(restoreFocus: boolean): void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = menu.current!;
    const rect = node.getBoundingClientRect();
    node.style.left =
      Math.max(6, Math.min(x, window.innerWidth - rect.width - 6)) + "px";
    node.style.top =
      Math.max(6, Math.min(y, window.innerHeight - rect.height - 6)) + "px";
    node.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!node.contains(event.target as Node)) onClose(false);
    };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, []);
  return (
    <div
      ref={menu}
      className="terminal-context-menu"
      role="menu"
      aria-label="终端菜单"
      style={{ left: x, top: y }}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.key === "Escape" || event.key === "Tab") {
          event.preventDefault();
          event.stopPropagation();
          onClose(true);
        }
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault();
          const buttons = Array.from(
            menu.current!.querySelectorAll<HTMLButtonElement>(
              "button:not(:disabled)",
            ),
          );
          const i = buttons.indexOf(
            document.activeElement as HTMLButtonElement,
          );
          buttons[
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? buttons.length - 1
                : (i + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) %
                  buttons.length
          ]?.focus();
        }
      }}
    >
      <button
        role="menuitem"
        disabled={!state.hasSelection}
        onClick={() => onAction("copy")}
      >
        复制<kbd>{labels.copy}</kbd>
      </button>
      {!readonly && (
        <>
          <button
            role="menuitem"
            disabled={!state.canPaste || state.busy}
            title={state.unavailableReason}
            onClick={() => onAction("paste")}
          >
            粘贴<kbd>{labels.paste}</kbd>
          </button>
          <button
            role="menuitem"
            disabled={!state.canPaste || state.busy}
            title={state.unavailableReason}
            onClick={() => onAction("preview")}
          >
            粘贴前预览…
          </button>
        </>
      )}
      <button role="menuitem" onClick={() => onAction("selectAll")}>
        全选缓冲区
      </button>
      {!readonly && (
        <>
          <div role="separator" />
          <button
            role="menuitem"
            disabled={!state.canPaste || state.busy}
            onClick={() => onAction("ctrlC")}
          >
            发送按键 Ctrl+C
          </button>
          <button
            role="menuitem"
            disabled={!state.canPaste || state.busy}
            onClick={() => onAction("ctrlV")}
          >
            发送按键 Ctrl+V
          </button>
        </>
      )}
      <div role="separator" />
      <button role="menuitem" onClick={() => onAction("settings")}>
        复制粘贴设置…
      </button>
    </div>
  );
}
