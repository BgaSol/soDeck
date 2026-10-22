import { useCallback, useEffect, useState, type RefObject } from "react";
import type { WorkspaceController } from "./ServerWorkspace";
import type { WorkspaceToolView } from "./WorkspaceToolConnection";
import type { WorkspaceTab } from "./workspaces";
import { forwardApi } from "./portForwardApi";

/** Transient menu state stays outside workspace snapshots in both window types. */
export function useWorkspaceToolActions(
  tabs: WorkspaceTab[],
  moving: Set<string>,
  handles: RefObject<Map<string, WorkspaceController>>,
  onError: (error: unknown) => void,
) {
  const [toolStates, setToolStates] = useState<
    Record<string, WorkspaceToolView>
  >({});
  const onToolState = useCallback((id: string, state: WorkspaceToolView) => {
    setToolStates((previous) => {
      const old = previous[id];
      return old?.busy === state.busy &&
        old?.error === state.error &&
        old?.promptPending === state.promptPending
        ? previous
        : { ...previous, [id]: state };
    });
  }, []);
  useEffect(() => {
    setToolStates((previous) => {
      const entries = Object.entries(previous).filter(([id]) =>
        tabs.some((tab) => tab.id === id),
      );
      return entries.length === Object.keys(previous).length
        ? previous
        : Object.fromEntries(entries);
    });
  }, [tabs]);
  return {
    toolStates,
    onToolState,
    onToolAction: (id: string, action: "connection" | "forward") => {
      const tab = tabs.find((tab) => tab.id === id);
      if (!tab || tab.lifecycle !== "active" || moving.has(id)) return;
      if (action === "connection") handles.current.get(id)?.runToolConnection();
      else void forwardApi.open(tab.server.id).catch(onError);
    },
  };
}
