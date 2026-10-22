import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  type RefObject,
} from "react";
import type { ServiceTool, ToolSide } from "./workspaceTools";
import type { WorkspaceVisibility } from "./workspaceSizing";
export function canRestoreFocus(
  target: HTMLElement | null | undefined,
): target is HTMLElement {
  return (
    !!target?.isConnected &&
    !target.closest("[hidden], [inert]") &&
    !target.matches(":disabled") &&
    target.getClientRects().length > 0
  );
}
export interface WorkspaceInteraction {
  active: boolean;
  visible: WorkspaceVisibility;
  serviceTool: ServiceTool;
  restoreFocus: (preferred: HTMLElement | null, side: ToolSide) => void;
}
export const WorkspaceInteractionContext =
  createContext<WorkspaceInteraction | null>(null);
/** Keep a presented modal stable through layout changes, but suspend it with its session. */
export function useWorkspaceDialog(
  ref: RefObject<HTMLDialogElement | null>,
  side: ToolSide,
  enabled = true,
  serviceTool?: ServiceTool,
) {
  const workspace = useContext(WorkspaceInteractionContext);
  const latest = useRef(workspace);
  latest.current = workspace;
  const presented = useRef(false);
  const previous = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (!enabled) {
      dialog.close();
      presented.current = false;
      return;
    }
    const ownerVisible =
      !workspace ||
      (workspace.visible[side] &&
        (!serviceTool || workspace.serviceTool === serviceTool));
    if (
      (!workspace || workspace.active) &&
      (presented.current || ownerVisible)
    ) {
      if (!dialog.open) {
        if (!presented.current)
          previous.current = document.activeElement as HTMLElement | null;
        dialog.showModal();
        presented.current = true;
        dialog
          .querySelector<HTMLElement>("[data-initial-focus]")
          ?.focus({ preventScroll: true });
      }
    } else if (dialog.open) dialog.close();
  }, [
    ref,
    workspace?.active,
    workspace?.visible[side],
    workspace?.serviceTool,
    side,
    enabled,
    serviceTool,
  ]);
  useLayoutEffect(() => {
    const dialog = ref.current;
    return () => {
      dialog?.close();
      if (!presented.current) return;
      // Run after a chained dialog has mounted and after React has removed this one.
      queueMicrotask(() => {
        if (document.querySelector("dialog[open]")) return;
        if (latest.current) latest.current.restoreFocus(previous.current, side);
        else if (canRestoreFocus(previous.current))
          previous.current.focus({ preventScroll: true });
      });
    };
  }, [ref, side]);
}
