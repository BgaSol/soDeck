import { useCallback, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TerminalSquare } from "lucide-react";
import { SessionWorkspace } from "./SessionWorkspace";
import { WorkspaceTabs } from "./WorkspaceTabs";
import { TransferPanel } from "./TransferPanel";
import { DesktopDragPreview } from "./DesktopDragPreview";
import { useWorkspaceMoves } from "./useWorkspaceMoves";
import { emptyWorkspaces, reorderWorkspace } from "./workspaces";
import type { WorkspaceTab } from "./workspaces";
import type { AppError } from "./types";

export function DetachedApp() {
  const [workspaces, setWorkspaces] = useState(emptyWorkspaces);
  const [error, setError] = useState<AppError | null>(null);
  const moves = useWorkspaceMoves(setWorkspaces, setError, true);
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
      <header className="app-header">
        <span className="app-mark">
          <TerminalSquare size={20} />
        </span>
        <strong>SSH 会话</strong>
        <span className="header-summary">拖动顶部会话标签可移回主窗口</span>
      </header>
      <div className="workspaces">
        <WorkspaceTabs
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
        />
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
      <TransferPanel />
      <DesktopDragPreview />
    </div>
  );
}
