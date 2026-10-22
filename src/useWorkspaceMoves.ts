import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { workspaceApi } from "./workspaceApi";
import type {
  WorkspaceController,
  WorkspaceSnapshot,
} from "./SessionWorkspace";
import type { Workspaces } from "./workspaces";
import {
  projectWorkspace,
  closeWorkspace,
  reorderWorkspace,
} from "./workspaces";
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
  const finished = useRef(new Set<string>());
  const remember = useCallback((transferId: string) => {
    finished.current.add(transferId);
    if (finished.current.size > 4096)
      finished.current.delete(finished.current.values().next().value!);
  }, []);
  const [moving, setMoving] = useState(new Set<string>());
  const movingRef = useRef(new Set<string>());
  const mark = useCallback((id: string, value: boolean) => {
    if (value) movingRef.current.add(id);
    else movingRef.current.delete(id);
    setMoving(new Set(movingRef.current));
  }, []);
  const receive = useCallback(
    async (transferId: string) => {
      if (
        receiving.current.has(transferId) ||
        finished.current.has(transferId) ||
        [...arrivals.current.values()].includes(transferId)
      )
        return;
      receiving.current.add(transferId);
      try {
        const incoming = await invoke<{
          payload: WorkspaceSnapshot;
          beforeId: string | null;
        }>("take_workspace", { transferId });
        if (finished.current.has(transferId)) return;
        const tab = projectWorkspace(incoming.payload.tab);
        incoming.payload.tab = tab;
        for (const t of incoming.payload.terminals)
          t.terminal.transferId = transferId;
        mark(tab.id, true);
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
        if (finished.current.has(transferId)) return;
        remember(transferId);
        setError(errorOf(e));
        void invoke("cancel_workspace_move", { transferId }).catch(() => {});
      } finally {
        receiving.current.delete(transferId);
      }
    },
    [setWorkspaces, setError, mark, remember],
  );
  const move = useCallback(
    async (id: string, target: string | null, beforeId?: string | null) => {
      if (movingRef.current.has(id)) return;
      const handle = handles.current.get(id);
      if (!handle) return;
      mark(id, true);
      setError(null);
      let preparedId: string | undefined;
      try {
        const payload = await handle.prepareMove((transferId) => {
          preparedId = transferId;
          leaving.current.set(id, transferId);
        });
        if (finished.current.has(payload.transferId)) return;
        await invoke<string>("move_workspace", {
          payload,
          target,
          beforeId: beforeId || null,
        });
      } catch (e) {
        if (preparedId && finished.current.has(preparedId)) return;
        // The target may have committed before a source IPC response was lost.
        if (preparedId) {
          const latest = await workspaceApi.list().catch(() => null);
          if (finished.current.has(preparedId)) return;
          const workspace = latest?.workspaces.find((w) => w.id === id);
          if (workspace && workspace.owner.label !== getCurrentWindow().label) {
            remember(preparedId);
            leaving.current.delete(id);
            mark(id, false);
            snapshots.current.delete(id);
            setWorkspaces((current) => closeWorkspace(current, id));
            if (detached) void getCurrentWindow().close();
            return;
          }
        }
        await handle.cancelMove(preparedId);
        if (preparedId) remember(preparedId);
        leaving.current.delete(id);
        mark(id, false);
        setError(errorOf(e));
      }
    },
    [mark, setError, setWorkspaces, detached, remember],
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
        ({ tabId, transferId }) => {
          if (leaving.current.get(tabId) !== transferId) return;
          remember(transferId);
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
          if (finished.current.has(transferId)) return;
          remember(transferId);
          if (arrivals.current.get(tabId) === transferId) {
            arrivals.current.delete(tabId);
            mark(tabId, false);
            setWorkspaces((current) => closeWorkspace(current, tabId));
            snapshots.current.delete(tabId);
          } else if (leaving.current.get(tabId) === transferId) {
            void Promise.resolve(
              handles.current.get(tabId)?.cancelMove(transferId),
            )
              .catch((e) => setError(errorOf(e)))
              .finally(() => {
                if (leaving.current.get(tabId) !== transferId) return;
                leaving.current.delete(tabId);
                mark(tabId, false);
              });
          } else return;
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
  }, [setWorkspaces, setError, detached, mark, remember]);
  const ready = useCallback(
    (id: string) => {
      const transferId = arrivals.current.get(id);
      if (!transferId) return;
      void invoke("workspace_ready", { transferId })
        .then(() => {
          if (arrivals.current.get(id) !== transferId) return;
          remember(transferId);
          arrivals.current.delete(id);
          mark(id, false);
          handles.current.get(id)?.resumeRecovery();
        })
        .catch(async (e) => {
          if (arrivals.current.get(id) !== transferId) return;
          const status = await invoke<string>("workspace_move_status", {
            transferId,
          }).catch(() => "gone");
          if (arrivals.current.get(id) !== transferId) return;
          if (status === "committed") {
            remember(transferId);
            arrivals.current.delete(id);
            mark(id, false);
            handles.current.get(id)?.resumeRecovery();
            return;
          }
          setError(errorOf(e));
          await invoke("cancel_workspace_move", { transferId }).catch(() => {});
          if (arrivals.current.get(id) !== transferId) return;
          remember(transferId);
          arrivals.current.delete(id);
          snapshots.current.delete(id);
          mark(id, false);
          setWorkspaces((current) => closeWorkspace(current, id));
        });
    },
    [setError, setWorkspaces, mark, remember],
  );
  const cancel = useCallback((id: string) => {
    const transferId = leaving.current.get(id);
    if (transferId) void invoke("cancel_workspace_move", { transferId });
  }, []);
  return { handles, snapshots, moving, move, ready, cancel };
}
