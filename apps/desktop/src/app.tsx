import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { Activity, ArrowLeftToLine, ChevronDown, ChevronLeft, ChevronRight, LayoutPanelTop, Logs, Package, PanelLeftClose, Plus, Power, Server as ServerIcon, X } from "lucide-react";
import { core, context, CoreError, errorMessage, onCoreNotification, showMenu } from "@sodeck/sdk";
import { useOverflow } from "./use-overflow";
import { NativeSurface } from "./surface";
import { ServerDialog } from "./server-dialog";
import { ServerPicker } from "./server-picker";
import { ServerTabs } from "./server-tabs";
import { WindowControls } from "./window-controls";
import { AuthDialog } from "./auth-dialog";
import { Dialog } from "./dialog";
import { ManagementDialog } from "./management-dialog";
import { emptyWorkbench } from "./types";
import type { Command, Server, View, Workbench } from "./types";

type CommandRequest = { commandId: string; workspaceId?: string; viewId?: string; args?: unknown; operationId?: string };
type Drag = { view: View; x: number; y: number; offsetX: number; offsetY: number };
export function App() {
  const [servers, setServers] = useState<Server[]>([]), [workbench, setWorkbench] = useState<Workbench>(emptyWorkbench);
  const [error, setError] = useState(""), [picker, setPicker] = useState(false);
  const [serverEditor, setServerEditor] = useState<Server | "new" | null>(null), [deleteServer, setDeleteServer] = useState<Server | null>(null);
  const [auth, setAuth] = useState<{ server: Server; fault: CoreError; request: CommandRequest } | null>(null), [page, setPage] = useState<"plugins" | "resources" | "calls" | null>(null);
  const [commandInput,setCommandInput]=useState<{view:View;command:Command}|null>(null);
  const [savingInput,setSavingInput]=useState(false);
  const [draft,setDraft]=useState(""),[inputError,setInputError]=useState("");
  const [pageList,setPageList]=useState<"server"|"view"|null>(null),[pageQuery,setPageQuery]=useState("");
  const [dragging, setDragging] = useState(false), [drag, setDrag] = useState<Drag | null>(null);
  const [sideWidth, setSideWidth] = useState(226), [bottomHeight, setBottomHeight] = useState(245);
  const [pendingCommand, setPendingCommand] = useState<string | null>(null), [moveOperation, setMoveOperation] = useState<string | null>(null);
  const tabRow=useRef<HTMLDivElement>(null);
  const tabOverflow=useOverflow(tabRow,workbench.views.map(v=>v.viewId+v.title).join(","));
  useEffect(()=>{document.getElementById(`tab-${workbench.focusedViewId}`)?.scrollIntoView({block:"nearest",inline:"nearest"});},[workbench.focusedViewId]);
  const canceled = useRef(new Set<string>()), refreshing = useRef(false), refreshAgain = useRef(false);
  const workspaceRef = useRef<HTMLDivElement>(null), topRef = useRef<HTMLElement>(null);
  const dragCandidate = useRef<Drag | null>(null), dragged = useRef<Drag | null>(null), suppressClick = useRef<string | null>(null);
  const detachedId = context().detachedViewId;
  const modal = Boolean(picker || serverEditor || auth || page || deleteServer || commandInput || pageList);
  const blocked = modal || Boolean(dragging || drag);
  const refresh = useCallback(async () => {
    if (refreshing.current) { refreshAgain.current = true; return; } refreshing.current = true;
    try { do {
      refreshAgain.current = false;
      const [result, layout] = await Promise.all([core.call<{ servers: Server[] }>("servers.list"), core.call<Workbench>("workbench.snapshot")]);
      setServers(result.servers); setWorkbench({ ...emptyWorkbench, ...layout });
    } while (refreshAgain.current); } catch (e) { setError(errorMessage(e)); } finally { refreshing.current = false; }
  }, []);
  const run = useCallback(async (method: string, params: unknown = {}) => {
    if (["workspaces.move", "windows.merge"].includes(method)) setMoveOperation((params as { operationId?: string }).operationId ?? null);
    try { setError(""); const result = await core.call(method, params); await refresh(); return result; }
    catch (e) { setError(errorMessage(e)); return undefined; }
    finally { if (["workspaces.move", "windows.merge"].includes(method)) setMoveOperation(null); }
  }, [refresh]);
  useEffect(() => { void core.call("workbench.modal", { open: blocked }).catch(e => setError(errorMessage(e))); }, [blocked]);
  useEffect(() => {
    void refresh(); void core.call<{ startupError?: string }>("app.info").then(info => { if (info.startupError) setError(info.startupError); }).catch(() => {});
    const off = onCoreNotification(n => {
      if (["workbench.changed", "servers.changed"].includes(n.type)) void refresh();
      if (n.type === "workbench.warning") setError((n.payload as { message: string }).message);
      if (n.type === "workbench.openCanceled") { const id = (n.payload as { operationId: string }).operationId; canceled.current.add(id); setAuth(current => current?.request.operationId === id ? null : current); }
      if (n.type === "auth.required") {
        const p = n.payload as CommandRequest & { server: Server; error: { code: string; message: string; details?: unknown } };
        if (!p.operationId || !canceled.current.has(p.operationId)) setAuth({ server: p.server, fault: new CoreError(p.error), request: { commandId: p.commandId, workspaceId: p.server.id, viewId: p.viewId, args: p.args, operationId: p.operationId } });
      }
    });
    const interval = setInterval(() => void refresh(), 2000); return () => { off(); clearInterval(interval); };
  }, [refresh]);
  useEffect(() => {
    if (!topRef.current) return;
    const report = () => {
      const rect = (el: Element | null) => { if (!el) return undefined; const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; };
      void core.call("windows.chrome", { titlebar: rect(topRef.current), drag: rect(document.querySelector("[data-chrome-drag]")), maximize: rect(document.querySelector("[data-chrome-maximize]")), tabStrip: rect(document.querySelector("[data-server-tabs]")), tabs: [...document.querySelectorAll<HTMLElement>("[data-workspace-id]")].map(el => ({ workspaceId: el.dataset.workspaceId, ...rect(el) })) }).catch(e => setError(errorMessage(e)));
    };
    const observer = new ResizeObserver(report); observer.observe(topRef.current); window.addEventListener("resize", report); window.addEventListener("scroll", report, true); report();
    return () => { observer.disconnect(); window.removeEventListener("resize", report); window.removeEventListener("scroll", report, true); };
  }, [workbench.workspaceIds.join(","), detachedId]);
  const execute = async (request: CommandRequest) => {
    const operationId = crypto.randomUUID(); setPendingCommand(operationId); setError("");
    try { await core.call("commands.execute", { ...request, operationId }, 130_000); }
    catch (e) { if (!(e instanceof CoreError && ["HOST_KEY_REQUIRED", "HOST_KEY_CHANGED", "AUTH_REQUIRED", "AUTH_FAILED", "CANCELED"].includes(e.code))) setError(errorMessage(e)); }
    finally { setPendingCommand(null); await refresh(); }
  };
  const cancel = async (operationId: string) => { const previous=document.activeElement; canceled.current.add(operationId); await run("workbench.operation.cancel", { operationId }); requestAnimationFrame(()=>{if(previous&&!previous.isConnected)document.querySelector<HTMLButtonElement>(".tab-tools > button")?.focus();}); };
  const openServer = async (server: Server, newWindow = false) => { setPicker(false); await run("workspaces.open", { serverId: server.id, target: newWindow ? "newWindow" : "currentWindow", operationId: crypto.randomUUID() }); };
  const activeServer = servers.find(s => s.id === workbench.activeWorkspaceId), primary = workbench.commands.filter(c => c.primary && (c.scope === "global" || c.scope === "server" && activeServer));
  const commandButton = (c: Command) => <button key={c.id} disabled={Boolean(pendingCommand)} onClick={() => void execute({ commandId: c.id, ...(c.scope === "server" ? { workspaceId: workbench.activeWorkspaceId! } : {}) })}><Plus size={14} />{c.title}</button>;
  const moveWorkspace = (id: string, targetWindowId?: string) => { void run("workspaces.move", { workspaceId: id, ...(targetWindowId ? { targetWindowId } : {}), operationId: crypto.randomUUID() }); };
  const moveView = async (v: View, placement: string, group = 0, screenPosition?: { x: number; y: number }) => { await run("views.move", { viewId: v.viewId, placement, group, ...(screenPosition ? { screenPosition } : {}) }); };
  useEffect(() => {
    const keydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {   dragCandidate.current = null; dragged.current = null; setDrag(null); }
      if (e.ctrlKey && e.key.toLowerCase() === "o" && !modal) { e.preventDefault(); setPicker(true); }
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "t" && !modal && activeServer && primary[0]) { e.preventDefault(); void execute({ commandId: primary[0].id, workspaceId: activeServer.id }); }
      if (e.altKey && e.key === " ") { e.preventDefault(); void run("windows.control", { action: "systemMenu" }); }
    };
    window.addEventListener("keydown", keydown); return () => window.removeEventListener("keydown", keydown);
  });
  const visibleViews = workbench.views.filter(v => detachedId ? v.viewId === detachedId : (v.workspaceId === workbench.activeWorkspaceId || v.workspaceId === "@app" && workbench.windowId === "main") && v.placement !== "detached");
  const centers = visibleViews.filter(v => v.placement === "center"), bottom = visibleViews.filter(v => v.placement === "bottom");
  const surface = (v: View) => <NativeSurface key={`${v.viewId}:${v.mountGeneration}`} view={v} visible={!blocked} commands={workbench.commands} />;
  const resize = (e: ReactPointerEvent, kind: "side" | "bottom", edge = "left") => {
    const x = e.clientX, y = e.clientY, side = sideWidth, height = bottomHeight;
    const target = e.currentTarget; target.setPointerCapture(e.pointerId);
    const move = (next: PointerEvent) => {
      if (kind === "side") setSideWidth(Math.max(200, Math.min(400, side + (next.clientX - x) * (edge === "right" ? -1 : 1))));
      if (kind === "bottom") setBottomHeight(Math.max(160, Math.min(innerHeight * .65, height - next.clientY + y)));
    };
    const end = () => { target.removeEventListener("pointermove", move as EventListener); target.removeEventListener("pointerup", end); target.removeEventListener("pointercancel", end); };
    target.addEventListener("pointermove", move as EventListener); target.addEventListener("pointerup", end); target.addEventListener("pointercancel", end);
  };
  const menu = async (items: {id:string;label:string;enabled:boolean;separator?:boolean}[], x:number,y:number) => {
    try { return await showMenu(items,{x,y}); } catch(e) { setError(errorMessage(e));return null; }
  };
  const serverContext = async (id:string,x:number,y:number) => {
    const ws=workbench.workspaces.find(w=>w.workspaceId===id);if(!ws)return;
    const targets=[...new Set(["main",...workbench.workspaces.map(w=>w.ownerWindowId)])].filter(w=>w!==ws.ownerWindowId);
    const selected=await menu([{id:"detach",label:"移到新窗口",enabled:true},...targets.map(w=>({id:"window:"+w,label:w==="main"?"合并回主窗口":"移到 "+(servers.find(s=>s.id===workbench.workspaces.find(v=>v.ownerWindowId===w)?.serverId)?.name??"工作区")+" 窗口",enabled:true})),...(workbench.windowRole==="workspace"?[{id:"merge",label:"当前窗口全部合并回主窗口",enabled:true}]:[]),{id:"edit",label:"编辑服务器",enabled:true,separator:true},{id:"close",label:"关闭工作区并结束会话",enabled:true}],x,y);
    if(selected==="detach")moveWorkspace(id);else if(selected?.startsWith("window:"))moveWorkspace(id,selected.slice(7));else if(selected==="merge")void run("windows.merge",{operationId:crypto.randomUUID()});else if(selected==="edit"){const server=servers.find(s=>s.id===id);if(server)setServerEditor(server);}else if(selected==="close")void run("workspaces.close",{workspaceId:id});
  };
  const viewContext = async (v:View,x:number,y:number) => {
    const commands=workbench.commands.filter(c=>c.scope==="view"&&v.actions.includes(c.id));
    const selected=await menu([...commands.map(c=>({id:"command:"+c.id,label:c.title,enabled:true})),{id:"move",label:v.placement==="detached"?"返回所属工作区":"移到独立窗口",enabled:true,separator:true},{id:"close",label:"关闭视图并结束会话",enabled:true}],x,y);
    if(selected==="move")void moveView(v,v.placement==="detached"?"center":"detached");
    else if(selected==="close")void run("views.close",{viewId:v.viewId});
    else if(selected?.startsWith("command:")){const c=commands.find(c=>c.id===selected.slice(8));if(!c)return;if(c.input){setDraft(v.title);setInputError("");setCommandInput({view:v,command:c});}else void execute({commandId:c.id,viewId:v.viewId});}
  };
  const applicationMenu = async (x:number,y:number) => {
    const selected=await menu([{id:"plugins",label:"管理插件",enabled:true},{id:"resources",label:"运行资源",enabled:true},{id:"calls",label:"调用记录",enabled:true},{id:"recall",label:"找回所有窗口",enabled:true},{id:"layoutRetry",label:"重试保存布局",enabled:true},...(workbench.windowRole==="workspace"?[{id:"merge",label:"全部合并回主窗口",enabled:true}]:[]),{id:"exit",label:"退出 SoDeck",enabled:true,separator:true}],x,y);
    if(selected==="plugins"||selected==="resources"||selected==="calls")setPage(selected);else if(selected==="layoutRetry")void run("workbench.persistence.retry");else if(selected==="recall")void run("windows.control",{action:"recall"});else if(selected==="merge")void run("windows.merge",{operationId:crypto.randomUUID()});else if(selected==="exit")void run("app.exit");
  };
  const tab = (v: View, active: boolean, peers: View[]) => <div key={v.viewId} id={`tab-${v.viewId}`} className={`view-tab ${active ? "active" : ""}`} role="tab" aria-selected={active} tabIndex={active ? 0 : -1} title={v.title} aria-label={v.title} onContextMenu={e=>{e.preventDefault();void viewContext(v,e.clientX,e.clientY);}} onClick={() => { if (suppressClick.current === v.viewId) { suppressClick.current = null; return; } void run("views.activate", { viewId: v.viewId }); }} onKeyDown={e => {
    if (e.target !== e.currentTarget) return;
    if ((e.key==="F10"&&e.shiftKey)||e.key==="ContextMenu") {e.preventDefault();const r=e.currentTarget.getBoundingClientRect();void viewContext(v,r.left,r.bottom);}
    if (["Enter", " "].includes(e.key)) { e.preventDefault(); void run("views.activate", { viewId: v.viewId }); }
    if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) { e.preventDefault(); const i = peers.indexOf(v), next = peers[e.key === "Home" ? 0 : e.key === "End" ? peers.length - 1 : (i + (e.key === "ArrowRight" ? 1 : -1) + peers.length) % peers.length]; document.getElementById(`tab-${next.viewId}`)?.focus(); void run("views.activate", { viewId: next.viewId }); }
  }} onPointerDown={e => { if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return; const r = e.currentTarget.getBoundingClientRect(); dragCandidate.current = { view: v, x: e.clientX, y: e.clientY, offsetX: e.clientX - r.x, offsetY: e.clientY - r.y }; e.currentTarget.setPointerCapture(e.pointerId); }} onPointerMove={e => {
    const c = dragCandidate.current; if (!c || (!dragged.current && Math.hypot(e.clientX - c.x, e.clientY - c.y) < 8)) return; suppressClick.current = v.viewId; dragged.current = { ...c, x: e.clientX, y: e.clientY }; setDrag(dragged.current);
  }} onPointerUp={e => {
    const current = dragged.current; dragCandidate.current = null; dragged.current = null; setDrag(null); if (!current) return;
    const target = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-drop-placement]");
    if (target) void moveView(v, target.dataset.dropPlacement!, Number(target.dataset.dropGroup ?? 0));
    else if (e.clientX < 0 || e.clientY < 0 || e.clientX > innerWidth || e.clientY > innerHeight) void moveView(v, "detached", 0, { x: e.screenX, y: e.screenY });
  }} onPointerCancel={() => { dragCandidate.current = null; dragged.current = null; setDrag(null); }}>
    <LayoutPanelTop size={14} /><span>{v.title}</span><button className="icon-button" aria-label={`关闭 ${v.title}`} title="关闭视图并结束其会话" onClick={e => { e.stopPropagation(); void run("views.close", { viewId: v.viewId }); }}><X size={13} /></button>
  </div>;
  const renderGroup = (_group:number) => {
    const active=centers.find(v=>v.viewId===workbench.focusedViewId)??centers.find(v=>v.active)??centers[0];
    return <section className="editor-group" data-drop-placement="center" data-drop-group="0"><div className="tab-bar"><div ref={tabRow} className="tab-scroll" role="tablist" aria-label="工作区页面">{centers.map(v=>tab(v,v.viewId===active?.viewId,centers))}</div><div className="tab-tools">{primary.map(commandButton)}{tabOverflow && <><button className="icon-button" aria-label="向左滚动终端页" onClick={()=>document.querySelector('.tab-scroll')?.scrollBy({left:-240})}><ChevronLeft size={14}/></button><button className="icon-button" aria-label="向右滚动终端页" onClick={()=>document.querySelector('.tab-scroll')?.scrollBy({left:240})}><ChevronRight size={14}/></button><button className="icon-button" aria-label="已打开页面" onClick={()=>{setPageQuery("");setPageList("view");}}><ChevronDown size={14}/></button></>}</div></div><div className="editor-content">{active?surface(active):<div className="workspace-empty"><LayoutPanelTop size={30}/><h2>工作区已准备好</h2><p>打开页面，开始工作</p><div>{primary.map(commandButton)}</div></div>}</div></section>;
  };
  const dock = (edge: "left" | "right") => {
    const candidates = visibleViews.filter(v => v.placement === edge && v.active && workbench.dockSlots.some(s => s.viewId === v.viewId && s.visible));
    const v = candidates.find(v => v.viewId === workbench.focusedViewId) ?? candidates.find(v => v.workspaceId !== "@app") ?? candidates[0];
    return v && <aside className="sessions-panel" style={{ width: sideWidth }}><div className="panel-heading"><span>{v.title}</span><div><button className="icon-button" aria-label={`移动 ${v.title}`} title="切换停靠位置" onClick={() => void moveView(v, edge === "left" ? "right" : "left")}><ArrowLeftToLine size={13} /></button><button className="icon-button" aria-label={`收起 ${v.title}`} onClick={() => void run("workbench.configure", { dockViewId: v.viewId, visible: false })}><PanelLeftClose size={13} /></button></div></div>{surface(v)}<div className={`resize-handle side ${edge}`} role="separator" tabIndex={0} aria-label="调整侧栏宽度" onPointerDown={e => resize(e, "side", edge)} onKeyDown={e => { if (e.key === "ArrowLeft") setSideWidth(Math.max(200, sideWidth - 16)); if (e.key === "ArrowRight") setSideWidth(Math.min(400, sideWidth + 16)); }} /></aside>;
  };
  const bottomActive = bottom.find(v => v.active) ?? bottom[0], detachedView = visibleViews[0];
  return <div className="app-shell">
    <header ref={topRef} className="window-topbar">{detachedId ? <div className="detached-heading">{detachedView && tab(detachedView,true,[detachedView])}</div> : <ServerTabs workbench={workbench} servers={servers} run={run} onPicker={() => setPicker(true)} onMenu={(id,x,y)=>void serverContext(id,x,y)} onList={()=>{setPageQuery("");setPageList("server");}} onDragging={setDragging} />}
      <div className="window-drag-space" data-chrome-drag onPointerDown={e => { if (e.button === 0) void run("windows.control", { action: "drag" }); }} onDoubleClick={() => void run("windows.control", { action: "toggleMaximize" })} onContextMenu={e => { e.preventDefault(); void run("windows.control", { action: "systemMenu" }); }} /><button className="application-menu" aria-label="应用菜单" onClick={e=>{const r=e.currentTarget.getBoundingClientRect();void applicationMenu(r.left,r.bottom);}}>应用</button><WindowControls run={run} />
    </header>
    <div className="app-body">{!detachedId && <nav className="activity-rail" aria-label="工作区工具"><button className="rail-button" aria-label="打开服务器" onClick={() => setPicker(true)}><ServerIcon size={19} /></button>{visibleViews.filter(v => ["left", "right"].includes(v.placement)).map(v => <button key={v.viewId} className="rail-button" aria-label={`显示或收起 ${v.title}`} title={v.title} onClick={() => void run("workbench.configure", { dockViewId: v.viewId, visible: !workbench.dockSlots.find(s => s.viewId === v.viewId)?.visible })}><LayoutPanelTop size={19} /></button>)}<div className="rail-spacer" /><button className="rail-button" aria-label="运行资源" onClick={() => setPage("resources")}><Activity size={19} /></button><button className="rail-button" aria-label="插件" onClick={() => setPage("plugins")}><Package size={19} /></button><button className="rail-button" aria-label="调用记录" onClick={() => setPage("calls")}><Logs size={19} /></button></nav>}
      <main className="main-area">{error && <div className="app-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="收起错误提示" onClick={() => setError("")}><X size={14} /></button></div>}{workbench.recoveryWarnings.map(w => <div className="app-error" key={w}>{w}</div>)}
        {workbench.pendingOpens.map(op => <div className="connection-progress" key={op.operationId}><span role="status">{op.cancelling ? "正在取消" : "正在处理"} · {servers.find(s => s.id === op.serverId)?.name}</span><button aria-label={`取消连接到 ${servers.find(s => s.id === op.serverId)?.name ?? "服务器"}`} onClick={() => void cancel(op.operationId)}>取消连接</button></div>)}{moveOperation && <div className="connection-progress"><span role="status">正在移动工作区…</span><button onClick={() => void cancel(moveOperation)}>取消移动</button></div>}
        {detachedId ? <div className="detached-area">{detachedView && <><div className="editor-content">{surface(detachedView)}</div></>}</div> : !activeServer && visibleViews.length === 0 ? <div className="welcome"><div className="welcome-icon"><ServerIcon size={32} strokeWidth={1.3} /></div><h1>打开服务器，开始工作。</h1><p>每台服务器拥有自己的页面与布局。<br />拖动顶部标签，即可在窗口之间移动工作区。</p><button className="primary large" onClick={() => setPicker(true)}><Plus size={16} />打开服务器</button></div> : <>
          <div className="workspace-body">{dock("left")}<div className="workspace-editors" ref={workspaceRef}><div className="editor-groups">{renderGroup(0)}</div>
            {bottomActive && <section className="bottom-panel" style={{ height: bottomHeight }} data-drop-placement="bottom"><div className="resize-handle bottom" role="separator" tabIndex={0} aria-label="调整底部面板高度" onPointerDown={e => resize(e, "bottom")} onKeyDown={e => { if (e.key === "ArrowUp") setBottomHeight(bottomHeight + 16); if (e.key === "ArrowDown") setBottomHeight(Math.max(160, bottomHeight - 16)); }} /><div className="tab-bar" role="tablist" aria-label="底部视图">{bottom.map(v => tab(v, v.viewId === bottomActive.viewId, bottom))}</div>{surface(bottomActive)}</section>}
          </div>{dock("right")}</div></>}
      </main></div>
    <footer className="statusbar"><span><i className="local-indicator" />{activeServer ? `${activeServer.username}@${activeServer.host}:${activeServer.port}` : "就绪"}</span><span>{workbench.windowRole === "main" ? "主窗口" : workbench.windowRole === "workspace" ? "工作区窗口" : "独立视图"}</span></footer>
    {drag && <><div className="drag-targets"><div data-drop-placement="center" data-drop-group="0">移到中央</div><div data-drop-placement="detached">独立窗口</div></div><div className="drag-preview" style={{ left: drag.x - drag.offsetX, top: drag.y - drag.offsetY }}>{drag.view.title}</div></>}
    {commandInput && <Dialog title={commandInput.command.title} onClose={()=>setCommandInput(null)}><form onSubmit={async e=>{e.preventDefault();if(savingInput)return;setSavingInput(true);try{await core.call("commands.execute",{commandId:commandInput.command.id,viewId:commandInput.view.viewId,args:{[commandInput.command.input!.key]:draft.trim()},operationId:crypto.randomUUID()});setCommandInput(null);await refresh();}catch(e){setInputError(errorMessage(e));}finally{setSavingInput(false);}}}><label>{commandInput.command.input!.label}<input autoFocus value={draft} onFocus={e=>e.target.select()} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.nativeEvent.isComposing&&e.key==="Enter")e.preventDefault();}}/></label>{inputError&&<p role="alert" className="inline-error">{inputError}</p>}<footer><button type="button" onClick={()=>setCommandInput(null)}>取消</button><button type="submit" disabled={savingInput||[...draft.trim()].length<commandInput.command.input!.minLength||[...draft.trim()].length>commandInput.command.input!.maxLength}>保存</button></footer></form></Dialog>}
    {pageList && <Dialog dismissOnOutside title={pageList==="server"?"已打开服务器":"已打开页面"} onClose={()=>setPageList(null)}><div className="management-body"><input aria-label="搜索已打开页面" placeholder="按名称搜索" value={pageQuery} onChange={e=>setPageQuery(e.target.value)}/><div className="opened-list">{(pageList==="server"?workbench.workspaces.map(w=>({id:w.workspaceId,title:servers.find(s=>s.id===w.serverId)?.name??w.serverId,note:w.ownerWindowId===workbench.windowId?"本窗口":"其他窗口"})):workbench.views.filter(v=>v.workspaceId===workbench.activeWorkspaceId).map(v=>({id:v.viewId,title:v.title,note:v.placement==="detached"?"独立窗口":"本窗口"}))).filter(p=>p.title.toLowerCase().includes(pageQuery.toLowerCase())).map(p=><button key={p.id} onClick={()=>{void run(pageList==="server"?"workspaces.open":"views.activate",pageList==="server"?{serverId:p.id,target:"currentWindow",operationId:crypto.randomUUID()}:{viewId:p.id});setPageList(null);}}><span>{p.title}</span><small>{p.note}</small></button>)}</div></div></Dialog>}
    {picker && <ServerPicker servers={servers} workspaces={workbench.workspaces} onClose={() => setPicker(false)} onOpen={(server, newWindow) => void openServer(server, newWindow)} onEdit={server => { setPicker(false); setServerEditor(server); }} onDelete={server => { setPicker(false); setDeleteServer(server); }} />}
    {serverEditor && <ServerDialog server={serverEditor === "new" ? undefined : serverEditor} onClose={() => { setServerEditor(null); void refresh(); }} onSaved={server => { setServerEditor(null); void openServer(server); }} />}
    {auth && <AuthDialog key={`${auth.server.id}-${auth.fault.code}`} server={auth.server} fault={auth.fault} onClose={() => { if (auth.request.operationId) void cancel(auth.request.operationId); setAuth(null); }} onReady={() => { const request = auth.request; setAuth(null); if (request.operationId && !canceled.current.has(request.operationId)) void run("commands.retry", { operationId: request.operationId }); }} />}
    {page && <ManagementDialog page={page} onClose={() => setPage(null)} />}
    {deleteServer && <Dialog title="删除服务器" subtitle={deleteServer.name} onClose={() => setDeleteServer(null)}><div className="confirm-body"><p>删除连接档案，并结束此服务器当前运行的所有会话。</p><p className="form-help">远程服务器上的文件不会被删除。</p><footer><button onClick={() => setDeleteServer(null)}>取消</button><button className="danger" onClick={async () => { const result = await run("servers.delete", { serverId: deleteServer.id, stopSessions: true }); if (result !== undefined) setDeleteServer(null); }}>结束会话并删除</button></footer></div></Dialog>}
  </div>;
}
