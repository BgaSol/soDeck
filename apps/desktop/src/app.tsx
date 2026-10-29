import {ConnectionDiagnostics} from './connection-diagnostics';
import type {DiagnosticRef} from '@sodeck/sdk';
import {diagnosticReference,isConnectionAuthenticationError} from '@sodeck/sdk';
import {contributionActions,ToolButtons,type Tool} from './workbench-tools';
import type {WorkbenchContributionsV2} from '@sodeck/sdk';
import {ConnectionHome,ConnectionHomeDialogs} from './connection-home/page';
import {useConnectionHome} from './connection-home/controller';
import {useConnectionCatalog} from './connection-home/catalog';
import type {HomeSnapshot,HomeIntent,HomePage} from '@sodeck/sdk';
import {ApplicationMenuButton} from "./application-menu";
import {Button,IconButton,Input,StatusBar,StatusDot,TabItem} from '@sodeck/ui';
import {ConnectionStatus,DeleteServerDialog} from './connection-status';
import {CommunicationSummary} from './communication-monitor/summary';
import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { Settings, Activity, ArrowLeftToLine, ChevronDown, ChevronLeft, ChevronRight, LayoutPanelTop, Logs, Package, PanelLeftClose, Plus, Power, Server as ServerIcon, X } from "lucide-react";
import { core, connectionCommandTimeout, context, CoreError, errorMessage, onCoreNotification, onThemeError, retryTheme, showMenu } from "@sodeck/sdk";
import { useOverflow } from "./use-overflow";
import { NativeSurface } from "./surface";




import {catalogServer} from './server-catalog-client';
import { ServerTabs } from "./server-tabs";
import { WindowControls } from "./window-controls";
import { AuthDialog } from "./auth-dialog";
import { Notice } from '@sodeck/ui';
import { Dialog } from "./dialog";
import { ManagementDialog } from "./management-dialog";
import { emptyWorkbench } from "./types";
import type { Command, Server, View, Workbench } from "./types";

