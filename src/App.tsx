import { workspaceApi } from "./workspaceApi";
import { useWorkspaceToolActions } from "./useWorkspaceToolActions";
import { projectWorkspace } from "./workspaces";
import { useServerWorkspaces } from "./useServerWorkspaces";
import { useWorkspaceClose } from "./useWorkspaceClose";
import { recordFrontendError } from "./diagnostics";
import { DiagnosticsSection } from "./DiagnosticsSection";
import type { TerminalSourceLabel } from "./terminalClipboard/types";
import {
  lazy,
  Component,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
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
import {
  createServerName,
  duplicatePasswordPolicy,
  configurationFields,
} from "./serverConfiguration";
import { emptyServer, errorOf, filterServers } from "./types";
import { emptyWorkspaces, reorderWorkspace } from "./workspaces";
import type {
  AppError,
  Server,
  SaveServerRequest,
  ServerSaveIntent,
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
  const [info, setInfo] = useState<TerminalSourceLabel | null>(null),
    [error, setError] = useState<AppError | null>(null),
    [state, setState] = useState("连接中"),
    [attempt, setAttempt] = useState(0);
  useEffect(() => {
    void invoke<TerminalSourceLabel>("container_info")
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
          source={info ?? undefined}
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
  const [editorSelectName, setEditorSelectName] = useState(false);
  const [focusRequest, setFocusRequest] = useState(0);
  const [savedNotice, setSavedNotice] = useState<{
    text: string;
    serverId: string;
  } | null>(null);
  const lastVisited = useRef(new Map<string, number>());
  const navigationSerial = useRef(0);
  useEffect(() => {
    if (!workspaces.activeId) return;
    lastVisited.current.set(workspaces.activeId, performance.now());
  }, [workspaces.activeId]);
  function navigate(id: string) {
    navigationSerial.current++;
    setWorkspaces((current) => ({ ...current, activeId: id }));
    if (!id) setFocusRequest((value) => value + 1);
  }
  const moves = useWorkspaceMoves(setWorkspaces, setError);
  const toolActions = useWorkspaceToolActions(
    workspaces.tabs,
    moves.moving,
    moves.handles,
    (e) => setError(errorOf(e)),
  );
  const allWorkspaces = useServerWorkspaces(setWorkspaces, setError);
  const workspaceClose = useWorkspaceClose(setError);
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
  const configurationBusy = (id: string) =>
    allWorkspaces.some(
      (w) =>
        w.server.id === id &&
        (w.tools.state === "verifying" ||
          w.terminals.some((t) =>
            ["queued", "connecting", "authenticating"].includes(t.state),
          )),
    );
  async function open(server: Server, newTerminal = false) {
    if (!desktop) return;
    if (!server.username) {
      setError({ kind: "validation", message: "请先填写 SSH 用户名。" });
      setEditor(structuredClone(server));
      return;
    }
    const serial = ++navigationSerial.current;
    try {
      const view = await workspaceApi.open(server.id, newTerminal);
      if (
        view.owner.label === getCurrentWindow().label &&
        navigationSerial.current === serial
      )
        setWorkspaces((current) => ({
          tabs: current.tabs.some((t) => t.id === view.id)
            ? current.tabs
            : [...current.tabs, projectWorkspace(view)],
          activeId: view.id,
        }));
      setError(null);
    } catch (e) {
      setError(errorOf(e));
    }
  }
  const createConnection = useCallback(
    async (serverId: string, _afterId?: string, requestId?: string) => {
      try {
        const view = await workspaceApi.open(serverId, true, requestId);
        if (view.owner.label === getCurrentWindow().label)
          setWorkspaces((current) => ({
            tabs: current.tabs.some((t) => t.id === view.id)
              ? current.tabs.map((t) =>
                  t.id === view.id ? projectWorkspace(view) : t,
                )
              : [...current.tabs, projectWorkspace(view)],
            activeId: view.id,
          }));
      } catch (e) {
        setError(errorOf(e));
      }
    },
    [],
  );
  function editServer(server: Server) {
    setEditorSelectName(false);
    setEditor(structuredClone(server));
  }
  function copyServer(server: Server) {
    setEditorSelectName(true);
    setEditor(structuredClone(server));
  }
  async function save(
    server: SaveServerRequest,
    intent: ServerSaveIntent,
    autoName: boolean,
  ) {
    if (!editor) return;
    if (intent === "update" && (!editor.id || server.id !== editor.id))
      throw {
        kind: "validation",
        message: "无法确定要更新的服务器，请重新打开配置。",
      };
    if (intent === "update" && configurationBusy(editor.id))
      throw { kind: "busy", message: "请完成或取消当前连接后再保存修改。" };
    if (intent === "create" && autoName) {
      const latest = await api.listServers();
      const expectedName = createServerName(editor, editor, latest);
      if (server.name !== expectedName) {
        setServers(latest);
        throw {
          kind: "namePreviewChanged",
          message: "可用的新建名称已更新，请确认名称预览后再次新建。",
        };
      }
    }
    const saved =
      intent === "create" && server.id
        ? await api.duplicateServer({
            sourceId: editor.id,
            draft: configurationFields(server),
            passwordPolicy: duplicatePasswordPolicy(
              editor,
              server,
              server.passwordChange,
            ),
          })
        : await api.saveServer({
            ...server,
            id: intent === "update" ? editor.id : "",
          });
    setServers((values) => [
      ...values.filter((value) => value.id !== saved.id),
      saved,
    ]);
    setSelectedId(saved.id);
    setSavedNotice({
      text: `${intent === "create" ? "服务器已新建" : "修改已保存"}：${saved.name}`,
      serverId: saved.id,
    });
    setEditor(null);
    setEditorSelectName(false);
    setError(null);
  }
  async function remove(server: Server) {
    if (allWorkspaces.some((tab) => tab.server.id === server.id))
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
        toolStates={toolActions.toolStates}
        onToolAction={toolActions.onToolAction}
        onSelect={navigate}
        onHome={() => navigate("")}
        onClose={(id) => {
          if (moves.moving.has(id)) return;
          navigationSerial.current++;
          const workspace = workspaces.tabs.find((t) => t.id === id);
          if (workspace) void workspaceClose.close(workspace);
        }}
        onMove={(...args) => void moves.move(...args)}
        onReorder={(id, beforeId) =>
          setWorkspaces((current) => reorderWorkspace(current, id, beforeId))
        }
        onNew={desktop ? () => navigate("") : undefined}
        onNewConnection={
          desktop
            ? (id) => {
                const tab = workspaces.tabs.find((item) => item.id === id);
                if (tab && !moves.moving.has(id))
                  void createConnection(tab.server.id, id);
              }
            : undefined
        }
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
                    workspaces.activeId === tab.id
                  }
                  initial={moves.snapshots.current.get(tab.id)}
                  onReady={() => moves.ready(tab.id)}
                  controllerRef={(value) => {
                    if (value) moves.handles.current.set(tab.id, value);
                    else moves.handles.current.delete(tab.id);
                  }}
                  onChange={updateTab}
                  onToolState={toolActions.onToolState}
                  onConfigurationChanged={refreshConfigurations}
                />
                {moves.moving.has(tab.id) && (
                  <div className="session-moving">
                    <span>正在移动服务器…</span>
                    <button onClick={() => moves.cancel(tab.id)}>
                      取消移动
                    </button>
                  </div>
                )}
              </div>
            ))}
            <ServerHome
              servers={servers}
              tabs={allWorkspaces}
              selectedId={selectedId}
              query={query}
              tag={tag}
              loading={loading}
              active={!workspaces.activeId}
              focusRequest={focusRequest}
              loadFailed={!!error && !servers.length}
              pending={null}
              notice={savedNotice}
              onSelect={setSelectedId}
              onQuery={setQuery}
              onTag={setTag}
              onConnect={open}
              onEdit={editServer}
              onCopy={copyServer}
              onDelete={requestDelete}
              onNew={() => editServer(emptyServer())}
              onCancel={() => {}}
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
          {allWorkspaces.length} 个服务器 ·{" "}
          {allWorkspaces.reduce((n, w) => n + w.terminals.length, 0)} 个终端 ·{" "}
          {allWorkspaces.reduce(
            (n, w) =>
              n + w.terminals.filter((t) => t.state === "connected").length,
            0,
          )}{" "}
          已连接
        </span>
        <div className="workspace-summary-end">
          <span>本地工作空间</span>
          <FileTaskPanel embedded />
        </div>
      </footer>
      {workspaceClose.dialog}
      <DesktopDragPreview />
      {editor && (
        <ServerEditor
          initial={editor}
          selectName={editorSelectName}
          updateBlocked={configurationBusy(editor.id)}
          servers={servers}
          tags={tags}
          onSave={save}
          onClose={() => {
            setEditor(null);
            setEditorSelectName(false);
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
          blocked={allWorkspaces.some((tab) => tab.server.id === deleting.id)}
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
  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    recordFrontendError(
      "frontend.render.error",
      error,
      info.componentStack ?? undefined,
    );
  }
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    if (this.state.failed)
      return (
        <div className="window-loading">
          <WindowTitlebar title="SoDeck" />
          <p role="alert">窗口内容加载失败，请关闭窗口后重试。</p>
          <DiagnosticsSection />
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
