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
import { TerminalPane } from "./TerminalPane";
import { ServerEditor } from "./ServerEditor";
import { ServerDeleteDialog } from "./ServerDeleteDialog";
import { DatabaseResetDialog } from "./DatabaseResetDialog";
import { logApi } from "./logApi";
import { terminalHistoryApi } from "./terminalHistoryApi";
import { WorkspaceTitlebar } from "./WorkspaceTitlebar";
import { DetachedApp } from "./DetachedApp";
import { FileTaskPanel } from "./FileTaskPanel";
import { DesktopDragPreview } from "./DesktopDragPreview";
import { fileTaskApi } from "./fileTaskApi";
import { QuitDialog } from "./QuitDialog";
import { PortForwardButton } from "./PortForwardButton";
import { forwardApi } from "./portForwardApi";
import { useWorkspaceMoves } from "./useWorkspaceMoves";
import { SessionWorkspace } from "./SessionWorkspace";
import { ServerHome } from "./ServerHome";
import { duplicateDraft, configurationFields } from "./serverConfiguration";
import { emptyServer, errorOf, filterServers } from "./types";
import {
  closeWorkspace,
  emptyWorkspaces,
  openWorkspace,
  reorderWorkspace,
} from "./workspaces";
import type {
  AppError,
  Server,
  SaveServerRequest,
  DuplicatePasswordPolicy,
} from "./types";
import type { WorkspaceTab } from "./workspaces";
const MonitorWindowApp = lazy(() =>
  import("./MonitorWindowApp").then(({ MonitorWindowApp }) => ({
    default: MonitorWindowApp,
  })),
);
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
const PortForwardApp = lazy(() =>
  import("./PortForwardApp").then(({ PortForwardApp }) => ({
    default: PortForwardApp,
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
      setSelectedId(filterServers(values, "", "")[0]?.id || "");
    } catch (error) {
      setError(errorOf(error));
    }
  }
  const [duplicateSource, setDuplicateSource] = useState<Server | null>(null);
  const [focusRequest, setFocusRequest] = useState(0);
  const [savedNotice, setSavedNotice] = useState<{
    text: string;
    serverId: string;
  } | null>(null);
  const lastVisited = useRef(new Map<string, number>());
  const navigationSerial = useRef(0);
  type PendingConnection = {
    tabId: string;
    serverId: string;
    returnId: string;
    attempt: number;
    busy: boolean;
    error?: string;
    navigationSerial: number;
  };
  const [pending, setPendingState] = useState<PendingConnection | null>(null);
  const pendingRef = useRef<PendingConnection | null>(null);
  const setPending = useCallback((value: PendingConnection | null) => {
    pendingRef.current = value;
    setPendingState(value);
  }, []);
  useEffect(() => {
    if (!workspaces.activeId) return;
    lastVisited.current.set(workspaces.activeId, performance.now());
    const frame = requestAnimationFrame(() => {
      document
        .querySelector<HTMLTextAreaElement>(
          ".workspace-page-wrap:not([hidden]) .xterm-helper-textarea",
        )
        ?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [workspaces.activeId]);
  function navigate(id: string) {
    navigationSerial.current++;
    setWorkspaces((current) => ({ ...current, activeId: id }));
    if (!id) setFocusRequest((value) => value + 1);
  }
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
        setSelectedId(filterServers(values, "", "")[0]?.id || "");
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
        let summary;
        let forwards;
        let docker;
        try {
          summary = (await fileTaskApi.list("all", null, 1)).summary;
          forwards = await forwardApi.summary();
          docker = await dockerApi.summary();
        } catch (e) {
          closing.current = false;
          setError(errorOf(e));
          return;
        }
        const details = [
          count ? `仍有 ${count} 个活动终端` : "",
          logs ? `${logs} 个日志窗口正在接收` : "",
          dirty ? `${dirty} 个文件有未保存修改` : "",
          docker?.active ? `${docker.active} 个 Docker 操作尚未完成` : "",
          docker?.attention
            ? `${docker.attention} 个 Docker 操作结果待确认`
            : "",
          summary.active
            ? `${summary.active} 个文件任务尚未完成（含 Docker 传输）`
            : "",
          summary.attention ? `${summary.attention} 个文件任务需要处理` : "",
          forwards.active
            ? `${forwards.active} 条端口转发仍在运行（${forwards.connections} 条业务连接）`
            : "",
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
    if (current)
      setWorkspaces((state) => {
        const next = closeWorkspace(state, current.tabId);
        return {
          ...next,
          activeId:
            state.activeId === current.tabId
              ? next.tabs.some((t) => t.id === current.returnId)
                ? current.returnId
                : ""
              : state.activeId,
        };
      });
  }
  const receiveStatus = useCallback(
    (id: string, attempt: number, state: string, failure?: AppError) => {
      const current = pendingRef.current;
      if (!current || current.tabId !== id || current.attempt !== attempt)
        return;
      if (state === "已连接") {
        setPending(null);
        if (current.navigationSerial === navigationSerial.current)
          setWorkspaces((value) => ({ ...value, activeId: id }));
      } else if (state === "连接失败" || (state === "error" && !current.busy)) {
        setPending({
          ...current,
          busy: false,
          error: failure?.message || current.error || "连接失败，请重试。",
        });
      } else if (state === "已断开") {
        setPending(null);

        setWorkspaces((value) => {
          const next = closeWorkspace(value, id);
          return {
            ...next,
            activeId:
              value.activeId === id
                ? next.tabs.some((t) => t.id === current.returnId)
                  ? current.returnId
                  : ""
                : value.activeId,
          };
        });
      }
    },
    [setPending],
  );
  function open(server: Server, forceNew = false) {
    if (!desktop) return;
    if (!forceNew) {
      const existing = workspaces.tabs
        .filter((tab) => tab.server.id === server.id && tab.state === "已连接")
        .sort(
          (a, b) =>
            (lastVisited.current.get(b.id) || 0) -
            (lastVisited.current.get(a.id) || 0),
        )[0];
      if (existing) {
        navigate(existing.id);
        return;
      }
    }
    if (pendingRef.current?.busy) return;
    if (!server.username) {
      setError({ kind: "validation", message: "请先填写 SSH 用户名。" });
      discardPending();
      setDuplicateSource(null);
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
        navigationSerial: navigationSerial.current,
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
      navigationSerial: navigationSerial.current,
    });
    setWorkspaces((current) => {
      const next = openWorkspace(
        previous ? closeWorkspace(current, previous.tabId) : current,
        server,
        true,
        tabId,
      );
      return {
        ...next,
        activeId: current.activeId === previous?.tabId ? "" : current.activeId,
      };
    });
  }
  function editServer(server: Server) {
    setDuplicateSource(null);
    setEditor(structuredClone(server));
  }
  function copyServer(server: Server) {
    setDuplicateSource(structuredClone(server));
    setEditor(duplicateDraft(server, servers));
  }
  async function save(
    server: SaveServerRequest,
    connect = false,
    passwordPolicy?: DuplicatePasswordPolicy,
    asNew = false,
  ) {
    const saved =
      duplicateSource && !asNew
        ? await api.duplicateServer({
            sourceId: duplicateSource.id,
            draft: configurationFields(server),
            passwordPolicy: passwordPolicy || { type: "clear" },
          })
        : await api.saveServer(server);
    setServers((values) => [
      ...values.filter((value) => value.id !== saved.id),
      saved,
    ]);
    setSelectedId(saved.id);
    setSavedNotice({
      text: `${duplicateSource && !asNew ? "副本已保存" : "配置已保存"}：${saved.name}`,
      serverId: saved.id,
    });
    setEditor(null);
    setDuplicateSource(null);
    setError(null);
    if (connect) open(saved, true);
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
    setDeleting(server);
  }
  return (
    <div className="app-shell">
      <WorkspaceTitlebar
        tabs={workspaces.tabs}
        activeId={workspaces.activeId}
        moving={moves.moving}
        onSelect={navigate}
        onHome={() => navigate("")}
        onClose={(id) => {
          if (moves.moving.has(id)) return;
          navigationSerial.current++;
          if (pendingRef.current?.tabId === id) discardPending();
          else setWorkspaces((current) => closeWorkspace(current, id));
        }}
        onMove={(...args) => void moves.move(...args)}
        onReorder={(id, beforeId) =>
          setWorkspaces((current) => reorderWorkspace(current, id, beforeId))
        }
        onNew={desktop ? () => navigate("") : undefined}
        onError={(error) => setError(errorOf(error))}
        actions={
          <>
            <PortForwardButton
              iconOnly
              summary
              onError={(e) => setError(errorOf(e))}
            />
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
                  promptActive={
                    !editor &&
                    !deleting &&
                    !quitRequest &&
                    databaseResetDetails === null &&
                    (workspaces.activeId === tab.id ||
                      (!workspaces.activeId && pending?.tabId === tab.id))
                  }
                  initial={moves.snapshots.current.get(tab.id)}
                  onReady={() => moves.ready(tab.id)}
                  controllerRef={(value) => {
                    if (value) moves.handles.current.set(tab.id, value);
                    else moves.handles.current.delete(tab.id);
                  }}
                  onChange={updateTab}
                  onConfigurationChanged={refreshConfigurations}
                  onConnectionStatus={receiveStatus}
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
            <ServerHome
              servers={servers}
              tabs={workspaces.tabs}
              selectedId={selectedId}
              query={query}
              tag={tag}
              loading={loading}
              active={!workspaces.activeId}
              focusRequest={focusRequest}
              loadFailed={!!error && !servers.length}
              pending={pending}
              notice={savedNotice}
              onSelect={setSelectedId}
              onQuery={setQuery}
              onTag={setTag}
              onConnect={open}
              onEdit={editServer}
              onCopy={copyServer}
              onDelete={requestDelete}
              onNew={() => editServer(emptyServer())}
              onCancel={discardPending}
              onForward={(server) => {
                void forwardApi
                  .open(server.id)
                  .catch((e) => setError(errorOf(e)));
              }}
              onReveal={(id) => {
                setQuery("");
                setTag("");
                setSelectedId(id);
              }}
            />
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
          <FileTaskPanel embedded />
        </div>
      </footer>
      <DesktopDragPreview />
      {editor && (
        <ServerEditor
          initial={editor}
          duplicateSource={duplicateSource}
          connectDisabled={!!pending?.busy}
          servers={servers}
          tags={tags}
          onSave={save}
          onDelete={editor.id ? requestDelete : undefined}
          onClose={() => {
            setEditor(null);
            setDuplicateSource(null);
          }}
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
  if (params.has("monitor")) {
    title = "服务器监控";
    content = <MonitorWindowApp />;
  } else if (params.has("port-forwarding")) {
    title = "端口转发";
    content = <PortForwardApp />;
  } else if (params.has("terminal-history")) {
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
import { dockerApi } from "./dockerApi";
