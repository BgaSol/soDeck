import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Plus,
  Search,
  Server as ServerIcon,
  Pencil,
  TerminalSquare,
  Plug,
  X,
  LockKeyhole,
} from "lucide-react";
import { api, desktop } from "./api";
import { TerminalPane } from "./TerminalPane";
import { ServerEditor } from "./ServerEditor";
import { FilePanel } from "./FilePanel";
import { DockerPanel } from "./DockerPanel";
import { WorkspaceTabs } from "./WorkspaceTabs";
import { DetachedApp } from "./DetachedApp";
import { DownloadPanel } from "./DownloadPanel";
import { fileApi, isDownloadActive } from "./fileApi";
import { useWorkspaceMoves } from "./useWorkspaceMoves";
import { SessionWorkspace } from "./SessionWorkspace";
import { addressOf, emptyServer, errorOf, filterServers } from "./types";
import {
  closeWorkspace,
  emptyWorkspaces,
  openWorkspace,
  reorderWorkspace,
} from "./workspaces";
import type { AppError, Server } from "./types";
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
      <header className="container-topbar">
        <TerminalSquare size={18} />
        <strong>
          {info ? `${info.serverName} / ${info.containerName}` : "容器终端"}
        </strong>
        <span className="push-right muted">{state}</span>
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
      </header>
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
  const [databaseReset, setDatabaseReset] = useState(false);
  const [databaseNotice, setDatabaseNotice] = useState("");
  async function resetDatabase() {
    if (
      databaseReset ||
      !window.confirm(
        "将先备份当前旧数据库，再重建空数据库。服务器配置、标签和已确认的主机指纹将清空，旧数据保留在备份中。是否继续？",
      )
    )
      return;
    setDatabaseReset(true);
    try {
      const backup = await api.resetIncompatibleDatabase();
      const values = await api.listServers();
      setServers(values);
      setSelectedId(values[0]?.id || "");
      setError(null);
      setDatabaseNotice(`数据库已重新创建。旧数据备份：${backup}`);
    } catch (error) {
      setError(errorOf(error));
    } finally {
      setDatabaseReset(false);
    }
  }
  const [menu, setMenu] = useState<{
    server: Server;
    x: number;
    y: number;
  } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const moves = useWorkspaceMoves(setWorkspaces, setError);
  const selected = servers.find((server) => server.id === selectedId);
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
        const count = await invoke<number>("session_count").catch(() => 0);
        const logs = await invoke<number>("log_session_count").catch(() => 0);
        const dirty = await invoke<number>("unsaved_editor_count").catch(
          () => 0,
        );
        const downloads = await fileApi.downloads().catch(() => []);
        const activeDownloads = downloads.filter(isDownloadActive).length;
        const details = [
          count ? `仍有 ${count} 个活动终端` : "",
          logs ? `${logs} 个日志窗口正在接收` : "",
          dirty ? `${dirty} 个文件有未保存修改` : "",
          activeDownloads ? `${activeDownloads} 个下载尚未完成` : "",
        ]
          .filter(Boolean)
          .join("，");
        if (
          !details ||
          window.confirm(
            details +
              "。退出应用将关闭连接、取消下载并放弃未保存修改，是否退出？",
          )
        )
          void invoke("quit_app", { discardUnsaved: dirty > 0 }).catch((e) =>
            setError(errorOf(e)),
          );
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
  useEffect(() => {
    if (!menu) return;
    menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const outside = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenu(null);
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [menu]);
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
  const updateSavedServer = useCallback((saved: Server) => {
    setServers((current) =>
      current.map((server) => (server.id === saved.id ? saved : server)),
    );
  }, []);
  function open(server: Server, forceNew = false) {
    setMenu(null);
    if (!desktop) return;
    if (!server.username) {
      setError({ kind: "validation", message: "请先填写 SSH 用户名。" });
      setEditor(structuredClone(server));
      return;
    }
    setError(null);
    setWorkspaces((current) => openWorkspace(current, server, forceNew));
  }
  async function save(server: Server) {
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
    await api.deleteServer(server.id);
    setServers((values) => values.filter((value) => value.id !== server.id));
    if (selectedId === server.id)
      setSelectedId(servers.find((value) => value.id !== server.id)?.id || "");
    if (
      !servers.some(
        (value) => value.id !== server.id && value.tags.includes(tag),
      )
    )
      setTag("");
    setEditor(null);
  }
  return (
    <div className="app-shell">
      <header className="app-header">
        <span className="app-mark" aria-hidden="true">
          <TerminalSquare size={20} />
        </span>
        <div className="app-brand">
          <strong>SSH Workspace</strong>
          <span>服务器工作空间</span>
        </div>
        <span className="header-divider" />
        <span className="header-summary">
          {workspaces.tabs.length
            ? `${workspaces.tabs.length} 个会话 · ${workspaces.tabs.filter((tab) => tab.state === "已连接").length} 已连接`
            : "随时连接，专注工作"}
        </span>
        <span className="local-label">
          <LockKeyhole size={12} />
          本地工作空间
        </span>
      </header>
      {!desktop && (
        <div className="browser-notice">
          请使用桌面客户端运行，服务器数据保存在本地 SQLite 文件中。
          <code>npm run desktop</code>
        </div>
      )}
      <div className="workspace">
        <div className="workspaces">
          <WorkspaceTabs
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
              setWorkspaces((current) =>
                reorderWorkspace(current, id, beforeId),
              )
            }
            onNew={selected && desktop ? () => open(selected, true) : undefined}
          />
          {error && (
            <div className="connection-error" role="alert">
              <span style={{ whiteSpace: "pre-wrap" }}>{error.message}</span>
              {error.kind === "databaseSchema" && (
                <button
                  disabled={databaseReset}
                  onClick={() => void resetDatabase()}
                >
                  {databaseReset ? "正在备份并重建…" : "备份旧库并重建"}
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
                  latestServer={servers.find(
                    (server) => server.id === tab.server.id,
                  )}
                  initial={moves.snapshots.current.get(tab.id)}
                  onReady={() => moves.ready(tab.id)}
                  controllerRef={(value) => {
                    if (value) moves.handles.current.set(tab.id, value);
                    else moves.handles.current.delete(tab.id);
                  }}
                  onChange={updateTab}
                  onServerSaved={updateSavedServer}
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
              <div className="session-page empty-session">
                <aside
                  className="resource-panel"
                  style={{ gridTemplateRows: "1fr 10px 1fr" }}
                >
                  <FilePanel sessionId={null} serverName="" />
                  <div className="horizontal-resizer" />
                  <DockerPanel sessionId={null} serverName="" active={false} />
                </aside>
                <div className="vertical-resizer" />
                <main className="terminal-panel">
                  <div className="session-welcome">
                    <span className="welcome-symbol" aria-hidden="true">
                      <TerminalSquare size={38} />
                    </span>
                    <span className="welcome-caption">
                      你的远程工作，从这里开始
                    </span>
                    <h2>打开一个 SSH 连接</h2>
                    <p>双击右侧服务器，或选择服务器后点击“连接”。</p>
                    <p>每个会话都有独立的终端、文件目录和 Docker 面板。</p>
                    {desktop && (
                      <button
                        className="primary welcome-action"
                        title={selected?.name}
                        onClick={() =>
                          selected ? open(selected) : setEditor(emptyServer())
                        }
                      >
                        {selected ? <Plug size={15} /> : <Plus size={15} />}
                        {selected ? "打开所选服务器" : "添加第一台服务器"}
                      </button>
                    )}
                    <div className="welcome-features" aria-hidden="true">
                      <span>SSH 终端</span>
                      <span>SFTP 文件</span>
                      <span>Docker 服务</span>
                    </div>
                  </div>
                </main>
              </div>
            )}
          </div>
        </div>
        <aside className="server-panel" aria-label="服务器列表">
          <div className="server-tools">
            <div className="panel-title">
              <h2>
                服务器<span className="inventory-count">{servers.length}</span>
              </h2>
              <button
                className="primary"
                disabled={!desktop}
                onClick={() => setEditor(emptyServer())}
              >
                <Plus size={15} />
                新增
              </button>
            </div>
            <label className="search-box">
              <Search size={16} />
              <input
                aria-label="搜索服务器"
                placeholder="搜索名称或地址"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <select
              aria-label="按标签筛选"
              value={tag}
              onChange={(event) => setTag(event.target.value)}
            >
              <option value="">全部标签</option>
              {tags.map((value) => (
                <option key={value}>{value}</option>
              ))}
            </select>
          </div>
          <div className="server-list">
            {visible.map((server) => (
              <button
                className={`server-row${selectedId === server.id ? " selected" : ""}`}
                aria-pressed={selectedId === server.id}
                key={server.id}
                onClick={() => setSelectedId(server.id)}
                onDoubleClick={() => open(server)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setSelectedId(server.id);
                  setMenu({
                    server,
                    x: Math.min(event.clientX, window.innerWidth - 190),
                    y: Math.min(event.clientY, window.innerHeight - 145),
                  });
                }}
              >
                <span className="server-symbol" aria-hidden="true">
                  <ServerIcon size={17} />
                </span>
                <span className="server-row-body">
                  <span className="server-name">{server.name}</span>
                  <span className="server-address">{addressOf(server)}</span>
                  {server.tags.length > 0 && (
                    <span className="chips">
                      {server.tags.map((value) => (
                        <span className="chip" key={value}>
                          {value}
                        </span>
                      ))}
                    </span>
                  )}
                </span>
              </button>
            ))}
            {!visible.length && (
              <div className="panel-empty">
                <ServerIcon size={26} aria-hidden="true" />
                <strong>
                  {loading
                    ? "正在读取服务器…"
                    : servers.length
                      ? "没有匹配的服务器"
                      : "还没有服务器"}
                </strong>
                {!loading && !servers.length && (
                  <p>添加服务器，开始你的第一个会话。</p>
                )}
                {(query || tag) && (
                  <button
                    onClick={() => {
                      setQuery("");
                      setTag("");
                    }}
                  >
                    清除筛选
                  </button>
                )}
              </div>
            )}
          </div>
          <footer className="server-footer">
            <span>
              {visible.length} / {servers.length} 台服务器
            </span>
            <div className="server-actions">
              <button
                className="primary"
                disabled={!selected || !desktop}
                onClick={() => open(selected!)}
              >
                <Plug size={13} />
                连接
              </button>
              <button
                className="quiet"
                disabled={!selected}
                onClick={() => setEditor(structuredClone(selected!))}
              >
                <Pencil size={13} />
                编辑
              </button>
            </div>
          </footer>
        </aside>
      </div>
      <DownloadPanel />
      {menu && (
        <div
          className="server-context-menu"
          role="menu"
          aria-label={`${menu.server.name} 操作`}
          ref={menuRef}
          style={{ left: menu.x, top: menu.y }}
        >
          <button role="menuitem" onClick={() => open(menu.server)}>
            <Plug size={14} />
            打开连接
          </button>
          <button role="menuitem" onClick={() => open(menu.server, true)}>
            <Plus size={14} />
            新建连接
          </button>
          <button
            role="menuitem"
            onClick={() => {
              setEditor(structuredClone(menu.server));
              setMenu(null);
            }}
          >
            <Pencil size={14} />
            编辑
          </button>
        </div>
      )}
      {editor && (
        <ServerEditor
          initial={editor}
          tags={tags}
          onSave={save}
          onDelete={editor.id ? remove : undefined}
          onClose={() => setEditor(null)}
        />
      )}
    </div>
  );
}
export function Root() {
  const params = new URLSearchParams(location.search);
  if (params.has("logs"))
    return (
      <Suspense
        fallback={<div className="remote-editor-app">正在打开容器日志…</div>}
      >
        <ContainerLogsApp />
      </Suspense>
    );
  if (params.has("editor"))
    return (
      <Suspense
        fallback={<div className="remote-editor-app">正在打开远程编辑器…</div>}
      >
        <FileEditorApp />
      </Suspense>
    );
  if (params.has("workspace")) return <DetachedApp />;
  return new URLSearchParams(location.search).has("container") ? (
    <ContainerApp />
  ) : (
    <App />
  );
}