type CommandRequest = { commandId: string; pluginId?: string; workspaceId?: string; viewId?: string; args?: unknown; operationId?: string };
type Drag = { view: View; x: number; y: number; offsetX: number; offsetY: number };
export function App() {
  const directory=useConnectionCatalog();
  const servers=directory.catalog.items.map(catalogServer);
  const [home,setHome]=useState<{page:HomePage;navigationRevision:string}>({page:{open:true,selected:true,generation:"initial"},navigationRevision:"0"});
  const [themeError,setThemeError]=useState<string>();
  useEffect(()=>onThemeError(setThemeError),[]);
  const [workbench, setWorkbench] = useState<Workbench>(emptyWorkbench);
  const [routesSupported,setRoutesSupported]=useState(false),[multihopSupported,setMultihopSupported]=useState(false);

  const contributionsSupport=useRef(false);
  const [contributionsV2,setContributionsV2]=useState<WorkbenchContributionsV2|null>(null),[badges,setBadges]=useState<Record<string,number>>({});
  const capabilities=useRef<Promise<boolean>|undefined>(undefined);
  const [loadState, setLoadState] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState("");
  const [deleteServer, setDeleteServer] = useState<Server | null>(null);
  const [diagnostic,setDiagnostic]=useState<DiagnosticRef>();
  const [auth, setAuth] = useState<{ server: Server; fault: CoreError; request: CommandRequest } | null>(null), [page, setPage] = useState<"resources" | "calls" | null>(null);
  const [commandInput,setCommandInput]=useState<{view:View;command:Command}|null>(null);
  const [savingInput,setSavingInput]=useState(false);
  const [draft,setDraft]=useState(""),[inputError,setInputError]=useState("");
  const [pageList,setPageList]=useState<"server"|"view"|null>(null),[pageQuery,setPageQuery]=useState("");
  const [dragging, setDragging] = useState(false), [drag, setDrag] = useState<Drag | null>(null);
  const [dockWidths,setDockWidths] = useState<Record<string,number>>({}), [bottomHeight, setBottomHeight] = useState(245);
  const [pendingCommand, setPendingCommand] = useState<string | null>(null), [moveOperation, setMoveOperation] = useState<string | null>(null);
  const tabRow=useRef<HTMLDivElement>(null);
  const tabOverflow=useOverflow(tabRow,workbench.views.map(v=>v.viewId+v.title).join(","));
  useEffect(()=>{document.getElementById(`tab-${workbench.focusedViewId}`)?.scrollIntoView({block:"nearest",inline:"nearest"});},[workbench.focusedViewId]);
  const canceled = useRef(new Set<string>()), refreshing = useRef(false), refreshAgain = useRef(false);
  const workspaceRef = useRef<HTMLDivElement>(null), topRef = useRef<HTMLElement>(null);
  const dragCandidate = useRef<Drag | null>(null), dragged = useRef<Drag | null>(null), suppressClick = useRef<string | null>(null);
  const detachedId = context().detachedViewId;
  const [routeModal,setRouteModal]=useState(false);
  const refresh = useCallback(async () => {
    if (refreshing.current) { refreshAgain.current = true; return; } refreshing.current = true;
    try { do {
      refreshAgain.current = false;
      capabilities.current??=core.call<{capabilities:string[]}>("capabilities.list").then(c=>{contributionsSupport.current=Boolean(c?.capabilities?.includes('workspace.contributions.v2'));return Boolean(c?.capabilities?.includes("servers.routes.v2"));});
      const version=await capabilities.current;setRoutesSupported(version);setMultihopSupported(version);
      const [snapshot, contributions] = await Promise.all([core.call<HomeSnapshot>("ui.connectionHome.snapshot"),core.call<{commands:Command[]}>("workbench.contributions")]);
      const layout=snapshot.workbench;
      setHome(old=>BigInt(snapshot.navigationRevision)>=BigInt(old.navigationRevision)?{page:snapshot.page,navigationRevision:snapshot.navigationRevision}:old);
      if(contributionsSupport.current){const c=await core.call<WorkbenchContributionsV2>('workbench.contributions.v2');setContributionsV2(c);setBadges(Object.fromEntries(c.badges.map(b=>[b.viewId,b.count])));}
      setWorkbench(old=>layout.revision>=old.revision?{ ...emptyWorkbench, ...layout, commands:contributions.commands } as Workbench:old); setLoadState("ready");
    } while (refreshAgain.current); } catch (e) { setLoadState("error"); setError(errorMessage(e)); } finally { refreshing.current = false; }
  }, []);
  const run = useCallback(async (method: string, params: unknown = {}, timeoutMs?:number) => {
    if (["workspaces.move", "windows.merge"].includes(method)) setMoveOperation((params as { operationId?: string }).operationId ?? null);
    try { setError(""); const result = await core.call(method, params, timeoutMs); await refresh(); return result; }
    catch (e) { if(!(e instanceof CoreError&&e.code==="CLOSE_REVIEW_REQUIRED")&&!diagnosticReference(e))setError(errorMessage(e)); return undefined; }
    finally { if (["workspaces.move", "windows.merge"].includes(method)) setMoveOperation(null); }
  }, [refresh]);
  const openHome=useCallback((intent:HomeIntent={kind:"home"})=>{void run("ui.connectionHome.open",{operationId:crypto.randomUUID(),intent});},[run]);
  const connectionHome=useConnectionHome(directory.catalog,directory.ready,directory.refresh,home.page.selected,home.navigationRevision,async(id,operationId)=>{await core.call("workspaces.open",{serverId:id,target:"currentWindow",operationId});await refresh();},!context().detachedViewId);
  const modal = Boolean(diagnostic || routeModal || auth || page || deleteServer || commandInput || pageList || connectionHome.closing || connectionHome.discard);
  const blocked = modal || Boolean(dragging || drag);
  useEffect(() => { void core.call("workbench.modal", { open: blocked }).catch(e => setError(errorMessage(e))); }, [blocked]);
  useEffect(() => {
    void refresh(); void core.call<{ startupError?: string }>("app.info").then(info => { if (info.startupError) setError(info.startupError); }).catch(() => {});
    const off = onCoreNotification(n => {
      if (["workbench.changed", "servers.changed"].includes(n.type)) void refresh();
      if(n.type==='ui.servers.editRequested')openHome({kind:'editServer',serverId:(n.payload as {server:Server}).server.id});
      if(n.type==='views.badge.changed'){const p=n.payload as {viewId:string;count:number};setBadges(old=>({...old,[p.viewId]:p.count}));}
      if (n.type === "workbench.warning") setError((n.payload as { message: string }).message);
      if (n.type === "workbench.openCanceled") { const id = (n.payload as { operationId: string }).operationId; canceled.current.add(id); setAuth(current => current?.request.operationId === id ? null : current); }
      if(n.type==='connectionDiagnostics.open')setDiagnostic((n.payload as {reference:DiagnosticRef}).reference);
      if (n.type === "auth.required") {
        const p = n.payload as CommandRequest & { server: Server; error: { code: string; message: string; details?: unknown } };
        // A background workspace retains its authentication action in the terminal page.
        // Query current ownership/selection rather than using this effect's old render state.
        void core.call<Workbench>("workbench.snapshot").then(current => {
          if (current.activeWorkspaceId === (p.workspaceId??p.server.id) && (!p.operationId || !canceled.current.has(p.operationId)))
            setAuth(old=>(old?.fault.details as {challengeId?:string})?.challengeId===(p.error.details as {challengeId?:string})?.challengeId?old:{ server: p.server, fault: new CoreError(p.error), request: { commandId: p.commandId, workspaceId: p.workspaceId??p.server.id, viewId: p.viewId, args: p.args, operationId: p.operationId } });
        }).catch(e=>setError(errorMessage(e)));
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
    try {
      const timeout=await connectionCommandTimeout(core,request.workspaceId??workbench.views.find(v=>v.viewId===request.viewId)?.workspaceId);
      if(canceled.current.has(operationId))return;
      await core.call("commands.execute", { ...request, operationId }, timeout);
    }
    catch (e) { if (!diagnosticReference(e) && !isConnectionAuthenticationError(e) && !(e instanceof CoreError && e.code==='CANCELED')) setError(errorMessage(e)); }
    finally { setPendingCommand(null); await refresh(); }
  };
  const cancel = async (operationId: string) => { const previous=document.activeElement; canceled.current.add(operationId); await run("workbench.operation.cancel", { operationId }); requestAnimationFrame(()=>{if(previous&&!previous.isConnected)document.querySelector<HTMLButtonElement>(".tab-tools > button")?.focus();}); };
  const openServer = async (server: Server, newWindow = false) => { await run("workspaces.open", { serverId: server.id, target: newWindow ? "newWindow" : "currentWindow", operationId: crypto.randomUUID() }); };
  const activeServer = servers.find(s => s.id === workbench.activeWorkspaceId), primary = workbench.commands.filter(c => c.primary && (c.scope === "global" || c.scope === "server" && activeServer));
  const actions=contributionActions(contributionsV2,workbench.commands);
  const createTab=async()=>{
    if(!activeServer||pendingCommand||!actions.create.length)return;
    let command=actions.create[0];
    if(actions.create.length>1){const r=document.querySelector('.tab-create')?.getBoundingClientRect();const id=await showMenu(actions.create.map((c,i)=>({id:String(i),label:c.title,enabled:true})),{x:r?.left??0,y:r?.bottom??0});if(id===null)return;command=actions.create[Number(id)];if(!command)return;}
    await execute({pluginId:command.pluginId,commandId:command.id,workspaceId:activeServer.id});
  };
  const openTool=(tool:Tool)=>execute({pluginId:tool.pluginId,commandId:tool.commandId,workspaceId:workbench.activeWorkspaceId!});
  const activeWorkspace = workbench.workspaces.find(w=>w.workspaceId===workbench.activeWorkspaceId);
  const dockKey=(edge:string)=>(activeWorkspace?.generation??'')+':'+edge;
  const dockWidth=(edge:string)=>dockWidths[dockKey(edge)]??320;
  const clampDock=(width:number)=>Math.max(260,Math.min(480,innerWidth-360,width));
  const saveDock=(edge:string,width:number)=>{
    if(!activeWorkspace)return;const key=dockKey(edge),value=clampDock(width);
    setDockWidths(previous=>({...previous,[key]:value}));
    void core.call('workbench.dock.configure',{workspaceId:activeWorkspace.workspaceId,workspaceGeneration:activeWorkspace.generation,edge,width:value}).catch(e=>setError(errorMessage(e)));
  };
  useEffect(()=>{
    if(!activeWorkspace)return;let stale=false;
    for(const edge of ['left','right'])void core.call<{width:number}>('workbench.dock.configure',{workspaceId:activeWorkspace.workspaceId,workspaceGeneration:activeWorkspace.generation,edge}).then(r=>{if(!stale)setDockWidths(previous=>({...previous,[activeWorkspace.generation+':'+edge]:r.width}));}).catch(e=>{if(!stale)setError(errorMessage(e));});
    return()=>{stale=true;};
  },[activeWorkspace?.generation]);
  const initializationError = activeWorkspace?.initializationError ?? activeWorkspace?.initializers?.find(i=>i.state==="failed"&&!i.viewId)?.error;
  const initializing = activeWorkspace?.initializers?.some(i=>i.state==="running");
  const commandButton = (c: Command) => <Button key={(c.pluginId??"")+c.id} disabled={Boolean(pendingCommand) || Boolean(activeWorkspace?.initializers?.some(i=>i.commandId===c.id && i.state==="running"))} onClick={() => void execute({ pluginId:c.pluginId, commandId: c.id, ...(c.scope === "server" ? { workspaceId: workbench.activeWorkspaceId! } : {}) })}><Plus size={14} />{c.title}</Button>;
  const moveWorkspace = (id: string, targetWindowId?: string) => { void run("workspaces.move", { workspaceId: id, ...(targetWindowId ? { targetWindowId } : {}), operationId: crypto.randomUUID() }); };
  const moveView = async (v: View, placement: string, group = 0, screenPosition?: { x: number; y: number }) => { await run("views.move", { viewId: v.viewId, placement, group, ...(screenPosition ? { screenPosition } : {}) }); };
  useEffect(() => {
    const keydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {   dragCandidate.current = null; dragged.current = null; setDrag(null); }
      if (e.ctrlKey && e.key.toLowerCase() === "o" && !modal) { e.preventDefault(); openHome(); }
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "t" && !modal && activeServer && actions.create.length) { e.preventDefault(); void createTab(); }
      if (e.altKey && e.key === " ") { e.preventDefault(); void run("windows.control", { action: "systemMenu" }); }
    };
    window.addEventListener("keydown", keydown); return () => window.removeEventListener("keydown", keydown);
  });
  const visibleViews = workbench.views.filter(v => detachedId ? v.viewId === detachedId : !home.page.selected && (v.workspaceId === workbench.activeWorkspaceId || v.workspaceId === "@app" && workbench.windowId === "main") && v.placement !== "detached");
  const centers = visibleViews.filter(v => v.placement === "center"), bottom = visibleViews.filter(v => v.placement === "bottom");
  const surface = (v: View) => <NativeSurface key={`${v.viewId}:${v.mountGeneration}`} view={v} visible={!blocked} commands={workbench.commands} />;
  const resize = (e: ReactPointerEvent, kind: "side" | "bottom", edge = "left") => {
    const x = e.clientX, y = e.clientY, side = dockWidth(edge), height = bottomHeight;
    let width=side;const key=dockKey(edge);
    const target = e.currentTarget; target.setPointerCapture(e.pointerId);
    const move = (next: PointerEvent) => {
      if (kind === "side"){width=clampDock(side+(next.clientX-x)*(edge==='right'?-1:1));setDockWidths(previous=>({...previous,[key]:width}));}
      if (kind === "bottom") setBottomHeight(Math.max(160, Math.min(innerHeight * .65, height - next.clientY + y)));
    };
    const end = () => { target.removeEventListener("pointermove", move as EventListener); target.removeEventListener("pointerup", end); target.removeEventListener("pointercancel", end);if(kind==='side')saveDock(edge,width); };
    target.addEventListener("pointermove", move as EventListener); target.addEventListener("pointerup", end); target.addEventListener("pointercancel", end);
  };
  const menu = async (items: {id:string;label:string;enabled:boolean;separator?:boolean}[], x:number,y:number) => {
    try { return await showMenu(items,{x,y}); } catch(e) { setError(errorMessage(e));return null; }
  };
  const serverContext = async (id:string,x:number,y:number) => {
    const ws=workbench.workspaces.find(w=>w.workspaceId===id);if(!ws)return;
    const targets=[...new Set(["main",...workbench.workspaces.map(w=>w.ownerWindowId)])].filter(w=>w!==ws.ownerWindowId);
    const selected=await menu([{id:"detach",label:"移到新窗口",enabled:true},...targets.map(w=>({id:"window:"+w,label:w==="main"?"合并回主窗口":"移到 "+(servers.find(s=>s.id===workbench.workspaces.find(v=>v.ownerWindowId===w)?.serverId)?.name??"工作区")+" 窗口",enabled:true})),...(workbench.windowRole==="workspace"?[{id:"merge",label:"当前窗口全部合并回主窗口",enabled:true}]:[]),{id:"edit",label:"编辑服务器",enabled:true,separator:true},{id:"close",label:"关闭工作区并结束会话",enabled:true}],x,y);
    if(selected==="detach")moveWorkspace(id);else if(selected?.startsWith("window:"))moveWorkspace(id,selected.slice(7));else if(selected==="merge")void run("windows.merge",{operationId:crypto.randomUUID()});else if(selected==="edit"){const server=servers.find(s=>s.id===id);if(server)openHome({kind:'editServer',serverId:server.id});}else if(selected==="close")void run("workspaces.close",{workspaceId:id});
  };
  const viewContext = async (v:View,x:number,y:number) => {
    const commands=workbench.commands.filter(c=>c.scope==="view"&&(!c.pluginId||c.pluginId===v.pluginId)&&v.actions.includes(c.id));
    const selected=await menu([...commands.map(c=>({id:"command:"+c.id,label:c.title,enabled:true})),{id:"move",label:v.placement==="detached"?"返回所属工作区":"移到独立窗口",enabled:true,separator:true},{id:"close",label:"关闭视图并结束会话",enabled:true}],x,y);
    if(selected==="move")void moveView(v,v.placement==="detached"?"center":"detached");
    else if(selected==="close")void run("views.close",{viewId:v.viewId});
    else if(selected?.startsWith("command:")){const c=commands.find(c=>c.id===selected.slice(8));if(!c)return;if(c.input){setDraft(v.title);setInputError("");setCommandInput({view:v,command:c});}else void execute({commandId:c.id,viewId:v.viewId});}
  };
  const applicationMenu = (selected:string) => {
    if(selected==='communication'){void run('ui.communicationMonitor.open',{operationId:crypto.randomUUID()});return;}
    if(selected==="appearance")void run("ui.settings.open",{operationId:crypto.randomUUID(),pageId:"core:appearance"});else if(selected==="plugins")void run("ui.settings.open",{operationId:crypto.randomUUID(),pageId:"core:plugins"});else if(selected==="resources"||selected==="calls")setPage(selected);else if(selected==="layoutRetry")void run("workbench.preferences.retry");else if(selected==="recall")void run("windows.control",{action:"recall"});else if(selected==="merge")void run("windows.merge",{operationId:crypto.randomUUID()});else if(selected==="exit")void run("app.exit");
  };
  const tab = (v: View, active: boolean, peers: View[]) => <TabItem key={v.viewId} id={`tab-${v.viewId}`} className={`view-tab ${active ? "active" : ""}`} role="tab" selected={active} tabIndex={active ? 0 : -1} title={v.title} aria-label={v.title} onContextMenu={e=>{e.preventDefault();void viewContext(v,e.clientX,e.clientY);}} onClick={() => { if (suppressClick.current === v.viewId) { suppressClick.current = null; return; } void run("views.activate", { viewId: v.viewId }); }} onKeyDown={e => {
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
    <LayoutPanelTop size={14} /><span>{v.title}</span><Button variant="quiet" size="compact" className="icon-button" aria-label={`关闭 ${v.title}`} title="关闭视图并结束其会话" onClick={e => { e.stopPropagation(); void run("views.close", { viewId: v.viewId }); }}><X size={13} /></Button>
  </TabItem>;
  const renderGroup = (_group:number) => {
    const active=centers.find(v=>v.viewId===workbench.focusedViewId)??centers.find(v=>v.active)??centers[0];
    return <section className="editor-group" data-drop-placement="center" data-drop-group="0"><div className="tab-bar"><div ref={tabRow} className="tab-scroll" role="tablist" aria-label="工作区页面">{centers.map(v=>tab(v,v.viewId===active?.viewId,centers))}{actions.create.length>0&&<Button variant="quiet" size="compact" className="tab-create icon-button" aria-label="新建页面" title="新建页面（Ctrl+Shift+T）" disabled={!!pendingCommand||!!initializing} onClick={()=>void createTab()}><Plus size={16}/></Button>}</div><div className="tab-tools">{actions.fallback.filter(c=>c.scope!=="server"||activeServer).map(commandButton)}{tabOverflow && <><Button variant="quiet" size="compact" className="icon-button" aria-label="向左滚动终端页" onClick={()=>document.querySelector('.tab-scroll')?.scrollBy({left:-240})}><ChevronLeft size={14}/></Button><Button variant="quiet" size="compact" className="icon-button" aria-label="向右滚动终端页" onClick={()=>document.querySelector('.tab-scroll')?.scrollBy({left:240})}><ChevronRight size={14}/></Button><Button variant="quiet" size="compact" className="icon-button" aria-label="已打开页面" onClick={()=>{setPageQuery("");setPageList("view");}}><ChevronDown size={14}/></Button></>}</div></div><div className="editor-content">{active?surface(active):<div className="workspace-empty"><LayoutPanelTop size={30}/><h2>{initializationError?"初始页面未能打开":initializing?"正在打开终端":"工作区已准备好"}</h2><p role={initializationError?"alert":initializing?"status":undefined}>{initializationError??(initializing?"正在准备连接…":"打开页面，开始工作")}</p><div>{initializationError?<><Button onClick={()=>void run("workspaces.initialize",{workspaceId:workbench.activeWorkspaceId})}>重试打开</Button><Button onClick={()=>void run("ui.settings.open",{operationId:crypto.randomUUID(),pageId:"core:plugins"})}>查看插件</Button></>:primary.map(commandButton)}</div></div>}</div></section>;
  };
  const dock = (edge: "left" | "right") => {
    const candidates = visibleViews.filter(v => v.placement === edge && v.active && workbench.dockSlots.some(s => s.viewId === v.viewId && s.visible));
    const v = candidates.find(v => v.viewId === workbench.focusedViewId) ?? candidates.find(v => v.workspaceId !== "@app") ?? candidates[0];
    return v && <aside className="sessions-panel" data-edge={edge} style={{ width: dockWidth(edge) }}><div className="panel-heading"><span>{v.title}</span><div><Button variant="quiet" size="compact" className="icon-button" aria-label={`移动 ${v.title}`} title="切换停靠位置" onClick={() => void moveView(v, edge === "left" ? "right" : "left")}><ArrowLeftToLine size={13} /></Button><Button variant="quiet" size="compact" className="icon-button" aria-label={`收起 ${v.title}`} onClick={() => void run("workbench.configure", { dockViewId: v.viewId, visible: false })}><PanelLeftClose size={13} /></Button></div></div>{surface(v)}<div className={`resize-handle side ${edge}`} role="separator" tabIndex={0} aria-label="调整侧栏宽度" onPointerDown={e => resize(e, "side", edge)} onKeyDown={e => { if (e.key === "ArrowLeft") saveDock(edge,dockWidth(edge)-16); if (e.key === "ArrowRight") saveDock(edge,dockWidth(edge)+16); }} /></aside>;
  };
  const bottomActive = bottom.find(v => v.active) ?? bottom[0], detachedView = visibleViews[0];
  return <div className="app-shell">
    <header ref={topRef} className="window-topbar">{detachedId ? <div className="detached-heading">{detachedView && tab(detachedView,true,[detachedView])}</div> : <ServerTabs home={home.page} onHomeClose={()=>void run("ui.connectionHome.close.request",{reason:"close"})} workbench={workbench} servers={servers} run={run} onPicker={() => openHome()} onMenu={(id,x,y)=>void serverContext(id,x,y)} onList={()=>{setPageQuery("");setPageList("server");}} onDragging={setDragging} />}
      <div className="window-drag-space" data-chrome-drag onPointerDown={e => { if (e.button === 0) void run("windows.control", { action: "drag" }); }} onDoubleClick={() => void run("windows.control", { action: "toggleMaximize" })} onContextMenu={e => { e.preventDefault(); void run("windows.control", { action: "systemMenu" }); }} /><IconButton label="设置" icon={Settings} variant="quiet" onClick={()=>void run("ui.settings.open",{operationId:crypto.randomUUID()})}/><ApplicationMenuButton onSelect={applicationMenu} onError={e=>setError(errorMessage(e))}/><WindowControls run={run} />
    </header>
    <div className="app-body">{!detachedId && <nav className="activity-rail" aria-label="工作区工具"><Button variant="quiet" size="compact" className="rail-button" aria-label="打开服务器" onClick={() => openHome()}><ServerIcon size={19} /></Button><ToolButtons edge="left" tools={actions.tools} views={visibleViews} visible={new Set(workbench.dockSlots.filter(s=>s.visible).map(s=>s.viewId))} badges={badges} disabled={!activeServer||!!pendingCommand} onToggle={v=>void run('workbench.configure',{dockViewId:v.viewId,visible:!workbench.dockSlots.find(s=>s.viewId===v.viewId)?.visible})} onOpen={t=>void openTool(t)}/><div className="rail-spacer" /><Button variant="quiet" size="compact" className="rail-button" aria-label="运行资源" onClick={() => setPage("resources")}><Activity size={19} /></Button><Button variant="quiet" size="compact" className="rail-button" aria-label="插件" onClick={() => void run("ui.settings.open",{operationId:crypto.randomUUID(),pageId:"core:plugins"})}><Package size={19} /></Button><Button variant="quiet" size="compact" className="rail-button" aria-label="调用记录" onClick={() => setPage("calls")}><Logs size={19} /></Button></nav>}
      <main className="main-area">{themeError&&<Notice kind="error" action={<Button onClick={()=>void retryTheme()}>重试同步</Button>}>{themeError}</Notice>}{error && <div className="app-error" role="alert"><span>{error}</span><Button variant="quiet" size="compact" className="icon-button" aria-label="收起错误提示" onClick={() => setError("")}><X size={14} /></Button></div>}{workbench.recoveryWarnings.map(w => <div className="app-error" key={w}>{w}</div>)}
        {workbench.pendingOpens.map(op => <div className="connection-progress" key={op.operationId}><span role="status">{op.cancelling ? "正在取消" : "正在处理"} · {servers.find(s => s.id === op.serverId)?.name}</span><Button aria-label={`取消连接到 ${servers.find(s => s.id === op.serverId)?.name ?? "服务器"}`} onClick={() => void cancel(op.operationId)}>取消连接</Button></div>)}{moveOperation && <div className="connection-progress"><span role="status">正在移动工作区…</span><Button onClick={() => void cancel(moveOperation)}>取消移动</Button></div>}
        {detachedId ? <div className="detached-area">{detachedView && <><div className="editor-content">{surface(detachedView)}</div></>}</div> : home.page.selected ? <ConnectionHome controller={connectionHome} catalog={directory.catalog} ready={directory.ready} error={directory.error} refresh={directory.refresh} opened={new Set(workbench.workspaces.map(w=>w.serverId))} onOpen={(item,newWindow)=>void openServer(catalogServer(item),newWindow)} onDelete={item=>setDeleteServer(catalogServer(item))}/> : !activeServer && visibleViews.length===0 ? <div className="welcome"><Button onClick={()=>openHome()}>新建连接</Button></div> : <>
          <div className="workspace-body">{dock("left")}<div className="workspace-editors" ref={workspaceRef}><div className="editor-groups">{renderGroup(0)}</div>
            {bottomActive && <section className="bottom-panel" style={{ height: bottomHeight }} data-drop-placement="bottom"><div className="resize-handle bottom" role="separator" tabIndex={0} aria-label="调整底部面板高度" onPointerDown={e => resize(e, "bottom")} onKeyDown={e => { if (e.key === "ArrowUp") setBottomHeight(bottomHeight + 16); if (e.key === "ArrowDown") setBottomHeight(Math.max(160, bottomHeight - 16)); }} /><div className="tab-bar" role="tablist" aria-label="底部视图">{bottom.map(v => tab(v, v.viewId === bottomActive.viewId, bottom))}</div>{surface(bottomActive)}</section>}
          </div>{dock("right")}</div></>}
      </main>{!detachedId&&<nav className="activity-rail right" aria-label="右侧工作区工具"><ToolButtons edge="right" tools={actions.tools} views={visibleViews} visible={new Set(workbench.dockSlots.filter(s=>s.visible).map(s=>s.viewId))} badges={badges} disabled={!activeServer||!!pendingCommand} onToggle={v=>void run('workbench.configure',{dockViewId:v.viewId,visible:!workbench.dockSlots.find(s=>s.viewId===v.viewId)?.visible})} onOpen={t=>void openTool(t)}/></nav>}</div>
    <StatusBar className="statusbar"><span><StatusDot tone="success"/>{activeServer ? `${activeServer.username}@${activeServer.host}:${activeServer.port}` : "就绪"}</span><span>{routesSupported&&activeWorkspace&&<ConnectionStatus multihop={multihopSupported} key={activeWorkspace.generation} workspaceId={activeWorkspace.workspaceId} generation={activeWorkspace.generation} onModalChange={setRouteModal}/>} {activeServer&&<CommunicationSummary key={activeServer.id} serverId={activeServer.id} onError={setError}/>}</span><span>{workbench.windowRole === "main" ? "主窗口" : workbench.windowRole === "workspace" ? "工作区窗口" : "独立视图"}</span></StatusBar>
    {drag && <><div className="drag-targets"><div data-drop-placement="center" data-drop-group="0">移到中央</div><div data-drop-placement="detached">独立窗口</div></div><div className="drag-preview" style={{ left: drag.x - drag.offsetX, top: drag.y - drag.offsetY }}>{drag.view.title}</div></>}
    {commandInput && <Dialog title={commandInput.command.title} onClose={()=>setCommandInput(null)}><form onSubmit={async e=>{e.preventDefault();if(savingInput)return;setSavingInput(true);try{await core.call("commands.execute",{commandId:commandInput.command.id,viewId:commandInput.view.viewId,args:{[commandInput.command.input!.key]:draft.trim()},operationId:crypto.randomUUID()});setCommandInput(null);await refresh();}catch(e){setInputError(errorMessage(e));}finally{setSavingInput(false);}}}><label>{commandInput.command.input!.label}<Input autoFocus value={draft} onFocus={e=>e.target.select()} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.nativeEvent.isComposing&&e.key==="Enter")e.preventDefault();}}/></label>{inputError&&<p role="alert" className="inline-error">{inputError}</p>}<footer><Button type="button" onClick={()=>setCommandInput(null)}>取消</Button><Button type="submit" disabled={savingInput||[...draft.trim()].length<commandInput.command.input!.minLength||[...draft.trim()].length>commandInput.command.input!.maxLength}>保存</Button></footer></form></Dialog>}
    {pageList && <Dialog dismissOnOutside title={pageList==="server"?"已打开服务器":"已打开页面"} onClose={()=>setPageList(null)}><div className="management-body"><Input aria-label="搜索已打开页面" placeholder="按名称搜索" value={pageQuery} onChange={e=>setPageQuery(e.target.value)}/><div className="opened-list">{(pageList==="server"?workbench.workspaces.map(w=>({id:w.workspaceId,title:servers.find(s=>s.id===w.serverId)?.name??w.serverId,note:w.ownerWindowId===workbench.windowId?"本窗口":"其他窗口"})):workbench.views.filter(v=>v.workspaceId===workbench.activeWorkspaceId).map(v=>({id:v.viewId,title:v.title,note:v.placement==="detached"?"独立窗口":"本窗口"}))).filter(p=>p.title.toLowerCase().includes(pageQuery.toLowerCase())).map(p=><Button key={p.id} onClick={()=>{void run(pageList==="server"?"workspaces.open":"views.activate",pageList==="server"?{serverId:p.id,target:"currentWindow",operationId:crypto.randomUUID()}:{viewId:p.id});setPageList(null);}}><span>{p.title}</span><small>{p.note}</small></Button>)}</div></div></Dialog>}
    {diagnostic&&<ConnectionDiagnostics key={diagnostic.attemptId} reference={diagnostic} onClose={()=>setDiagnostic(undefined)}/>}
    {auth && <AuthDialog key={`${auth.server.id}-${auth.fault.code}-${(auth.fault.details as {challengeId?:string})?.challengeId??""}`} server={auth.server} fault={auth.fault} onClose={() => { if (auth.request.operationId) void cancel(auth.request.operationId); setAuth(null); }} onReady={() => { const request = auth.request; setAuth(null); if (request.operationId && !canceled.current.has(request.operationId)) void connectionCommandTimeout(core,request.workspaceId).then(timeout=>{if(request.operationId&&!canceled.current.has(request.operationId))return run("commands.retry", { operationId: request.operationId },timeout);}).catch(e=>setError(errorMessage(e))); }} />}

    <ConnectionHomeDialogs controller={connectionHome}/>
    {page && <ManagementDialog page={page} onClose={() => setPage(null)} />}
    {deleteServer && <DeleteServerDialog server={deleteServer} onClose={()=>setDeleteServer(null)} onDeleted={()=>{setDeleteServer(null);void refresh();}}/>}
  </div>;
}
