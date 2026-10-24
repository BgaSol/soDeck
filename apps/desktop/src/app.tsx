import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { Activity, ArrowLeftToLine, ArrowUpRight, ChevronDown, CircleHelp, Columns2, Command, Database, Ellipsis, FileTerminal as FolderTerminal, LayoutPanelTop, List, Logs, Package, PanelBottom, PanelLeftClose, Plus, Power, Rows2, Server as ServerIcon, Settings2, TerminalSquare, X } from "lucide-react";
import { core, context, CoreError, errorMessage, onCoreNotification } from "@sodeck/sdk";
import { NativeSurface } from "./surface";
import { ServerDialog } from "./server-dialog";
import { AuthDialog } from "./auth-dialog";
import { Dialog } from "./dialog";
import { ManagementDialog } from "./management-dialog";
import { emptyWorkbench } from "./types";
import type { PendingOpen, Server, View, Workbench } from "./types";

type Page = "plugins" | "resources" | "calls";
type Drag = { view: View; x: number; y: number; offsetX: number; offsetY: number };
export function App() {
  const [servers, setServers] = useState<Server[]>([]);
  const [workbench, setWorkbench] = useState<Workbench>(emptyWorkbench);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [serverEditor, setServerEditor] = useState<Server | "new" | null>(null);
  const [auth, setAuth] = useState<{ server: Server; fault: CoreError; operationId?: string } | null>(null);
  const [page, setPage] = useState<Page | null>(null);
  const [deleteServer, setDeleteServer] = useState<Server | null>(null);
  const [connecting, setConnecting] = useState<PendingOpen | null>(null);
  const [cancelling, setCancelling] = useState<Set<string>>(new Set());
  const openingRef = useRef<PendingOpen | null>(null);
  const cancellingRef = useRef(new Set<string>());
  const canceledOpens = useRef(new Set<string>());
  const newTerminalButton = useRef<HTMLButtonElement>(null);
  const [serverMenu, setServerMenu] = useState<string | null>(null);
  const [viewMenu, setViewMenu] = useState<string | null>(null);
  const [appMenu, setAppMenu] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [activeTabs, setActiveTabs] = useState<Record<number, string>>({});
  const [splitRatio, setSplitRatio] = useState(.5);
  const [sideWidth, setSideWidth] = useState(226);
  const [bottomHeight, setBottomHeight] = useState(245);
  const refreshInFlight = useRef(false);
  const dragCandidate = useRef<{ view: View; x: number; y: number; offsetX: number; offsetY: number } | null>(null);
  const dragged = useRef<Drag | null>(null);
  const suppressTabClick = useRef<string | null>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const detachedId = context().detachedViewId;
  const modal = Boolean(serverEditor || auth || page || deleteServer);
  const blocked = modal || Boolean(drag || viewMenu || serverMenu || appMenu);
  useEffect(() => { void core.call("workbench.modal", { open: blocked }).catch(() => {}); }, [blocked]);
  const refresh = useCallback(async () => {
    if (refreshInFlight.current) return; refreshInFlight.current = true;
    try {
      const [serverResult, layout] = await Promise.all([core.call<{ servers: Server[] }>("servers.list"), core.call<Workbench>("workbench.snapshot")]);
      setServers(serverResult.servers); setWorkbench({ ...emptyWorkbench, ...layout });
    } catch (e) { setError(errorMessage(e)); } finally { refreshInFlight.current = false; }
  }, []);
  useEffect(() => {
    void refresh();
    const off = onCoreNotification(notification => {
      if (["workbench.changed", "servers.changed", "sessions.changed"].includes(notification.type)) void refresh();
      if (notification.type === "workbench.warning") {
        const warning = notification.payload as { message?: string };
        if (warning.message) setError(warning.message);
      }
      if (notification.type === "workbench.openCanceled") {
        const { operationId } = notification.payload as { operationId: string };
        canceledOpens.current.add(operationId);
        if (canceledOpens.current.size > 1000) canceledOpens.current.delete(canceledOpens.current.values().next().value!);
        setAuth(current => current?.operationId === operationId ? null : current);
      }
      if (notification.type === "auth.required") {
        const payload = notification.payload as { server: Server; operationId?: string; error: { code: string; message: string; details?: unknown } };
        if (!payload.operationId || !canceledOpens.current.has(payload.operationId)) setAuth({ server: payload.server, fault: new CoreError(payload.error), operationId: payload.operationId });
      }
    });
    const interval = setInterval(() => void refresh(), 1600);
    return () => { off(); clearInterval(interval); };
  }, [refresh]);
  const run = useCallback(async (method: string, params: unknown = {}) => {
    try { setError(""); const result = await core.call(method, params); await refresh(); return result; }
    catch (e) { setError(errorMessage(e)); return undefined; }
  }, [refresh]);
  const activeServer = servers.find(server => server.id === workbench.activeWorkspaceId) ?? servers.find(server => workbench.views.some(view => view.workspaceId === workbench.activeWorkspaceId && view.serverId === server.id)) ?? servers.find(server => server.id === selectedServerId);
  const activateServer = async (server: Server) => {
    setSelectedServerId(server.id); setServerMenu(null); setActiveTabs({}); await run("workbench.activateWorkspace", { serverId: server.id });
  };
  const openTerminal = async (server: Server) => {
    if (openingRef.current) return;
    const operation = { serverId: server.id, operationId: crypto.randomUUID() };
    openingRef.current = operation; setConnecting(operation); setError("");
    try { await core.call("workbench.openTerminal", operation, 130_000); await refresh(); }
    catch (e) {
      if (e instanceof CoreError && ["HOST_KEY_REQUIRED", "HOST_KEY_CHANGED", "AUTH_REQUIRED", "AUTH_FAILED"].includes(e.code)) { if (!canceledOpens.current.has(operation.operationId)) setAuth({ server, fault: e, operationId: operation.operationId }); }
      else if (e instanceof CoreError && e.code === "CANCELED") { /* Cancellation is an expected end state. */ }
      else setError(errorMessage(e));
    } finally { openingRef.current = null; setConnecting(null); await refresh(); }
  };
  const cancelOpen = async (operationId: string) => {
    if (cancellingRef.current.has(operationId)) return;
    const previous = document.activeElement as HTMLElement | null;
    canceledOpens.current.add(operationId);
    if (canceledOpens.current.size > 1000) canceledOpens.current.delete(canceledOpens.current.values().next().value!);
    cancellingRef.current.add(operationId); setCancelling(new Set(cancellingRef.current)); setError("");
    try { await core.call("workbench.cancelOpen", { operationId }); await refresh(); }
    catch (e) { setError(errorMessage(e)); }
    finally {
      cancellingRef.current.delete(operationId); setCancelling(new Set(cancellingRef.current));
      requestAnimationFrame(() => { if (previous && !previous.isConnected && document.activeElement === document.body) newTerminalButton.current?.focus(); });
    }
  };
  const pendingOpens = connecting && !workbench.pendingOpens.some(open => open.operationId === connecting.operationId) ? [...workbench.pendingOpens, connecting] : workbench.pendingOpens;
  const moveView = async (view: View, placement: string, group = 0, screenPosition?: { x: number; y: number }) => {
    setViewMenu(null); await run("workbench.move", { viewId: view.viewId, placement, group, ...(screenPosition ? { screenPosition } : {}) });
    if (placement === "center") setActiveTabs(current => ({ ...current, [group]: view.viewId }));
  };
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { setViewMenu(null); setServerMenu(null); setAppMenu(false); dragCandidate.current = null; dragged.current = null; setDrag(null); }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "t" && !modal && activeServer) { event.preventDefault(); void openTerminal(activeServer); }
    };
    window.addEventListener("keydown", keydown); return () => window.removeEventListener("keydown", keydown);
  });
  const visibleViews = workbench.views.filter(view => detachedId ? view.viewId === detachedId : view.workspaceId === workbench.activeWorkspaceId && view.placement !== "detached");
  const detachedView = detachedId ? workbench.views.find(view => view.viewId === detachedId) : undefined;
  const sessions = visibleViews.find(view => view.viewType === "terminal.sessions");
  const terminalViews = visibleViews.filter(view => view.viewType !== "terminal.sessions");
  const centers = terminalViews.filter(view => view.placement === "center");
  const bottomViews = terminalViews.filter(view => view.placement === "bottom");
  const bottomActive = bottomViews.find(view => view.active) ?? bottomViews.find(view => view.viewId === activeTabs[2]) ?? bottomViews[0];
  const count = workbench.views.filter(view => view.viewType === "terminal.session" && ["active", "running", "connected"].includes(view.status)).length;

  const activateTab = (viewId: string, group: number) => {
    setActiveTabs(current => ({ ...current, [group]: viewId }));
    void run("workbench.activate", { viewId });
  };
  const tabKeyDown = (event: ReactKeyboardEvent, view: View, group: number) => {
    if (event.target !== event.currentTarget) return;
    if (event.key === "Enter" || event.key === " ") { event.preventDefault(); activateTab(view.viewId, group); return; }
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const peers = group === 2 ? bottomViews : centers.filter(item => workbench.split === "none" || item.group === group);
    const index = peers.findIndex(item => item.viewId === view.viewId);
    const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? peers.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + peers.length) % peers.length;
    const next = peers[nextIndex];
    if (!next) return;
    event.preventDefault();
    document.getElementById(`tab-${next.viewId}`)?.focus();
    activateTab(next.viewId, group);
  };

  const pointerDown = (event: ReactPointerEvent, view: View) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    suppressTabClick.current = null;
    const rect = event.currentTarget.getBoundingClientRect();
    dragCandidate.current = { view, x: event.clientX, y: event.clientY, offsetX: event.clientX - rect.x, offsetY: event.clientY - rect.y };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const pointerMove = (event: ReactPointerEvent) => {
    const candidate = dragCandidate.current;
    if (!candidate) return;
    if (!dragged.current && Math.hypot(event.clientX - candidate.x, event.clientY - candidate.y) < 8) return;
    suppressTabClick.current = candidate.view.viewId;
    const next = { ...candidate, x: event.clientX, y: event.clientY }; dragged.current = next; setDrag(next);
  };
  const pointerUp = (event: ReactPointerEvent) => {
    const current = dragged.current;
    dragCandidate.current = null; dragged.current = null; setDrag(null);
    if (!current) return;
    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-drop-placement]");
    if (target) void moveView(current.view, target.dataset.dropPlacement!, Number(target.dataset.dropGroup ?? 0));
    else if (event.clientX < 0 || event.clientY < 0 || event.clientX > innerWidth || event.clientY > innerHeight) void moveView(current.view, "detached", 0, { x: event.screenX, y: event.screenY });
  };
  const resize = (event: ReactPointerEvent, type: "side" | "split" | "bottom") => {
    const startX = event.clientX, startY = event.clientY, initialSide = sideWidth, initialBottom = bottomHeight;
    const initialRatio = splitRatio;
    const target = event.currentTarget; target.setPointerCapture(event.pointerId);
    const move = (next: PointerEvent) => {
      if (type === "side") setSideWidth(Math.max(200, Math.min(400, initialSide + (next.clientX - startX) * (workbench.sessionsSide === "right" ? -1 : 1))));
      if (type === "bottom") setBottomHeight(Math.max(160, Math.min(innerHeight * .65, initialBottom - next.clientY + startY)));
      if (type === "split") {
        const rect = workspaceRef.current?.getBoundingClientRect(); if (!rect) return;
        setSplitRatio(Math.max(.25, Math.min(.75, initialRatio + (workbench.split === "horizontal" ? (next.clientX - startX) / rect.width : (next.clientY - startY) / rect.height))));
      }
    };
    const up = () => { target.removeEventListener("pointermove", move as EventListener); target.removeEventListener("pointerup", up); target.removeEventListener("pointercancel", up); };
    target.addEventListener("pointermove", move as EventListener); target.addEventListener("pointerup", up, { once: true }); target.addEventListener("pointercancel", up, { once: true });
  };
  const tab = (view: View, active: boolean, group: number) => <div key={view.viewId} id={`tab-${view.viewId}`} className={`view-tab ${active ? "active" : ""}`} role="tab" aria-selected={active} tabIndex={active ? 0 : -1} title={view.title} onPointerDown={event => pointerDown(event, view)} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={() => { dragCandidate.current = null; dragged.current = null; setDrag(null); }} onClick={() => { if (suppressTabClick.current === view.viewId) { suppressTabClick.current = null; return; } activateTab(view.viewId, group); }} onKeyDown={event => tabKeyDown(event, view, group)}>
    <TerminalSquare size={14} /><span>{view.title}</span><button className="icon-button tab-menu" aria-label={`${view.title} 的视图操作`} onClick={event => { event.stopPropagation(); setViewMenu(viewMenu === view.viewId ? null : view.viewId); }}><Ellipsis size={14} /></button><button className="icon-button tab-close" title="关闭终端并结束会话" aria-label={`关闭 ${view.title}`} onClick={event => { event.stopPropagation(); void run("workbench.close", { viewId: view.viewId }); }}><X size={13} /></button>
    {viewMenu === view.viewId && <div className="popover tab-popover" role="menu" onClick={event => event.stopPropagation()}><button onClick={() => void moveView(view, "center", 0)}><LayoutPanelTop size={14} />移到中央</button><button onClick={() => void moveView(view, "center", 1)}><Columns2 size={14} />移到另一区域</button><button onClick={() => void moveView(view, "bottom")}><PanelBottom size={14} />移到底部</button><button onClick={() => void moveView(view, "detached")}><ArrowUpRight size={14} />独立窗口</button></div>}
  </div>;
  const renderGroup = (group: number) => {
    const views = centers.filter(view => workbench.split === "none" || view.group === group);
    const active = views.find(view => view.active || workbench.activeViewIds?.[group] === view.viewId) ?? views.find(view => view.viewId === activeTabs[group]) ?? views[0];
    return <section className="editor-group" data-drop-placement="center" data-drop-group={group} style={workbench.split !== "none" ? { flex: group === 0 ? splitRatio : 1 - splitRatio } : undefined}>
      <div className="tab-bar" role="tablist" aria-label={`终端区域 ${group + 1}`}>{views.map(view => tab(view, view.viewId === active?.viewId, group))}{activeServer && <button className="icon-button new-tab" aria-label="新建终端" onClick={() => void openTerminal(activeServer)} disabled={Boolean(connecting)}><Plus size={15} /></button>}</div>
      <div className="editor-content">{active ? <NativeSurface view={active} visible={!blocked} /> : <div className="group-empty"><TerminalSquare size={24} strokeWidth={1.2} /><p>将终端移到这里</p></div>}</div>
    </section>;
  };
  const sessionPanel = sessions && workbench.sessionsVisible && <aside className="sessions-panel" style={{ width: sideWidth }}><div className="panel-heading"><span><List size={13} />终端会话</span><div><button className="icon-button" title={workbench.sessionsSide === "left" ? "移到右侧" : "移到左侧"} aria-label="切换会话列表位置" onClick={() => void run("workbench.configure", { sessionsSide: workbench.sessionsSide === "left" ? "right" : "left" })}><ArrowLeftToLine size={13} style={{ transform: workbench.sessionsSide === "left" ? "rotate(180deg)" : undefined }} /></button><button className="icon-button" aria-label="收起会话列表" onClick={() => void run("workbench.configure", { sessionsVisible: false })}><PanelLeftClose size={13} /></button></div></div><NativeSurface view={sessions} visible={!blocked} /><div className={`resize-handle side ${workbench.sessionsSide}`} role="separator" tabIndex={0} aria-label="调整会话列表宽度" aria-orientation="vertical" onPointerDown={event => resize(event, "side")} onKeyDown={event => { if (event.key === "ArrowLeft") setSideWidth(Math.max(200, sideWidth - 16)); if (event.key === "ArrowRight") setSideWidth(Math.min(400, sideWidth + 16)); }} /></aside>;
  return <div className="app-shell">
    <header className="topbar"><div className="brand"><div className="brand-mark"><Command size={17} strokeWidth={1.7} /></div><span>SoDeck</span><span className="version">v2</span></div><div className="breadcrumb">{detachedId ? "独立终端" : "工作台"}{activeServer && <><span>/</span><strong>{activeServer.name}</strong></>}</div><div className="top-actions">{detachedId && <button onClick={() => { const view = workbench.views.find(item => item.viewId === detachedId); if (view) void moveView(view, "center"); }}><ArrowLeftToLine size={14} />返回主窗口</button>}<button className="icon-button" title="运行资源" aria-label="运行资源" onClick={() => setPage("resources")}><Activity size={17} /></button><div className="menu-parent"><button className="icon-button" aria-label="应用菜单" aria-expanded={appMenu} onClick={() => setAppMenu(!appMenu)}><ChevronDown size={16} /></button>{appMenu && <div className="popover app-popover"><button onClick={() => { setAppMenu(false); void run("app.hide"); }}>收起到托盘</button><button onClick={() => { setAppMenu(false); void run("app.exit"); }}><Power size={14} />退出 SoDeck</button></div>}</div></div></header>
    <div className="app-body">{!detachedId && <><nav className="activity-rail" aria-label="主要工具"><button className="rail-button active" aria-label="服务器" title="服务器"><ServerIcon size={20} /></button><button className={`rail-button ${workbench.sessionsVisible ? "selected" : ""}`} title="终端会话" aria-label="显示或收起终端会话" onClick={() => void run("workbench.configure", { sessionsVisible: !workbench.sessionsVisible })}><FolderTerminal size={20} /></button><div className="rail-spacer" /><button className="rail-button" title="插件" aria-label="插件" onClick={() => setPage("plugins")}><Package size={20} /></button><button className="rail-button" title="调用记录" aria-label="调用记录" onClick={() => setPage("calls")}><Logs size={20} /></button></nav>
      <aside className="servers-panel"><div className="server-list-heading"><span>服务器</span><button className="icon-button" title="添加服务器" aria-label="添加服务器" onClick={() => setServerEditor("new")}><Plus size={16} /></button></div><div className="server-list">{servers.map(server => <div key={server.id} className={`server-item ${activeServer?.id === server.id ? "active" : ""}`}><button className="server-select" onClick={() => void activateServer(server)}><span className="server-avatar"><ServerIcon size={17} /></span><span className="server-label"><strong>{server.name}</strong><small>{server.host}</small></span></button><button className="icon-button server-more" aria-label={`${server.name} 的操作`} onClick={() => setServerMenu(serverMenu === server.id ? null : server.id)}><Ellipsis size={15} /></button>{serverMenu === server.id && <div className="popover server-popover"><button onClick={() => { setServerEditor(server); setServerMenu(null); }}><Settings2 size={14} />编辑服务器</button><button onClick={() => { setDeleteServer(server); setServerMenu(null); }}>删除服务器</button></div>}</div>)}</div><button className="add-server" onClick={() => setServerEditor("new")}><Plus size={15} />添加服务器</button><div className="sidebar-note"><span className="local-indicator" />本地工作区<p>连接与凭据由核心统一管理</p></div></aside></>}
      <main className="main-area">
        {error && <div className="app-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="收起错误提示" onClick={() => setError("")}><X size={14} /></button></div>}
        {pendingOpens.length > 0 && <div className="pending-connections" aria-label="正在建立的连接">{pendingOpens.map(open => {
          const server = servers.find(server => server.id === open.serverId);
          const busy = cancelling.has(open.operationId);
          return <div className="connection-progress" key={open.operationId}><span role="status">{busy || open.cancelling ? "正在取消连接" : "正在连接"}<strong>{server?.name ?? "服务器"}</strong></span><button disabled={busy} aria-label={`取消连接到 ${server?.name ?? "服务器"}`} onClick={() => void cancelOpen(open.operationId)}>{busy ? "正在取消…" : "取消连接"}</button></div>;
        })}</div>}
        {detachedId ? <div className="detached-area">{detachedView ? <><div className="tab-bar" role="tablist" aria-label="独立终端">{tab(detachedView, true, 0)}</div><div className="editor-content"><NativeSurface view={detachedView} visible={!blocked} /></div></> : <div className="group-empty">正在载入终端视图…</div>}</div> : !activeServer ? <div className="welcome"><div className="welcome-icon"><TerminalSquare size={33} strokeWidth={1.25} /></div><div className="eyebrow">YOUR WORKSPACE, CONNECTED</div><h1>从一台服务器开始。</h1><p>打开终端，专注于眼前的工作。<br />连接、会话与窗口，在同一个工作台中有序展开。</p><button className="primary large" onClick={() => setServerEditor("new")}><Plus size={16} />添加服务器</button><div className="welcome-hints"><span><Database size={14} />连接信息保存在本机</span><span><Package size={14} />终端以插件独立更新</span></div>{servers.length > 0 && <p className="select-hint">选择左侧服务器以打开工作区</p>}</div> : <>
          <header className="workspace-heading"><div><div className="workspace-name"><ServerIcon size={17} /><h1>{activeServer.name}</h1><span className="protocol-pill">SSH</span></div><p>{activeServer.username}@{activeServer.host}<span>:</span>{activeServer.port}</p></div><div className="workspace-tools"><div className="segmented" aria-label="分屏方向"><button className={workbench.split === "none" ? "active" : ""} aria-label="单区域" title="单区域" onClick={() => void run("workbench.configure", { split: "none" })}><LayoutPanelTop size={16} /></button><button className={workbench.split === "horizontal" ? "active" : ""} aria-label="左右分屏" title="左右分屏" onClick={() => void run("workbench.configure", { split: "horizontal" })}><Columns2 size={16} /></button><button className={workbench.split === "vertical" ? "active" : ""} aria-label="上下分屏" title="上下分屏" onClick={() => void run("workbench.configure", { split: "vertical" })}><Rows2 size={16} /></button></div><button ref={newTerminalButton} className="primary" onClick={() => void openTerminal(activeServer)} disabled={Boolean(connecting)}><Plus size={15} />{connecting ? "连接中…" : "新建终端"}</button></div></header>
          <div className="workspace-body">{workbench.sessionsSide === "left" && sessionPanel}<div className="workspace-editors" ref={workspaceRef}>{centers.length || workbench.split !== "none" ? <div className={`editor-groups ${workbench.split}`}>{renderGroup(0)}{workbench.split !== "none" && <><div className={`resize-handle split ${workbench.split}`} role="separator" tabIndex={0} aria-label="调整分屏大小" onPointerDown={event => resize(event, "split")} onKeyDown={event => { if (["ArrowLeft", "ArrowUp"].includes(event.key)) setSplitRatio(Math.max(.25, splitRatio - .05)); if (["ArrowRight", "ArrowDown"].includes(event.key)) setSplitRatio(Math.min(.75, splitRatio + .05)); }} />{renderGroup(1)}</>}</div> : <div className="workspace-empty" data-drop-placement="center" data-drop-group="0"><TerminalSquare size={31} strokeWidth={1.2} /><h2>工作区已准备好</h2><p>创建终端以连接 {activeServer.name}</p><button onClick={() => void openTerminal(activeServer)} disabled={Boolean(connecting)}><Plus size={14} />新建终端</button><small>Ctrl + Shift + T</small></div>}
            {bottomActive && <section className="bottom-panel" style={{ height: bottomHeight }} data-drop-placement="bottom"><div className="resize-handle bottom" role="separator" tabIndex={0} aria-label="调整底部终端高度" onPointerDown={event => resize(event, "bottom")} onKeyDown={event => { if (event.key === "ArrowUp") setBottomHeight(bottomHeight + 16); if (event.key === "ArrowDown") setBottomHeight(Math.max(160, bottomHeight - 16)); }} /><div className="tab-bar" role="tablist" aria-label="底部终端">{bottomViews.map(view => tab(view, view.viewId === bottomActive.viewId, 2))}</div><NativeSurface view={bottomActive} visible={!blocked} /></section>}
          </div>{workbench.sessionsSide === "right" && sessionPanel}</div>
        </>}
      </main></div>
    <footer className="statusbar"><span><i className="local-indicator" />{count ? `${count} 个终端运行中` : "就绪"}</span><span className="statusbar-right"><button onClick={() => setPage("plugins")}><Package size={12} />插件</button><span>Windows · SSH</span><button aria-label="查看当前工作区信息" title="主窗口关闭后继续在托盘运行，退出会结束全部会话。" onClick={() => setAppMenu(!appMenu)}><CircleHelp size={13} /></button></span></footer>
    {drag && <><div className="drag-targets"><div data-drop-placement="center" data-drop-group="0">移到中央</div><div data-drop-placement="center" data-drop-group="1">移到另一区域</div><div data-drop-placement="bottom">移到底部</div><div data-drop-placement="detached">独立窗口</div></div><div className="drag-preview" style={{ left: drag.x - drag.offsetX, top: drag.y - drag.offsetY }}><TerminalSquare size={14} />{drag.view.title}</div></>}
    {serverEditor && <ServerDialog server={serverEditor === "new" ? undefined : serverEditor} onClose={() => setServerEditor(null)} onSaved={server => { setServerEditor(null); void refresh(); void activateServer(server); }} />}
    {auth && <AuthDialog key={`${auth.server.id}-${auth.fault.code}`} server={auth.server} fault={auth.fault} onClose={() => { if (auth.operationId) canceledOpens.current.add(auth.operationId); setAuth(null); }} onReady={() => { const { server, operationId } = auth; setAuth(null); if (!operationId || !canceledOpens.current.has(operationId)) void openTerminal(server); }} />}
    {page && <ManagementDialog page={page} onClose={() => setPage(null)} />}
    {deleteServer && <Dialog title="删除服务器" subtitle={deleteServer.name} onClose={() => setDeleteServer(null)}><div className="confirm-body"><p>删除连接档案，并结束此服务器当前运行的所有终端。</p><p className="form-help">远程服务器上的文件不会被删除。</p><footer><button onClick={() => setDeleteServer(null)}>取消</button><button className="danger" onClick={async () => { const server = deleteServer; const result = await run("servers.delete", { serverId: server.id, stopSessions: true }); if (result !== undefined) { setDeleteServer(null); if (selectedServerId === server.id) setSelectedServerId(null); } }}>结束会话并删除</button></footer></div></Dialog>}
  </div>;
}
