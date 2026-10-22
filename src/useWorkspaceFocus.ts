import { useCallback, useLayoutEffect, useRef, type RefObject } from "react";
import type { TerminalController } from "./TerminalPane";
import type { ServiceTool, ToolSide } from "./workspaceTools";
import {
  canRestoreFocus,
  type WorkspaceInteraction,
} from "./WorkspaceInteractionContext";
import type { WorkspaceVisibility } from "./workspaceSizing";
export function useWorkspaceFocus(
  root: RefObject<HTMLElement | null>,
  active: boolean,
  visible: WorkspaceVisibility,
  tool: ServiceTool,
  terminal: RefObject<TerminalController | null>,
) {
  const last = useRef<HTMLElement | null>(null);
  const previousActive = useRef(false);
  const pending = useRef<"terminal" | ToolSide | null>(null);
  const current = useRef({ active, visible, tool });
  current.current = { active, visible, tool };
  const rail = useCallback(
    (side: ToolSide) =>
      root.current?.querySelector<HTMLElement>(
        `[data-tool="${side === "files" ? "files" : current.current.tool}"]`,
      ) ?? null,
    [root],
  );
  const restoreFocus = useCallback(
    (preferred: HTMLElement | null, side: ToolSide) => {
      if (!current.current.active || document.querySelector("dialog[open]"))
        return;
      if (
        preferred &&
        root.current?.contains(preferred) &&
        canRestoreFocus(preferred)
      )
        preferred.focus({ preventScroll: true });
      else rail(side)?.focus({ preventScroll: true });
    },
    [root, rail],
  );
  useLayoutEffect(() => {
    const element = root.current;
    const remember = (e: FocusEvent) => {
      if (e.target instanceof HTMLElement) last.current = e.target;
    };
    element?.addEventListener("focusin", remember);
    return () => element?.removeEventListener("focusin", remember);
  }, [root]);
  useLayoutEffect(() => {
    const entering = active && !previousActive.current;
    previousActive.current = active;
    if (!active || document.querySelector("dialog[open]")) return;
    if (pending.current) {
      const target = pending.current;
      pending.current = null;
      if (target === "terminal" && visible.terminal) terminal.current?.focus();
      else if (target !== "terminal")
        rail(target)?.focus({ preventScroll: true });
      return;
    }
    const focused = document.activeElement as HTMLElement | null;
    // A passive resize must never steal focus from another valid control.
    if (
      !entering &&
      focused &&
      focused !== document.body &&
      canRestoreFocus(focused)
    )
      return;
    if (entering && canRestoreFocus(last.current))
      last.current.focus({ preventScroll: true });
    else if (
      !entering &&
      last.current?.closest(".workspace-slot")?.hasAttribute("hidden")
    ) {
      restoreFocus(
        null,
        last.current.closest(".workspace-files") ? "files" : "services",
      );
    } else if (entering) {
      if (visible.terminal) terminal.current?.focus();
      else rail(visible.files ? "files" : "services")?.focus();
    }
  }, [
    active,
    visible.files,
    visible.services,
    visible.terminal,
    tool,
    rail,
    restoreFocus,
    terminal,
  ]);
  const terminalReady = useCallback(() => {
    if (
      !current.current.active ||
      !current.current.visible.terminal ||
      document.querySelector("dialog[open]")
    )
      return;
    const focused = document.activeElement as HTMLElement | null;
    if (!focused || focused === document.body || !canRestoreFocus(focused))
      terminal.current?.focus();
  }, [terminal]);
  const requestFocus = (target: "terminal" | ToolSide) => {
    pending.current = target;
  };
  const interaction: WorkspaceInteraction = {
    active,
    visible,
    serviceTool: tool,
    restoreFocus,
  };
  return { interaction, requestFocus, terminalReady };
}
