import {
  lazy,
  Component,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TerminalSquare, X, ScrollText } from "lucide-react";
import { WindowTitlebar, WindowLoading } from "./WindowTitlebar";
import type { ReactNode } from "react";
import { api, desktop } from "./api";
import { BrandIcon } from "./AppBrand";
import { APP_TAGLINE } from "./brand";
import { TerminalPane } from "./TerminalPane";
import { ServerEditor } from "./ServerEditor";
import { ServerDeleteDialog } from "./ServerDeleteDialog";
import { DatabaseResetDialog } from "./DatabaseResetDialog";
import { logApi } from "./logApi";
import { terminalHistoryApi } from "./terminalHistoryApi";
import { FilePanel } from "./FilePanel";
import { DockerPanel } from "./DockerPanel";
import { WorkspaceTitlebar } from "./WorkspaceTitlebar";
import { DetachedApp } from "./DetachedApp";
import { TransferPanel } from "./TransferPanel";
import { DesktopDragPreview } from "./DesktopDragPreview";
import { transferApi, transferActive } from "./transferApi";
import { QuitDialog } from "./QuitDialog";
import { useWorkspaceMoves } from "./useWorkspaceMoves";
import { SessionWorkspace } from "./SessionWorkspace";
import { ServerPickerDialog } from "./ServerPickerDialog";
import { WorkspaceLayout } from "./WorkspaceLayout";
import { emptyServer, errorOf, filterServers } from "./types";
import {
  closeWorkspace,
  emptyWorkspaces,
  openWorkspace,
  reorderWorkspace,
} from "./workspaces";
import type { AppError, Server, SaveServerRequest } from "./types";
import type { WorkspaceTab } from "./workspaces";
const FileEditorApp = lazy(() =>
  import("./FileEditorApp").then(({ FileEditorApp }) => ({
    default: FileEditorApp,
  })),
);
const ContainerLogsApp = lazy(() =>
  import("./ContainerLogsApp").then(({ ContainerLogsApp }) => ({
    default: ContainerLogsApp,
  })),
);
const LogArchives = lazy(() =>
  import("./LogArchives").then(({ LogArchives }) => ({ default: LogArchives })),
);
const TerminalHistoryApp = lazy(() =>
  import("./TerminalHistoryView").then(({ TerminalHistoryApp }) => ({
    default: TerminalHistoryApp,
  })),
);
function ContainerApp() {
  const [info, setInfo] = useState<{
      serverName: string;
      containerName: string;
    } | null>(null),
    [error, setError] = useState<AppError | null>(null),
    [state, setState] = useState("连接中"),
    [attempt, setAttempt] = useState(0);
  useEffect(() => {
    void invoke<{ serverName: string; containerName: string }>("container_info")
      .then(setInfo)
      .catch((e) => setError(errorOf(e)));
  }, []);
  return (
    <div className="container-app">
      <WindowTitlebar
        leading={<TerminalSquare size={18} />}
        title={info ? `${info.serverName} / ${info.containerName}` : "容器终端"}
        onError={(error) => setError(errorOf(error))}
        actions={
          <>
            <span className="muted">{state}</span>
            {state !== "已连接" && state !== "连接中" && (
              <button
                onClick={() => {
                  setError(null);
                  setAttempt((a) => a + 1);
                }}
              >
                重新进入
              </button>
            )}
          </>
        }
      />
      {error && (
        <div className="connection-error" role="alert">
          {error.message}
        </div>
      )}
      <div className="container-terminal">
        <TerminalPane
          key={attempt}
          container
          onSession={() => {}}
          onError={setError}
          onState={setState}
        />
      </div>
      <footer className="workspace-status">
        独立容器会话<span>UTF-8</span>
      </footer>
    </div>
  );
}
export function App() {
  const [servers, setServers] = useState<Server[]>([]),
    [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState(""),
    [tag, setTag] = useState(""),
    [editor, setEditor] = useState<Server | null>(null);
  const [workspaces, setWorkspaces] = useState(emptyWorkspaces),
    [error, setError] = useState<AppError | null>(null),
    [loading, setLoading] = useState(true);
  const [databaseResetDetails, setDatabaseResetDetails] = useState<
    string | null
  >(null);
  const [databaseNotice, setDatabaseNotice] = useState("");
  const [deleting, setDeleting] = useState<Server | null>(null);
  const [openingArchives, setOpeningArchives] = useState(false);
  const [quitRequest, setQuitRequest] = useState<{
    details: string;
    discardUnsaved: boolean;
  } | null>(null);
  const closing = useRef(false);
  async function resetDatabase() {
    // Keep backup errors in the dialog so the user can retry. Once reset has
    // succeeded, retain its backup path even if refreshing the list fails.
    const backup = await api.resetIncompatibleDatabase();
    setDatabaseNotice(`数据库已重新创建。旧数据备份：${backup}`);
    setDatabaseResetDetails(null);
    setError(null);
    try {
      const values = await api.listServers();
      setServers(values);
      setSelectedId(values[0]?.id || "");
    } catch (error) {
      setError(errorOf(error));
    }
  }
  const [pickerOpen, setPickerOpen] = useState(false);
  const [promptTab, setPromptTab] = useState<string | null>(null);
  type PendingConnection = {
    tabId: string;
    serverId: string;
    returnId: string;
    attempt: number;
    busy: boolean;
    error?: string;
  };
  const [pending, setPendingState] = useState<PendingConnection | null>(null);
  const pendingRef = useRef<PendingConnection | null>(null);
  const setPending = useCallback((value: PendingConnection | null) => {
    pendingRef.current = value;
    setPendingState(value);
  }, []);
  const pickerWasOpen = useRef(false);
  const focusTerminal = useRef(false);
  useEffect(() => {
    if (pickerOpen) {
      pickerWasOpen.current = true;
      return;
    }
    if (!pickerWasOpen.current) return;
    pickerWasOpen.current = false;
    const frame = requestAnimationFrame(() => {
      const selector = focusTerminal.current
        ? ".workspace-page-wrap:not([hidden]) .xterm-helper-textarea"
        : ".new-session";
      document.querySelector<HTMLElement>(selector)?.focus();
      focusTerminal.current = false;
    });
    return () => cancelAnimationFrame(frame);
  }, [pickerOpen]);
  const moves = useWorkspaceMoves(setWorkspaces, setError);
  const tags = [...new Set(servers.flatMap((server) => server.tags))].sort(),
    visible = filterServers(servers, query, tag);
  useEffect(() => {
    if (!desktop) {
      setLoading(false);
      return;
    }
    void api
      .listServers()
      .then((values) => {
        setServers(values);
        setSelectedId(values[0]?.id || "");
      })
      .catch((error) => setError(errorOf(error)))
      .finally(() => setLoading(false));
    let cleanup: undefined | (() => void),
      disposed = false;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        event.preventDefault();
        if (closing.current) return;
        closing.current = true;
        const count = await invoke<number>("session_count").catch(() => 0);
        const logs = await invoke<number>("log_session_count").catch(() => 0);
        const dirty = await invoke<number>("unsaved_editor_count").catch(
          () => 0,
        );
        const transfers = await transferApi.list().catch(() => []);
        const activeTransfers = transfers.filter(transferActive).length;
        const unknown = transfers.filter((t) =>
          ["unknown", "needsAttention", "interrupted"].includes(t.status),
        ).length;
        const details = [
          count ? `仍有 ${count} 个活动终端` : "",
          logs ? `${logs} 个日志窗口正在接收` : "",
          dirty ? `${dirty} 个文件有未保存修改` : "",
          activeTransfers ? `${activeTransfers} 个传输尚未完成` : "",
          unknown ? `${unknown} 个传输结果待核对` : "",
        ]
          .filter(Boolean)
          .join("，");
        if (details) setQuitRequest({ details, discardUnsaved: dirty > 0 });
        else
          void invoke("quit_app", { discardUnsaved: false }).catch((error) => {
            closing.current = false;
            setError(errorOf(error));
          });
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);
  const updateTab = useCallback(
    (id: string, patch: Partial<Pick<WorkspaceTab, "server" | "state">>) => {
      setWorkspaces((current) => ({
        ...current,
        tabs: current.tabs.map((tab) =>
          tab.id === id ? { ...tab, ...patch } : tab,
        ),
      }));
    },
    [],
  );
  const refreshConfigurations = useCallback(() => {
    void api
      .listServers()
      .then(setServers)
      .catch((error) => setError(errorOf(error)));
  }, []);
  useEffect(() => {
    if (!desktop) return;
    let live = true;
    let off: (() => void) | undefined;
    void getCurrentWindow()
      .listen("server-configurations-changed", refreshConfigurations)
      .then((cleanup) => {
        if (live) off = cleanup;
        else cleanup();
      });
    return () => {
      live = false;
      off?.();
    };
  }, [refreshConfigurations]);
  function discardPending() {
    const current = pendingRef.current;
    setPending(null);
    setPromptTab(null);
    if (current)
      setWorkspaces((state) => {
        const next = closeWorkspace(state, current.tabId);
        return {
          ...next,
          activeId: next.tabs.some((t) => t.id === current.returnId)
            ? current.returnId
            : next.activeId,
        };
      });
  }
  function closePicker() {
    discardPending();
    setPickerOpen(false);
  }
  const receiveStatus = useCallback(
    (id: string, attempt: number, state: string, failure?: AppError) => {
      const current = pendingRef.current;
      if (!current || current.tabId !== id || current.attempt !== attempt)
        return;
      if (state === "已连接") {
        focusTerminal.current = true;
        setPending(null);
        setPromptTab(null);
        setPickerOpen(false);
      } else if (state === "连接失败" || (state === "error" && !current.busy)) {
        setPending({
          ...current,
          busy: false,
          error: failure?.message || current.error || "连接失败，请重试。",
        });
      } else if (state === "已断开") {
        setPending(null);
        setPromptTab(null);
        setPickerOpen(false);
        setWorkspaces((value) => {
          const next = closeWorkspace(value, id);
          return {
            ...next,
            activeId: next.tabs.some((t) => t.id === current.returnId)
              ? current.returnId
              : next.activeId,
          };
        });
      }
    },
    [setPending],
  );
  const receivePrompt = useCallback((id: string, visible: boolean) => {
    setPromptTab((current) => (visible ? id : current === id ? null : current));
  }, []);
  function open(server: Server) {
    if (!desktop || pendingRef.current?.busy) return;
    if (!server.username) {
      setError({ kind: "validation", message: "请先填写 SSH 用户名。" });
      discardPending();
      setEditor(structuredClone(server));
      return;
    }
    setError(null);
    const previous = pendingRef.current;
    if (
      previous?.serverId === server.id &&
      moves.handles.current.has(previous.tabId)
    ) {
      setPending({
        ...previous,
        attempt: previous.attempt + 1,
        busy: true,
        error: undefined,
      });
      moves.handles.current.get(previous.tabId)!.reconnect();
      return;
    }
    const tabId = crypto.randomUUID();
    setPending({
      tabId,
      serverId: server.id,
      returnId: previous?.returnId ?? workspaces.activeId,
      attempt: 0,
      busy: true,
    });
    setWorkspaces((current) =>
      openWorkspace(
        previous ? closeWorkspace(current, previous.tabId) : current,
        server,
        true,
        tabId,
      ),
    );
  }
  function editServer(server: Server) {
    discardPending();
    setEditor(structuredClone(server));
  }
  async function save(server: SaveServerRequest) {
    const saved = await api.saveServer(server);
    setServers((values) => [
      ...values.filter((server) => server.id !== saved.id),
      saved,
    ]);
    setSelectedId(saved.id);
    setQuery("");
    setTag("");
    setEditor(null);
    setError(null);
  }
  async function remove(server: Server) {
    if (workspaces.tabs.some((tab) => tab.server.id === server.id))
      throw {
        kind: "busy",
        message: "请先关闭此服务器的所有会话标签页，再删除配置。",
      };
    await api.deleteServer(server.id);
    const remaining = servers.filter((value) => value.id !== server.id);
    const remainingTag = remaining.some((value) => value.tags.includes(tag))
      ? tag
      : "";
    const candidates = filterServers(remaining, query, remainingTag);
    const index = visible.findIndex((value) => value.id === server.id);
    setServers((values) => values.filter((value) => value.id !== server.id));
    if (selectedId === server.id)
      setSelectedId(
        candidates[Math.max(0, Math.min(index, candidates.length - 1))]?.id ||
          "",
      );
    setTag(remainingTag);
    setEditor(null);
    setDeleting(null);
    setError(null);
  }
  function requestDelete(server: Server) {
    discardPending();
    setDeleting(server);
  }
  return (
    <div className="app-shell">
      <WorkspaceTitlebar
        tabs={workspaces.tabs}
        activeId={workspaces.activeId}
        moving={moves.moving}
        onSelect={(id) =>
          setWorkspaces((current) => ({ ...current, activeId: id }))
        }
        onClose={(id) => {
          if (!moves.moving.has(id))
            setWorkspaces((current) => closeWorkspace(current, id));
        }}
        onMove={(...args) => void moves.move(...args)}
        onReorder={(id, beforeId) =>
          setWorkspaces((current) => reorderWorkspace(current, id, beforeId))
        }
        onNew={desktop ? () => setPickerOpen(true) : undefined}
        onError={(error) => setError(errorOf(error))}
        actions={
          <>
            <button
              className="quiet"
              aria-label="终端历史"
              title="终端历史"
              disabled={!desktop}
              onClick={() =>
                void terminalHistoryApi
                  .open()
                  .catch((e) => setError(errorOf(e)))
              }
            >
              <TerminalSquare size={15} />
            </button>
            <button
              className="quiet"
              aria-label="本地日志"
              title="本地日志"
              disabled={!desktop || openingArchives}
              onClick={async () => {
                setOpeningArchives(true);
                try {
                  await logApi.openArchives();
                } catch (e) {
                  setError(errorOf(e));
                } finally {
                  setOpeningArchives(false);
                }
              }}
            >
              <ScrollText size={15} />
            </button>
          </>
        }
      />
      {!desktop && (
        <div className="browser-notice">
          请使用桌面客户端运行，服务器数据保存在本地 SQLite 文件中。
          <code>npm run desktop</code>
        </div>
      )}
      <div className="workspace">
        <div className="workspaces">
          {error && (
            <div className="connection-error" role="alert">
              <span style={{ whiteSpace: "pre-wrap" }}>{error.message}</span>
              {error.kind === "databaseSchema" && (
                <button onClick={() => setDatabaseResetDetails(error.message)}>
                  备份旧库并重建
                </button>
              )}
              <button
                className="icon-button"
                aria-label="关闭错误提示"
                onClick={() => setError(null)}
              >
                <X size={14} />
              </button>
            </div>
          )}
          {databaseNotice && (
            <div className="browser-notice" role="status">
              <span>{databaseNotice}</span>
              <button
                className="icon-button"
                aria-label="关闭数据库提示"
                onClick={() => setDatabaseNotice("")}
              >
                <X size={14} />
              </button>
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
                  onChange={updateTab}
                  onConfigurationChanged={refreshConfigurations}
                  onConnectionStatus={receiveStatus}
                  onPromptChange={receivePrompt}
                />
                {moves.moving.has(tab.id) && (
                  <div className="session-moving">
                    <span>正在移动会话…</span>
                    <button onClick={() => moves.cancel(tab.id)}>
                      取消移动
                    </button>
                  </div>
                )}
              </div>
            ))}
            {!workspaces.tabs.length && (
              <WorkspaceLayout
                files={<FilePanel sessionId={null} serverName="" />}
                services={
                  <DockerPanel sessionId={null} serverName="" active={false} />
                }
                terminal={
                  <main className="terminal-panel">
                    <div className="session-welcome">
                      <span className="welcome-symbol" aria-hidden="true">
                        <BrandIcon size={76} />
                      </span>
                      <span className="welcome-caption">{APP_TAGLINE}</span>
                      <h2>打开一个 SSH 连接</h2>
                      <p>点击上方标签栏右侧的“＋”，选择服务器并连接。</p>
                      <p>每个会话都有独立的终端、文件目录和 Docker 面板。</p>
                      <div className="welcome-features" aria-hidden="true">
                        <span>SSH 终端</span>
                        <span>SFTP 文件</span>
                        <span>Docker 服务</span>
                      </div>
                    </div>
                  </main>
                }
              />
            )}
          </div>
        </div>
      </div>
      <footer className="workspace-summary">
        <span>
          {workspaces.tabs.length} 个会话 ·{" "}
          {workspaces.tabs.filter((tab) => tab.state === "已连接").length}{" "}
          已连接
        </span>
        <div className="workspace-summary-end">
          <span>本地工作空间</span>
          <TransferPanel embedded />
        </div>
      </footer>
      <DesktopDragPreview />
      {pickerOpen && (
        <ServerPickerDialog
          servers={servers}
          selectedId={selectedId}
          query={query}
          tag={tag}
          loading={loading}
          busy={!!pending?.busy}
          suspended={
            !!editor ||
            !!deleting ||
            !!promptTab ||
            databaseResetDetails !== null ||
            !!quitRequest
          }
          error={pending?.error || error?.message}
          connectedIds={
            new Set(
              workspaces.tabs
                .filter((t) => t.state === "已连接")
                .map((t) => t.server.id),
            )
          }
          onSelect={setSelectedId}
          onQuery={setQuery}
          onTag={setTag}
          onConnect={open}
          onEdit={editServer}
          onDelete={requestDelete}
          onNew={() => editServer(emptyServer())}
          onClose={closePicker}
          onResetDatabase={
            error?.kind === "databaseSchema"
              ? () => setDatabaseResetDetails(error.message)
              : undefined
          }
        />
      )}
      {editor && (
        <ServerEditor
          initial={editor}
          servers={servers}
          tags={tags}
          onSave={save}
          onDelete={editor.id ? requestDelete : undefined}
          onClose={() => setEditor(null)}
        />
      )}
      {databaseResetDetails !== null && (
        <DatabaseResetDialog
          details={databaseResetDetails}
          onReset={resetDatabase}
          onClose={() => setDatabaseResetDetails(null)}
        />
      )}
      {deleting && (
        <ServerDeleteDialog
          server={deleting}
          blocked={workspaces.tabs.some((tab) => tab.server.id === deleting.id)}
          onDelete={remove}
          onClose={() => setDeleting(null)}
        />
      )}
      {quitRequest && (
        <QuitDialog
          details={quitRequest.details}
          onClose={() => {
            closing.current = false;
            setQuitRequest(null);
          }}
          onQuit={async () => {
            await invoke("quit_app", {
              discardUnsaved: quitRequest.discardUnsaved,
            });
            setQuitRequest(null);
          }}
        />
      )}
    </div>
  );
}
class WindowErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    if (this.state.failed)
      return (
        <div className="window-loading">
          <WindowTitlebar title="SoDeck" />
          <p role="alert">窗口内容加载失败，请关闭窗口后重试。</p>
        </div>
      );
    return this.props.children;
  }
}

export function Root() {
  const params = new URLSearchParams(location.search);
  let title = "SoDeck";
  let content: ReactNode;
  if (params.has("terminal-history")) {
    title = "终端历史";
    content = <TerminalHistoryApp />;
  } else if (params.has("archives")) {
    title = "本地日志";
    content = <LogArchives />;
  } else if (params.has("logs")) {
    title = "容器日志";
    content = <ContainerLogsApp />;
  } else if (params.has("editor")) {
    title = "远程文件编辑";
    content = <FileEditorApp />;
  } else if (params.has("workspace")) {
    content = <DetachedApp />;
  } else {
    content = params.has("container") ? <ContainerApp /> : <App />;
  }
  return (
    <WindowErrorBoundary>
      <Suspense fallback={<WindowLoading title={title} />}>{content}</Suspense>
    </WindowErrorBoundary>
  );
}
