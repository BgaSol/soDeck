import { useCallback, useEffect, useState } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { windowTitle } from "./brand";
import { desktop } from "./api";
import { SessionWorkspace } from "./SessionWorkspace";
import { WorkspaceTitlebar } from "./WorkspaceTitlebar";
import { FileTaskPanel } from "./FileTaskPanel";
import { DesktopDragPreview } from "./DesktopDragPreview";
import { useWorkspaceMoves } from "./useWorkspaceMoves";
import { emptyWorkspaces, reorderWorkspace } from "./workspaces";
import type { WorkspaceTab } from "./workspaces";
import type { AppError } from "./types";
import { errorOf } from "./types";

export function DetachedApp() {
  const [workspaces, setWorkspaces] = useState(emptyWorkspaces);
  const [error, setError] = useState<AppError | null>(null);
  const moves = useWorkspaceMoves(setWorkspaces, setError, true);
  const activeServerName = workspaces.tabs.find(
    (tab) => tab.id === workspaces.activeId,
  )?.server.name;
  useEffect(() => {
    if (!desktop || !activeServerName) return;
    void getCurrentWindow()
      .setTitle(windowTitle(activeServerName))
      .catch(() => {});
  }, [activeServerName]);
  const update = useCallback(
    (id: string, patch: Partial<Pick<WorkspaceTab, "server" | "state">>) =>
      setWorkspaces((current) => ({
        ...current,
        tabs: current.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)),
      })),
    [],
  );
  return (
    <div className="app-shell detached-app">
      <WorkspaceTitlebar
        tabs={workspaces.tabs}
        activeId={workspaces.activeId}
        detached
        moving={moves.moving}
        onSelect={(id) => setWorkspaces((c) => ({ ...c, activeId: id }))}
        onClose={() => void getCurrentWindow().close()}
        onMove={(...args) => void moves.move(...args)}
        onReorder={(id, before) =>
          setWorkspaces((c) => reorderWorkspace(c, id, before))
        }
        onNewConnection={(id) => {
          const tab = workspaces.tabs.find((item) => item.id === id);
          if (tab && !moves.moving.has(id))
            void invoke("request_workspace_connection", {
              serverId: tab.server.id,
              requestId: crypto.randomUUID(),
            }).catch((error) => setError(errorOf(error)));
        }}
        onError={(error) => setError(errorOf(error))}
      />
      <div className="workspaces">
        {error && (
          <div className="connection-error" role="alert">
            {error.message}
          </div>
        )}
        <div className="session-pages">
          {workspaces.tabs.map((tab) => (
            <div
              className="workspace-page-wrap"
              key={tab.id}
              hidden={workspaces.activeId !== tab.id}
            >
              <SessionWorkspace
                tab={tab}
                active={workspaces.activeId === tab.id}
                initial={moves.snapshots.current.get(tab.id)}
                onReady={() => moves.ready(tab.id)}
                controllerRef={(value) => {
                  if (value) moves.handles.current.set(tab.id, value);
                  else moves.handles.current.delete(tab.id);
                }}
                onChange={update}
              />
              {moves.moving.has(tab.id) && (
                <div className="session-moving">
                  <span>正在移动会话…</span>
                  <button onClick={() => moves.cancel(tab.id)}>取消移动</button>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      <FileTaskPanel />
      <DesktopDragPreview />
    </div>
  );
}
