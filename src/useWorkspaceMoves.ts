import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import type {
  WorkspaceController,
  WorkspaceSnapshot,
} from "./SessionWorkspace";
import type { Workspaces } from "./workspaces";
import { closeWorkspace, reorderWorkspace } from "./workspaces";
import type { AppError } from "./types";
import { errorOf } from "./types";

export function useWorkspaceMoves(
  setWorkspaces: Dispatch<SetStateAction<Workspaces>>,
  setError: (e: AppError | null) => void,
  detached = false,
) {
  const handles = useRef(new Map<string, WorkspaceController>());
  const snapshots = useRef(new Map<string, WorkspaceSnapshot>());
  const arrivals = useRef(new Map<string, string>());
  const receiving = useRef(new Set<string>());
  const leaving = useRef(new Map<string, string>());
  const [moving, setMoving] = useState(new Set<string>());
  const movingRef = useRef(new Set<string>());
  const mark = useCallback((id: string, value: boolean) => {
    if (value) movingRef.current.add(id);
    else movingRef.current.delete(id);
    setMoving(new Set(movingRef.current));
  }, []);
  const receive = useCallback(
    async (transferId: string) => {
      if (receiving.current.has(transferId)) return;
      receiving.current.add(transferId);
      try {
        const incoming = await invoke<{
          payload: WorkspaceSnapshot;
          beforeId: string | null;
        }>("take_workspace", { transferId });
        const tab = incoming.payload.tab;
        snapshots.current.set(tab.id, incoming.payload);
        arrivals.current.set(tab.id, transferId);
        setWorkspaces((current) => {
          const tabs = current.tabs.filter((t) => t.id !== tab.id);
          const index = incoming.beforeId
            ? tabs.findIndex((t) => t.id === incoming.beforeId)
            : -1;
          tabs.splice(index < 0 ? tabs.length : index, 0, tab);
          return { tabs, activeId: tab.id };
        });
      } catch (e) {
        setError(errorOf(e));
        void invoke("cancel_workspace_move", { transferId }).catch(() => {});
      }
    },
    [setWorkspaces, setError],
  );
  const move = useCallback(
    async (id: string, target: string | null, beforeId?: string | null) => {
      if (movingRef.current.has(id)) return;
      const handle = handles.current.get(id);
      if (!handle) return;
      mark(id, true);
      setError(null);
      try {
        const payload = await handle.prepareMove();
        const transferId = await invoke<string>("move_workspace", {
          payload,
          target,
          beforeId: beforeId || null,
        });
        // Completion may arrive before invoke resolves; never resurrect a completed move.
        if (movingRef.current.has(id)) leaving.current.set(id, transferId);
      } catch (e) {
        await handle.cancelMove();
        mark(id, false);
        setError(errorOf(e));
      }
    },
    [mark, setError],
  );
  const callbacks = useRef({ receive, move });
  callbacks.current = { receive, move };
  useEffect(() => {
    if (!desktop) return;
    const window = getCurrentWindow();
    let disposed = false;
    const unlisten: (() => void)[] = [];
    const listen = async <T>(name: string, callback: (payload: T) => void) => {
      const cleanup = await window.listen<T>(name, (event) =>
        callback(event.payload),
      );
      if (disposed) cleanup();
      else unlisten.push(cleanup);
    };
    void Promise.all([
      listen<string>("workspace-arriving", (id) => {
        void callbacks.current.receive(id);
      }),
      listen<{ tabId: string; transferId: string }>(
        "workspace-moved",
        ({ tabId }) => {
          leaving.current.delete(tabId);
          mark(tabId, false);
          setWorkspaces((current) => closeWorkspace(current, tabId));
          snapshots.current.delete(tabId);
          if (detached) void window.close();
        },
      ),
      listen<{ tabId: string; transferId: string; message: string }>(
        "workspace-move-failed",
        ({ tabId, transferId, message }) => {
          if (arrivals.current.get(tabId) === transferId) {
            arrivals.current.delete(tabId);
            setWorkspaces((current) => closeWorkspace(current, tabId));
            snapshots.current.delete(tabId);
          } else {
            void handles.current.get(tabId)?.cancelMove();
            leaving.current.delete(tabId);
            mark(tabId, false);
          }
          setError({ kind: "window", message });
        },
      ),
      listen<{ tabId: string; target: string | null; beforeId: string | null }>(
        "tab-drop",
        ({ tabId, target, beforeId }) => {
          if (target === window.label)
            setWorkspaces((current) =>
              reorderWorkspace(current, tabId, beforeId),
            );
          else void callbacks.current.move(tabId, target, beforeId);
        },
      ),
    ])
      .then(() => {
        const initial = new URLSearchParams(location.search).get("workspace");
        if (initial && !disposed) void callbacks.current.receive(initial);
      })
      .catch((e) => setError(errorOf(e)));
    return () => {
      disposed = true;
      unlisten.forEach((fn) => fn());
    };
  }, [setWorkspaces, setError, detached, mark]);
  const ready = useCallback(
    (id: string) => {
      const transferId = arrivals.current.get(id);
      if (!transferId) return;
      void invoke("workspace_ready", { transferId })
        .then(() => {
          arrivals.current.delete(id);
          handles.current.get(id)?.resumeRecovery();
        })
        .catch((e) => {
          setError(errorOf(e));
          void invoke("cancel_workspace_move", { transferId });
        });
    },
    [setError],
  );
  const cancel = useCallback((id: string) => {
    const transferId = leaving.current.get(id);
    if (transferId) void invoke("cancel_workspace_move", { transferId });
  }, []);
  return { handles, snapshots, moving, move, ready, cancel };
}
