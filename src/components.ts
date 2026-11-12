import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { themeTokens } from "./theme";
import { ClipboardBridge } from "./clipboard";
import { LocalFileDrop } from "./local-files";
import { ComponentData } from "./component-data";
import { ComponentState } from "./component-state";
import { attachMenuPosition, type MenuPosition, type MenuReference } from "./menu-position.js";
import { escape as e, icon, replace, patchChrome } from "./ui";
import type { Context, Projection, Workspace, DropTarget } from "./workspaces";
import type { NetworkSnapshot } from "./core";
import type { CloseTarget } from "./close";
import { panelGeometry, type PanelWidths, type PanelSide } from "./panel-layout";
import {centerMinimum,effectiveRatio,ratioBounds,splitNode,splitWidth,withSplit} from './center-layout';

type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
interface Definition { uiApiVersion?:string; key: string; id: string; pluginId: string; pluginName: string; pluginInstanceId: string; title: string; icon: string; container: string; instancePolicy: string; entry: string; available: boolean; createAction?: string; locateEntry: boolean }
interface Preference { side: string; pinned: boolean; startup: string; order: number }
interface View { id: string; definitionKey: string; pluginInstanceId: string; context: Context; title: string; dirty: boolean; busy: boolean; error?: string; lastUsed: number; uiRequested?: boolean }
export type CenterNode = {type:"group";groupId:string}|{type:"split";id:string;axis:"x"|"y";ratio:number;first:CenterNode;second:CenterNode};
export interface ViewGroup {id:string;viewIds:string[];activeViewId:string|null;explicitEmpty:boolean}
export interface WorkbenchHost {id:string;center:CenterNode;groups:Record<string,ViewGroup>;left:{viewIds:string[];activeViewId:string|null};right:{viewIds:string[];activeViewId:string|null};dialogs?:string[];focusedGroupId:string;zoomGroupId:string|null;revision:number}
interface Transfer {closeOwner?:string;id:string;workspaceId:string;source:string;target:string;viewIds:string[];phase:"preparing"|"draining"|"restoring"}
interface ReturnLocation {windowId:string;groupId?:string;side?:string}
interface ToolEntry {key:string;viewId:string|null;location:{windowId:string;hostId:string;side?:string;order?:number;groupId?:string}|null;active:boolean;pinned:boolean;available:boolean}
interface ToolResult {status:string;revision?:number;persistence?:string;error?:string;saveError?:string}
interface UiWorkspace {tools?:ToolEntry[];returnLocations?:Record<string,ReturnLocation>;groupReturnLocations?:Record<string,ReturnLocation>;unavailableTools?:{key:string;groupId?:string;side?:string}[];otherViews?:{id:string;definitionKey:string;title:string;windowId:string}[]; views: View[]; preferences: Record<string, Preference>; left: string | null; right: string | null; editor: string | null; host?:WorkbenchHost }
interface ActionDefinition { key: string; title: string; available: boolean; reason?: string }
interface ActionResult { status: "pending" | "completed" | "failed" | "invalidated"; requestId: string; context: Context; viewId?: string; error?: string }
export interface UiProjection { definitions: Record<string, Definition>; actions?: Record<string, ActionDefinition>; defaultCreateAction?: string; pendingAction?: ActionResult[]; workspaces: Record<string, UiWorkspace>; error?: string; layoutSaveError?:string; transfers?:Transfer[] }
interface Surface { readOnly?:boolean; transferEpoch?:string; node: HTMLDivElement; frame: HTMLIFrameElement; view: View; token?: string; chain: Promise<unknown>; deferred: Map<string,MessageEvent>; visible: boolean; initialized: boolean; mounted?: boolean; acceptsLocalFiles?: boolean; focusPending?: number; flushable?: boolean; flushing?: {id: string; done: (error?: string) => void}; timeout: ReturnType<typeof setTimeout> }
interface Shell { node: HTMLElement; surfaces: Map<string, Surface>; observer: ResizeObserver; workspace: Workspace; ui: UiWorkspace; railCapacity?: number; layoutFrame?: number }
interface MenuEntry { label: string; action: () => Promise<unknown>; extra?: MenuEntry; checked?: boolean; disabled?: boolean; separatorBefore?: boolean }
const empty = (): UiWorkspace => ({ views: [], preferences: {}, left: null, right: null, editor: null });

/** Shell and frames have different lifetimes. Rendering chrome never reconstructs a mounted iframe. */
export class Components {
  private fileDrop = new LocalFileDrop(event => {
    for (const shell of this.shells.values()) for (const surface of shell.surfaces.values()) {
      if (!surface.visible || !surface.mounted || !surface.acceptsLocalFiles) continue;
      const rect = surface.frame.getBoundingClientRect(), ratio = window.devicePixelRatio || 1;
      const x = (event.position?.x ?? -1) / ratio - rect.left;
      const y = (event.position?.y ?? -1) / ratio - rect.top;
      const inside = x >= 0 && y >= 0 && x < rect.width && y < rect.height;
      this.post(surface, { type: "localFileDrop", dropId: event.dropId, dropType: inside ? event.type : "leave", x, y });
    }
  });
  private clipboard = new ClipboardBridge(() => this.call<string>("clipboard_text", {operation:"read"}), text => this.call<void>("clipboard_text", {operation:"write",text}));
  private data: ComponentData;
  private shared: ComponentState;
  private surfaceLayer=document.createElement("div");
  private suppressClickUntil=0;
  private splitGesture?:{shell:Shell;cancel:()=>void};
  private shells = new Map<string, Shell>();
  private definitions: Record<string, Definition> = {};
  private state?: Projection;
  private menu?: HTMLElement;
  private menuAnchor?: HTMLElement;
  private menuPosition?: MenuPosition;
  private boundTabs = new WeakSet<Element>();
  private disposed = false;
  private transfers:Transfer[]=[];
  private transferJobs=new Set<string>();
  private movingViews=new Set<string>();
  private pendingMoves=new Map<string,Promise<void>>();
  private drag?:{shell:Shell;scope:{kind:'view'|'group'|'tool';id:string};handle:HTMLElement;trigger:HTMLElement;revision:number;context:string;hostId:string;instance?:string;focusEpoch:number;pointer:number;x:number;y:number;localPoint:{x:number;y:number};offsetX:number;offsetY:number;token?:string;starting?:Promise<void>;finished:boolean;sampling:boolean;ghost?:HTMLElement;toolTarget?:{side:string;beforeKey:string|null}};
  private creating = new Set<string>();
  private defaultAction?: ActionDefinition;
  private pendingActions = new Map<string, ActionResult & { focusEpoch?: number }>();
  private actionTimer?: ReturnType<typeof setTimeout>;
  private pollingActions = false;
  private resumeActionPolling = () => this.scheduleActionPolling();
  private layoutError?: string;
  private layoutSaveFailed=false;
  private focusEpoch = 0;
  private interaction = () => { this.focusEpoch++; };
  private theme = new MutationObserver(() => this.broadcast());
  private networkTimer = setInterval(() => { void this.updateNetwork(); }, 1000);
  private networkPending = false;
  private settingsListener?: UnlistenFn;
  private locateListener?:UnlistenFn;
  constructor(private content: HTMLElement, private call: Call, private notify: (message: string) => void, private refresh: () => Promise<void>, private requestClose: (target: CloseTarget) => Promise<boolean>, private widths: (workspace: Workspace) => PanelWidths, private cancelResize: () => void) {
    this.surfaceLayer.className="component-surfaces";content.append(this.surfaceLayer);
    this.data = new ComponentData(call);
    this.shared = new ComponentState(call);
    void listen<{pluginId:string}>("core-settings-changed", event => {
      for (const shell of this.shells.values()) for (const surface of shell.surfaces.values()) if (surface.token && this.definitions[surface.view.definitionKey]?.pluginId === event.payload.pluginId) void this.refreshSettings(surface);
    }).then(unlisten => {if(this.disposed)unlisten();else this.settingsListener=unlisten;}).catch(()=>{});
    void listen<{workspaceId:string;viewId:string}>("workbench-locate-view",event=>{void this.refresh().then(()=>{const surface=this.shells.get(event.payload.workspaceId)?.surfaces.get(event.payload.viewId);if(surface)this.focusSurface(surface);});}).then(unlisten=>{if(this.disposed)unlisten();else this.locateListener=unlisten;});
    window.addEventListener("message", this.message);
    document.addEventListener("visibilitychange", this.resumeActionPolling);
    this.theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", this.systemTheme);
    content.addEventListener("click", event => {
      if(Date.now()<this.suppressClickUntil){event.preventDefault();event.stopImmediatePropagation();return;}
      const button = (event.target as Element).closest<HTMLElement>("[data-component-action]");
      const shell = button?.closest<HTMLElement>(".server-workbench");
      if (button && shell) void this.run(() => this.action(shell.dataset.id!, button.dataset.componentAction!, button.dataset.key ?? "", button, (event as MouseEvent).detail === 0));
    });
    document.addEventListener("pointerdown", this.outside);
    document.addEventListener("pointerdown", this.interaction, true);
    document.addEventListener("keydown", this.interaction, true);
    content.addEventListener('pointerdown',this.dragDown);
    content.addEventListener('pointermove',this.dragMove);
    content.addEventListener('pointerup',this.dragUp);
    content.addEventListener('pointercancel',this.dragCancel);
    document.addEventListener('keydown',this.layoutKey);
    content.addEventListener('lostpointercapture',this.dragLost);window.addEventListener('blur',this.cancelGestures);
    content.addEventListener('contextmenu',this.toolMenuEvent);content.addEventListener('keydown',this.toolMenuEvent);
  }
  private invoke<T>(operation: string, args: Record<string, unknown> = {}) { return this.call<T>("component_ui", { operation, args }); }
  async sync(state: Projection) {
    if(this.state?.window.activeWorkspace!==state.window.activeWorkspace)this.closeMenu();
    const ui = await this.invoke<UiProjection>("snapshot");
    if (this.disposed) return;
    this.state = state; this.definitions = ui.definitions; this.transfers=ui.transfers??[];
    this.layoutSaveFailed=Boolean(ui.layoutSaveError);
    this.defaultAction = ui.defaultCreateAction ? ui.actions?.[ui.defaultCreateAction] : undefined;
    if (ui.error && ui.error !== this.layoutError) { this.layoutError = ui.error; this.notify(ui.error); }
    const incomingWorkspaces=state.incomingWorkspaces??(state.incoming?[state.incoming]:[]);
    const wanted = [...state.workspaces, ...incomingWorkspaces.filter(w=>!state.workspaces.some(active=>active.context.workspaceId===w.context.workspaceId))];
    for (const [id, pending] of this.pendingActions) if (!wanted.some(w => JSON.stringify(w.context) === JSON.stringify(pending.context))) this.pendingActions.delete(id);
    for (const pending of ui.pendingAction ?? []) if (!this.pendingActions.has(pending.context.workspaceId)) this.pendingActions.set(pending.context.workspaceId, pending);
    for (const [id, shell] of this.shells) if (!wanted.some(w => w.context.workspaceId === id)) { this.destroy(shell); this.shells.delete(id); }
    this.content.querySelector(".receiving-workspace")?.remove();
    for (const w of wanted) {
      const id = w.context.workspaceId;
      let shell = this.shells.get(id);
      if (!shell) {
        const node = document.createElement("section"); node.className = "server-workbench"; node.dataset.id = id;
        node.innerHTML = `<aside class="workbench-rail left-rail sd-edge-right" aria-label="左侧组件"></aside><aside class="workbench-panel left-panel sd-edge-right"><header class="sd-panel-header sd-edge-bottom"></header><div class="component-slot"></div><div role="separator" aria-label="左侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="left"></div></aside><section class="workbench-center workbench-layout"></section><aside class="workbench-panel right-panel sd-edge-left"><header class="sd-panel-header sd-edge-bottom"></header><div class="component-slot"></div><div role="separator" aria-label="右侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="right"></div></aside><aside class="workbench-rail right-rail sd-edge-left" aria-label="右侧组件"></aside><footer class="workspace-status sd-status-bar sd-edge-top"></footer>`;
        const observer = new ResizeObserver(() => this.scheduleLayout(id)); observer.observe(node);
        node.querySelectorAll(".component-slot").forEach(slot => observer.observe(slot));
        shell = { node, surfaces: new Map(), observer, workspace: w, ui: empty() }; this.shells.set(id, shell); this.content.append(node);
      }
      if (shell.workspace.connected !== w.connected) for (const surface of shell.surfaces.values()) this.post(surface, {type: "connectionChanged", connected: w.connected});
      shell.workspace = w; shell.ui = ui.workspaces[id] ?? empty();
      const drag=this.drag;
      if(drag?.shell===shell&&!drag.finished&&(state.window.activeWorkspace!==id||shell.ui.host?.id!==drag.hostId||shell.ui.host.revision!==drag.revision||JSON.stringify(w.context)!==drag.context||(drag.instance&&this.definitions[drag.scope.kind==='tool'?drag.scope.id:shell.ui.views.find(v=>v.id===drag.scope.id)?.definitionKey??'']?.pluginInstanceId!==drag.instance)))void this.finishDrag(true);
      const incoming = incomingWorkspaces.some(w=>w.context.workspaceId===id) && !state.workspaces.some(w=>w.context.workspaceId===id);
      shell.node.hidden = !incoming && state.window.activeWorkspace !== id;
      shell.node.classList.toggle("incoming-component-workspace", incoming);
      shell.node.setAttribute("aria-label", `${w.name} 的工作区`);
      this.paint(shell);
      this.setCloseBlocked([...this.closeBlocked]);
    }
    if (!wanted.length) { const placeholder = document.createElement("div"); placeholder.className = "workspace-empty receiving-workspace"; placeholder.textContent = "此窗口没有工作区"; this.content.append(placeholder); }
    this.syncTransfers();
    this.scheduleActionPolling();
  }
  private paint(shell: Shell) {
    const { node, ui: w } = shell;
    for (const side of ["left", "right"]) {
      const selected = side === "left" ? w.left : w.right;
      const entries = this.sideTools(shell,side).filter(t=>t.pinned||t.active),capacity=shell.railCapacity??20;
      let visible=entries.slice(0,capacity);const active=entries.find(t=>t.active);
      if(active&&!visible.includes(active))visible=[...visible.slice(0,Math.max(0,capacity-1)),active].sort((a,b)=>(a.location?.order??0)-(b.location?.order??0));
      patchChrome(node.querySelector(`.${side}-rail`)!, visible.map(t => {
        const d=this.definitions[t.key];
        return `<button class="sd-icon tool-entry" data-tool-key="${e(t.key)}" data-component-action="toggle" data-key="${e(t.key)}" aria-label="${e(d.title)}" aria-expanded="${t.active}" title="${e(d.title)} · ${e(d.pluginName)}${t.pinned?'':' · 临时入口'}" ${!t.available ? "disabled" : ""}>${icon(d.icon)}</button>`;
      }).join("") + (w.unavailableTools??[]).filter(t=>t.side===side).map(t=>`<button class="sd-icon" disabled title="${e(t.key)} 未安装或未启用；已保留停靠位置" aria-label="${e(t.key)} 不可用">${icon('plug')}</button>`).join('')+ `<button class="component-more sd-icon" data-component-action="more" data-key="${side}" aria-label="${side === "left" ? "左" : "右"}侧更多组件">${icon("more")}</button>`);
      const view = w.views.find(v => v.id === selected);
      patchChrome(node.querySelector(`.${side}-panel header`)!, view ? `<span title="${e(this.definitions[view.definitionKey]?.pluginName)}">${e(view.title)}</span><button class="sd-icon" data-component-action="collapse" data-key="${side}" aria-label="收起${e(view.title)}">${icon("minus")}</button>` : "");
    }
    this.paintCenter(shell);
    patchChrome(node.querySelector(".workspace-status")!, `<div class="workspace-connection" role="status" aria-label="连接状态" title="${e(shell.workspace.endpoint)}"><span class="health-dot ${shell.workspace.connected ? "good" : "warning"}"></span><span>${shell.workspace.connected ? "已连接" : "连接已断开"}</span><span class="workspace-endpoint">${e(shell.workspace.endpoint)}</span></div><button class="sd-status-action" data-reconnect-server="${e(shell.workspace.context.serverId)}" ${shell.workspace.connected ? "hidden" : ""}>重新连接</button><span class="workspace-status-spacer"></span>${this.layoutSaveFailed?`<button class="sd-status-action" data-component-action="retry-layout-save">重试保存布局</button>`:""}<button class="workspace-network sd-status-action" data-action="status" title="服务器连接传输速率；点击查看应用状态与插件业务量">↓ 0 B/s ↑ 0 B/s</button>`);
    for (const [id, surface] of shell.surfaces) if (!w.views.some(v => v.id === id)) { this.removeSurface(surface); shell.surfaces.delete(id); }
    for (const v of w.views) {
      if (v.uiRequested === false) continue;
      const d = this.definitions[v.definitionKey]; if (!d) continue;
      let surface = shell.surfaces.get(v.id);
      if (surface && surface.view.pluginInstanceId !== v.pluginInstanceId) { this.removeSurface(surface); shell.surfaces.delete(v.id); surface = undefined; }
      if (surface && (surface.view.context.connectionId !== v.context.connectionId || surface.view.context.generation !== v.context.generation)) {
        if(surface.token)this.clipboard.discard(surface.token);
        this.post(surface, {type: "contextChanged", context: v.context});
        this.removeSurface(surface);shell.surfaces.delete(v.id);surface=undefined;
      }
      if (!surface) {
        const host = document.createElement("div"); host.className = "component-surface"; host.dataset.viewId = v.id;
        const frame = document.createElement("iframe"); frame.title = `${v.title} · ${shell.workspace.name}`;
        frame.allow = "clipboard-write";
        frame.src = new URL(`${d.pluginId}/${d.entry}`, convertFileSrc("", "sodeck-plugin")).href;
        if(d.container === "dialog") {
          const modal=document.createElement("dialog"); modal.className="plugin-view-dialog";modal.setAttribute("aria-label",v.title);
          const header=document.createElement("header"),title=document.createElement("span"),close=document.createElement("button");title.textContent=v.title;close.textContent="×";close.setAttribute("aria-label","关闭弹窗");
          const requestClose=()=>void this.run(()=>this.close([shell.workspace.context.workspaceId],v.id));
          close.addEventListener("click",requestClose);modal.addEventListener("cancel",event=>{event.preventDefault();requestClose();});header.append(title,close);modal.append(header,frame);host.append(modal);
        } else host.append(frame);
        this.surfaceLayer.append(host);
        surface = { node: host, frame, view: v, chain: Promise.resolve(), deferred:new Map(), visible: false, initialized: false, timeout: setTimeout(() => this.surfaceError(surface!, "组件界面未在 8 秒内就绪"), 8000) };
        surface.transferEpoch=this.transfers.find(t=>t.target===this.state?.window.id&&t.viewIds.includes(v.id)&&t.phase==="restoring")?.id;
        shell.surfaces.set(v.id, surface);
      }
      const transfer=this.transfers.find(t=>t.target===this.state?.window.id&&t.viewIds.includes(v.id));
      const epoch=transfer?.phase==='restoring'?transfer.id:undefined;
      if(surface.transferEpoch!==epoch&&epoch){this.removeSurface(surface);shell.surfaces.delete(v.id);queueMicrotask(()=>this.paint(shell));continue;}
      surface.transferEpoch=epoch;
      surface.readOnly=Boolean(transfer);
      surface.view = v;
      surface.frame.title = `${v.title} · ${shell.workspace.name}`;
      if (!d.available) this.surfaceError(surface, "插件已停止，页面状态仍保留。请查看应用状态与日志。");
    }
    this.scheduleLayout(shell.workspace.context.workspaceId);
  }
  private paintCenter(shell:Shell) {
    if(this.splitGesture?.shell===shell)return;
    const host=shell.ui.host,center=shell.node.querySelector<HTMLElement>(".workbench-center")!;
    if(!host){patchChrome(center,'<div class="workspace-empty">正在准备工作台…</div>');return;}
    const scrolls=new Map([...center.querySelectorAll<HTMLElement>(".view-group")].map(g=>[g.dataset.group,g.querySelector(".component-tab-list")?.scrollLeft??0]));
    const group=(id:string)=>{
      const g=host.groups[id],pages=g.viewIds.map(id=>shell.ui.views.find(v=>v.id===id)).filter(Boolean) as View[];
      const missing=(shell.ui.unavailableTools??[]).filter(t=>t.groupId===id);
      const workspaceId=shell.workspace.context.workspaceId,pending=this.pendingActions.get(workspaceId);
      const canCreate=shell.workspace.connected&&this.defaultAction?.available&&!pending&&!this.creating.has(workspaceId);
      const createTitle=pending&&!this.creating.has(workspaceId)?'结果尚未确认':this.creating.has(workspaceId)?'正在创建…':this.defaultAction?.title||'未设置默认行为';
      return `<section class="view-group ${host.focusedGroupId===id?'focused':''}" data-group="${e(id)}" aria-label="标签组"><header class="component-tabs sd-tab-strip sd-edge-bottom" tabindex="0" aria-label="分屏标签栏"><div class="component-tab-list" role="tablist" aria-label="标签组页面">${missing.map(t=>`<button class="sd-quiet" disabled title="${e(t.key)} 未安装或未启用">${icon('plug')}${e(t.key)}</button>`).join('')}${pages.map(v=>`<div class="component-tab sd-tab ${g.activeViewId===v.id?'selected':''}" data-view="${e(v.id)}"><button role="tab" aria-selected="${g.activeViewId===v.id}" tabindex="${g.activeViewId===v.id?0:-1}" data-component-action="select" data-key="${e(v.id)}" title="${e(v.title)}">${icon(this.definitions[v.definitionKey]?.icon)}<span>${e(v.title)}${v.dirty?' ●':''}</span></button><button class="sd-icon" data-component-action="close" data-key="${e(v.id)}" aria-label="${this.definitions[v.definitionKey]?.container==='sidebar'?'隐藏':'关闭'} ${e(v.title)}">${icon('close')}</button></div>`).join('')}</div><button class="sd-icon" data-component-action="create" title="${e(createTitle)}" aria-label="${e(this.defaultAction?.title||'新建页面')}" ${canCreate?'':'disabled'}>${icon('plus')}</button></header><div class="component-slot central-slot">${missing.map(t=>`<div class="unavailable-tool" role="status">${icon('plug')}<span>${e(t.key)} 未安装或未启用；已保留此工具位置。</span></div>`).join('')}${pages.length?'':`<div class="workspace-empty"><p>${shell.workspace.connected?'点击＋新建，或拖入标签。':'连接已断开；仍可整理分屏。'}</p></div>`}</div></section>`;
    };
    const render=(n:CenterNode,width:number,height:number):string=>{
      if(n.type==='group')return group(n.groupId);
      const ratio=effectiveRatio(n,width,height),bounds=ratioBounds(n,width,height),space=(n.axis==='x'?width:height)-splitWidth;
      return `<div class="workbench-split axis-${n.axis}" data-split="${e(n.id)}" data-axis="${n.axis}" style="${n.axis==='x'?'grid-template-columns':'grid-template-rows'}:minmax(0,${ratio}fr) ${splitWidth}px minmax(0,${1-ratio}fr)">${render(n.first,n.axis==='x'?space*ratio:width,n.axis==='y'?space*ratio:height)}<div class="split-handle" role="separator" tabindex="0" data-split-ratio="${e(n.id)}" aria-label="调整${n.axis==='x'?'左右':'上下'}分屏比例" aria-orientation="${n.axis==='x'?'vertical':'horizontal'}" aria-valuemin="${Math.ceil(bounds.min*100)}" aria-valuemax="${Math.floor(bounds.max*100)}" aria-valuenow="${Math.round(ratio*100)}"></div>${render(n.second,n.axis==='x'?space*(1-ratio):width,n.axis==='y'?space*(1-ratio):height)}</div>`;
    };
    const minimum=centerMinimum(host.center);
    const compact=center.clientWidth>0&&(center.clientWidth<minimum.width||center.clientHeight<minimum.height)&&Object.keys(host.groups).length>1;
    const zoom=host.zoomGroupId||(compact?host.focusedGroupId:null);
    const hiddenViews=zoom?Object.values(host.groups).filter(g=>g.id!==zoom).flatMap(g=>g.viewIds).map(id=>shell.ui.views.find(v=>v.id===id)).filter(Boolean) as View[]:[];
    const hiddenStatus=[hiddenViews.filter(v=>v.dirty).length?`${hiddenViews.filter(v=>v.dirty).length} 页未保存`:'',hiddenViews.filter(v=>v.error).length?`${hiddenViews.filter(v=>v.error).length} 页错误`:''].filter(Boolean).join(' · ');
    patchChrome(center,zoom?`<div class="compact-group-bar"><span role="status">当前分屏${hiddenStatus?` · 其他分屏：${e(hiddenStatus)}`:''}</span><button class="sd-quiet" data-component-action="hidden-groups">其他分屏（${Object.keys(host.groups).length-1}）</button></div>${group(zoom)}`:render(host.center,center.clientWidth,center.clientHeight));
    for(const node of center.querySelectorAll<HTMLElement>(".view-group")){
      const list=node.querySelector<HTMLElement>(".component-tab-list")!;list.scrollLeft=scrolls.get(node.dataset.group)??0;
      if(this.boundTabs.has(node))continue;this.boundTabs.add(node);
      node.addEventListener("contextmenu",event=>{const target=event.target as Element,header=target.closest<HTMLElement>('.component-tabs');if(!header)return;event.preventDefault();const tab=target.closest<HTMLElement>("[data-view]"),anchor=tab?.querySelector<HTMLElement>('[role="tab"]')??header,reference={x:event.clientX,y:event.clientY,contextElement:anchor};if(tab)this.viewMenu(shell,tab.dataset.view!,anchor,false,reference);else this.groupMenu(shell,node.dataset.group!,anchor,false,reference);});
      node.addEventListener('keydown',event=>{if(event.key!=='ContextMenu'&&!(event.shiftKey&&event.key==='F10'))return;const target=event.target as Element,header=target.closest<HTMLElement>('.component-tabs');if(!header)return;event.preventDefault();event.stopPropagation();const tab=target.closest<HTMLElement>('[data-view]'),anchor=tab?.querySelector<HTMLElement>('[role="tab"]')??header;if(tab)this.viewMenu(shell,tab.dataset.view!,anchor,true);else this.groupMenu(shell,node.dataset.group!,anchor,true);});
      list.addEventListener("keydown",event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;event.preventDefault();const g=shell.ui.host!.groups[node.dataset.group!],at=g.viewIds.indexOf(g.activeViewId??'');const index=event.key==='Home'?0:event.key==='End'?g.viewIds.length-1:(at+(event.key==='ArrowRight'?1:-1)+g.viewIds.length)%g.viewIds.length;const id=g.viewIds[index];if(id)void this.run(async()=>{await this.perform(shell,{op:'select',viewId:id});const surface=shell.surfaces.get(id);if(surface)this.focusSurface(surface);});});
    }
    for(const handle of center.querySelectorAll<HTMLElement>("[data-split-ratio]"))if(!this.boundTabs.has(handle)){
      this.boundTabs.add(handle);handle.addEventListener('pointerdown',event=>this.splitResize(shell,handle,event));
      handle.addEventListener('dblclick',()=>void this.run(()=>this.perform(shell,{op:'splitRatio',splitId:handle.dataset.splitRatio,ratio:.5})));
      handle.addEventListener('keydown',event=>{if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home','End'].includes(event.key))return;event.preventDefault();const min=Number(handle.getAttribute('aria-valuemin'))/100,max=Number(handle.getAttribute('aria-valuemax'))/100;const ratio=event.key==='Home'?min:event.key==='End'?max:Number(handle.getAttribute('aria-valuenow'))/100+(['ArrowRight','ArrowDown'].includes(event.key)?1:-1)*(event.shiftKey?.1:.05);void this.run(()=>this.perform(shell,{op:'splitRatio',splitId:handle.dataset.splitRatio,ratio:Math.max(min,Math.min(max,ratio))}));});
    }
  }
  private splitResize(shell:Shell,handle:HTMLElement,event:PointerEvent) {
    if(event.button!==0||this.splitGesture)return;event.preventDefault();handle.setPointerCapture(event.pointerId);handle.focus();
    const node=handle.parentElement!,rect=node.getBoundingClientRect(),axis=node.dataset.axis,initial=Number(handle.getAttribute('aria-valuenow'))/100;let ratio=initial,finished=false;
    const move=(ev:PointerEvent)=>{if(ev.pointerId!==event.pointerId)return;const split=splitNode(shell.ui.host!.center,handle.dataset.splitRatio!);if(!split)return;ratio=effectiveRatio(split,rect.width,rect.height,axis==='x'?(ev.clientX-rect.left)/rect.width:(ev.clientY-rect.top)/rect.height);node.style.setProperty(axis==='x'?'grid-template-columns':'grid-template-rows',`minmax(0,${ratio}fr) ${splitWidth}px minmax(0,${1-ratio}fr)`);handle.setAttribute('aria-valuenow',String(Math.round(ratio*100)));this.scheduleLayout(shell.workspace.context.workspaceId);};
    const end=(commit:boolean)=>{if(finished)return;finished=true;handle.removeEventListener('pointermove',move);handle.removeEventListener('pointerup',up);handle.removeEventListener('pointercancel',cancel);handle.removeEventListener('lostpointercapture',cancel);if(handle.hasPointerCapture(event.pointerId))handle.releasePointerCapture(event.pointerId);
      const done=()=>{if(this.splitGesture?.shell===shell)this.splitGesture=undefined;this.paintCenter(shell);this.scheduleLayout(shell.workspace.context.workspaceId);};
      if(commit)void this.perform(shell,{op:'splitRatio',splitId:handle.dataset.splitRatio,ratio}).catch(error=>this.notify(String(error))).finally(done);else done();
    };
    const up=(ev:PointerEvent)=>{if(ev.pointerId===event.pointerId)end(true);},cancel=()=>end(false);
    this.splitGesture={shell,cancel};handle.addEventListener('pointermove',move);handle.addEventListener('pointerup',up);handle.addEventListener('pointercancel',cancel);handle.addEventListener('lostpointercapture',cancel);
  }
  private groupIds(node:CenterNode):string[] {return node.type==='group'?[node.groupId]:[...this.groupIds(node.first),...this.groupIds(node.second)];}
  private groupMenu(shell:Shell,id:string,anchor:HTMLElement,keyboard=false,reference:MenuReference=anchor) {
    this.cancelGestures();
    const host=shell.ui.host!,g=host.groups[id];if(!g)return;
    const order=this.groupIds(host.center),index=order.indexOf(id),target=order[index>0?index-1:index+1];
    const x=this.canSplit(shell,id,'x'),y=this.canSplit(shell,id,'y');
    const affected=[...g.viewIds,...(host.groups[target]?.viewIds??[])];
    const busy=()=>affected.some(id=>this.closeBlocked.has(id)||this.transfers.some(t=>t.viewIds.includes(id)))||[...this.pendingMoves.keys()].some(key=>key.startsWith(`${shell.workspace.context.workspaceId}:`));
    this.showMenu(anchor,[
      {label:'左右分屏',disabled:!x||busy(),action:()=>this.perform(shell,{op:'split',axis:'x',groupId:id,revision:host.revision})},
      {label:'上下分屏',disabled:!y||busy(),action:()=>this.perform(shell,{op:'split',axis:'y',groupId:id,revision:host.revision})},
      ...(target?[{label:'取消此分屏',disabled:busy(),separatorBefore:true,action:()=>this.perform(shell,{op:'mergeGroup',sourceGroupId:id,groupId:target,revision:host.revision})}]:[]),
    ],false,!x||!y?'窗口空间不足，扩大窗口后再分屏。':undefined,keyboard,reference,()=>shell.ui.host?.revision===host.revision&&!busy());
  }
  private returnEntry(shell:Shell,id:string):MenuEntry[] {
    const location=shell.ui.returnLocations?.[id],current=this.state?.window.id,wid=shell.workspace.context.workspaceId;
    const valid=(windowId:string)=>windowId!==current&&this.state?.windows?.some(w=>w.id===windowId&&w.ready&&w.workspaceIds.includes(wid));
    if(location&&valid(location.windowId))return [{label:'返回原窗口',action:()=>this.moveWindow(shell,{kind:'view',id},undefined,true)}];
    return valid('main')?[{label:'返回主窗口',action:()=>this.moveWindow(shell,{kind:'view',id},'main')}]:[];
  }
  private hiddenGroupsMenu(shell:Shell,anchor:HTMLElement,keyboard=false) {
    const host=shell.ui.host!;
    this.showMenu(anchor,this.groupIds(host.center).filter(id=>id!==host.focusedGroupId).map(id=>{
      const group=host.groups[id],views=group.viewIds.map(id=>shell.ui.views.find(v=>v.id===id)).filter(Boolean) as View[];
      return {label:(views.map(v=>v.title).join('、')||'空分屏')+(views.some(v=>v.dirty)?' · 未保存':'')+(views.some(v=>v.error)?' · 错误':''),action:async()=>{if(shell.ui.host?.zoomGroupId)await this.perform(shell,{op:'zoom',groupId:shell.ui.host.zoomGroupId});await this.perform(shell,{op:'focusGroup',groupId:id});}};
    }),false,undefined,keyboard);
  }
  private viewMenu(shell:Shell,id:string,anchor:HTMLElement,keyboard=false,reference:MenuReference=anchor) {
    this.cancelGestures();
    const host=shell.ui.host!,view=shell.ui.views.find(v=>v.id===id);if(!view)return;
    const group=Object.values(host.groups).find(g=>g.viewIds.includes(id));
    const tool=this.definitions[view.definitionKey]?.container==='sidebar';
    const canDetach=this.state?.window.id==='main'||this.state?.workspaces.length!==1||Object.keys(host.groups).length>1||host.left.viewIds.length>0||host.right.viewIds.length>0||(host.dialogs?.length??0)>0||Object.values(host.groups).some(g=>g.viewIds.some(v=>v!==id))||Boolean(shell.ui.unavailableTools?.length)||this.transfers.some(t=>t.workspaceId===shell.workspace.context.workspaceId);
    const x=this.canSplit(shell,group?.id??host.focusedGroupId,'x'),y=this.canSplit(shell,group?.id??host.focusedGroupId,'y');
    const busy=()=>this.closeBlocked.has(id)||this.transfers.some(t=>t.viewIds.includes(id))||[...this.pendingMoves.keys()].some(key=>key.startsWith(`${shell.workspace.context.workspaceId}:`));
    this.showMenu(anchor,[
      ...(tool?[{label:'停靠到侧栏',action:()=>this.perform(shell,{op:'dock',viewId:id,side:shell.ui.preferences[view.definitionKey]?.side??'right'})}]:[
        {label:'移到右侧分屏',disabled:!x,action:()=>this.perform(shell,{op:'split',axis:'x',groupId:group?.id,viewId:id})},
        {label:'移到下方分屏',disabled:!y,action:()=>this.perform(shell,{op:'split',axis:'y',groupId:group?.id,viewId:id})},
      ]),
      ...(canDetach?[{label:'移到新窗口',action:()=>this.moveWindow(shell,{kind:'view',id})}]:[]),
      ...this.returnEntry(shell,id),
      {label:tool?'隐藏工具':'关闭标签',separatorBefore:true,action:()=>tool?this.perform(shell,{op:'hideTool',viewId:id}):this.close([shell.workspace.context.workspaceId],id)},
    ].map(entry=>({...entry,disabled:entry.disabled||busy()})),false,!tool&&(!x||!y)?'窗口空间不足，扩大窗口后再分屏。':undefined,keyboard,reference,()=>shell.ui.host?.revision===host.revision&&shell.ui.views.some(v=>v.id===id)&&!busy());
  }
  private async confirmMove(requestId:string,operation:()=>Promise<unknown>) {
    try { await operation(); }
    catch(error) {
      while(!this.disposed) {
        const receipt=await this.invoke<{status:string;error?:string}>('moveStatus',{id:requestId}).catch(()=>null);
        if(receipt?.status==='completed')break;
        if(receipt?.status!=='pending')throw new Error(receipt?.error||String(error));
        await new Promise(resolve=>setTimeout(resolve,300));
      }
    }
    if(!this.disposed)await this.refresh();
  }
  private moveWindow(shell:Shell,scope:{kind:'view'|'group'|'host';id?:string},target?:string,returnToSource=false) {
    const key=`${shell.workspace.context.workspaceId}:${scope.kind}:${scope.id??''}`;
    const pending=this.pendingMoves.get(key);if(pending)return pending;
    const requestId=crypto.randomUUID();
    const job=this.confirmMove(requestId,()=>this.call('workbench_move',{id:shell.workspace.context.workspaceId,context:shell.workspace.context,requestId,scope,target:target??null,destination:returnToSource?{returnToSource:true}:null})).finally(()=>this.pendingMoves.delete(key));
    this.pendingMoves.set(key,job);return job;
  }
  moveHost(id:string,target?:string) {const shell=this.shells.get(id);if(!shell)return Promise.reject(new Error('工作台已关闭'));return this.moveWindow(shell,{kind:'host'},target);}
  returnWindow() {const existing=this.pendingMoves.get('@window');if(existing)return existing;const requestId=crypto.randomUUID(),job=this.confirmMove(requestId,()=>this.call('workbench_return_window',{requestId})).finally(()=>this.pendingMoves.delete('@window'));this.pendingMoves.set('@window',job);return job;}
  restoreLayout(id:string) {const shell=this.shells.get(id);if(!shell)return Promise.reject(new Error('工作台已关闭'));return this.call('workbench_restore_layout',{context:shell.workspace.context});}
  private syncTransfers() {
    const previous=this.movingViews;
    this.movingViews=new Set(this.transfers.filter(t=>t.source===this.state?.window.id&&t.phase!=='preparing').flatMap(t=>t.viewIds));
    for(const shell of this.shells.values())for(const surface of shell.surfaces.values()){surface.node.inert=this.movingViews.has(surface.view.id)||Boolean(surface.readOnly)||this.closeBlocked.has(surface.view.id);if(previous.has(surface.view.id)&&!this.movingViews.has(surface.view.id)){this.post(surface,{type:'transferCancelled'});this.resumeRequests(surface);}}
    for(const transfer of this.transfers)if(transfer.source===this.state?.window.id&&transfer.phase==='draining'&&!transfer.closeOwner&&!this.transferJobs.has(transfer.id)){
      this.transferJobs.add(transfer.id);void this.flush(transfer.viewIds,true).then(()=>{if(!this.disposed&&this.transfers.some(t=>t.id===transfer.id&&t.phase==='draining'))return this.invoke('transferDrained',{id:transfer.id});}).catch(error=>{if(this.transfers.some(t=>t.id===transfer.id&&t.phase==='draining')){this.notify(String(error));return this.call('workspace_cancel_move',{id:transfer.workspaceId});}}).finally(()=>{void this.refresh();});
    }
    const live=new Set(this.transfers.map(t=>t.id));for(const id of this.transferJobs)if(!live.has(id))this.transferJobs.delete(id);
  }
  dropZones() {
    return [...this.shells.values()].filter(s=>!s.node.hidden&&!s.node.classList.contains('incoming-component-workspace')).flatMap(shell=>[...shell.node.querySelectorAll<HTMLElement>('.view-group')].map(group=>{
      const r=group.getBoundingClientRect(),rails=['left','right'].reduce((width,side)=>width+shell.node.querySelector(`.${side}-rail`)!.getBoundingClientRect().width,0);return {workspaceId:shell.workspace.context.workspaceId,groupId:group.dataset.group!,x:r.x,y:r.y,width:r.width,height:r.height,workspaceWidth:shell.node.clientWidth-rails,workspaceHeight:shell.node.querySelector<HTMLElement>('.workbench-center')!.clientHeight,tabs:[...group.querySelectorAll<HTMLElement>('[data-view]')].map(tab=>{const r=tab.getBoundingClientRect();return{id:tab.dataset.view!,center:r.x+r.width/2};})};
    }));
  }
  dockZones() {
    return [...this.shells.values()].filter(s=>!s.node.hidden&&!s.node.classList.contains('incoming-component-workspace')).flatMap(shell=>['left','right'].map(side=>{
      const rail=shell.node.querySelector(`.${side}-rail`)!.getBoundingClientRect(),panel=shell.node.querySelector(`.${side}-panel`)!.getBoundingClientRect();
      return {workspaceId:shell.workspace.context.workspaceId,side,x:Math.min(rail.x,panel.width?panel.x:rail.x),y:rail.y,width:rail.width+panel.width,height:rail.height,tools:rail.width?[...shell.node.querySelectorAll<HTMLElement>(`.${side}-rail [data-tool-key]`)].map(button=>({key:button.dataset.toolKey!,center:button.getBoundingClientRect().top+button.offsetHeight/2})):[]};
    }));
  }
  previewDrop(target:DropTarget) {
    for(const shell of this.shells.values())this.previewTool(shell);
    for(const node of this.content.querySelectorAll<HTMLElement>('[data-drop-edge]'))delete node.dataset.dropEdge;
    for(const node of this.content.querySelectorAll<HTMLElement>('[data-dock-preview]'))delete node.dataset.dockPreview;
    if(target.kind==='group'&&target.windowId===this.state?.window.id)this.content.querySelector<HTMLElement>(`.view-group[data-group="${CSS.escape(target.groupId)}"]`)?.setAttribute('data-drop-edge',target.edge);
    if(target.kind==='dock'&&target.windowId===this.state?.window.id){const shell=this.shells.get(target.workspaceId);shell?.node.querySelector(`.${target.side}-rail`)?.setAttribute('data-dock-preview','true');shell?.node.querySelector(`.${target.side}-panel`)?.setAttribute('data-dock-preview','true');}
    if(target.kind==='dock'&&target.windowId===this.state?.window.id){const shell=this.shells.get(target.workspaceId);if(shell)this.previewTool(shell,{side:target.side,beforeKey:target.beforeToolKey??null});}
  }
  private dragDown=(event:PointerEvent)=>{
    if(event.button!==0||!event.isPrimary||this.drag)return;
    const element=event.target as Element,button=element.closest<HTMLElement>('[data-tool-key]'),tab=element.closest<HTMLElement>('.component-tab'),group=element.closest<HTMLElement>('.view-group'),panel=element.closest<HTMLElement>('.workbench-panel header');
    if(tab&&element.closest('[data-component-action="close"]'))return;
    if(!tab&&!button&&element.closest('button'))return;
    if(!button&&!tab&&!panel&&(!group||!element.closest('.component-tabs')||element.closest('[data-component-action="create"]')))return;
    const shell=this.shells.get(element.closest<HTMLElement>('.server-workbench')?.dataset.id??'');if(!shell?.ui.host||shell.node.inert||this.transfers.some(t=>t.viewIds.includes(tab?.dataset.view??'')))return;
    if(button&&(button.matches(':disabled')||this.pendingMoves.has(`${shell.workspace.context.workspaceId}:tool:${button.dataset.toolKey}`)))return;
    const id=tab?.dataset.view||(panel?shell.ui.views.find(v=>v.id===(panel.closest('.left-panel')?shell.ui.left:shell.ui.right))?.id:undefined);if(panel&&!id)return;
    const trigger=button??tab?.querySelector<HTMLElement>('[role="tab"]')??panel??group!,rect=trigger.getBoundingClientRect();
    this.drag={shell,scope:button?{kind:'tool',id:button.dataset.toolKey!}:id?{kind:'view',id}:{kind:'group',id:group!.dataset.group!},handle:this.content,trigger,revision:shell.ui.host.revision,hostId:shell.ui.host.id,context:JSON.stringify(shell.workspace.context),instance:this.definitions[button?.dataset.toolKey??shell.ui.views.find(v=>v.id===id)?.definitionKey??'']?.pluginInstanceId,focusEpoch:this.focusEpoch,pointer:event.pointerId,x:event.clientX,y:event.clientY,localPoint:{x:event.clientX,y:event.clientY},offsetX:event.clientX-rect.left,offsetY:event.clientY-rect.top,finished:false,sampling:false};
  };
  private dragMove=(event:PointerEvent)=>{
    const g=this.drag;if(!g||g.pointer!==event.pointerId||g.finished)return;
    g.localPoint={x:event.clientX,y:event.clientY};
    if(!g.starting&&Math.hypot(event.clientX-g.x,event.clientY-g.y)>=6){
      event.preventDefault();g.handle.setPointerCapture(g.pointer);document.body.classList.add('workbench-dragging');window.getSelection()?.removeAllRanges();this.closeMenu();
      const ghost=document.createElement('div');ghost.className='workbench-drag-ghost';ghost.textContent=g.scope.kind==='tool'?this.definitions[g.scope.id]?.title??'工具':g.scope.kind==='view'?(g.shell.ui.views.find(v=>v.id===g.scope.id)?.title??'页面'):'标签组';document.body.append(ghost);g.ghost=ghost;
      g.starting=g.scope.kind==='tool'?Promise.resolve():this.call<string>('workspace_drag_start',{id:g.shell.workspace.context.workspaceId,scope:g.scope,offsetX:g.offsetX,offsetY:g.offsetY}).then(token=>{g.token=token;}).catch(error=>{this.notify(String(error));g.finished=true;this.clearDrag(g);});
    }
    if(g.ghost){event.preventDefault();g.ghost.style.left=`${event.clientX+14}px`;g.ghost.style.top=`${event.clientY+14}px`;}
    if(g.scope.kind==='tool'&&g.starting){
      g.toolTarget=this.toolDrop(g.shell,event.clientX,event.clientY,g.scope.id);
      this.previewTool(g.shell,g.toolTarget);return;
    }
    if(g.token&&!g.sampling){g.sampling=true;void this.call<DropTarget>('workspace_drag_sample',{token:g.token,localPoint:g.localPoint}).then(target=>this.previewDrop(target)).catch(error=>{this.notify(String(error));void this.finishDrag(true);}).finally(()=>{g.sampling=false;});}
  };
  private dragUp=(event:PointerEvent)=>{if(event.pointerId===this.drag?.pointer)void this.finishDrag(false);setTimeout(()=>{this.suppressClickUntil=0;},0);};
  private dragLost=(event:PointerEvent)=>{if(event.pointerId===this.drag?.pointer&&!this.drag.finished)void this.finishDrag(true);};
  private cancelGestures=()=>{this.closeMenu();this.splitGesture?.cancel();void this.finishDrag(true);};
  private toolMenuEvent=(event:MouseEvent|KeyboardEvent)=>{
    if(event instanceof KeyboardEvent && event.key!=='ContextMenu' && !(event.shiftKey&&event.key==='F10'))return;
    const button=(event.target as Element).closest<HTMLElement>('[data-tool-key]');
    const shell=this.shells.get(button?.closest<HTMLElement>('.server-workbench')?.dataset.id??'');
    if(!button||!shell)return;event.preventDefault();this.toolMenu(shell,button.dataset.toolKey!,button,event instanceof KeyboardEvent);
  };
  private dragCancel=(event:PointerEvent)=>{if(event.pointerId===this.drag?.pointer)void this.finishDrag(true);};
  private async finishDrag(cancel:boolean) {
    const g=this.drag;if(!g||g.finished)return;g.finished=true;
    if(g.starting)this.suppressClickUntil=Date.now()+350;
    if(g.scope.kind==='tool') {
      const target=g.toolTarget,valid=g.shell.ui.host?.revision===g.revision&&JSON.stringify(g.shell.workspace.context)===g.context&&this.state?.window.activeWorkspace===g.shell.workspace.context.workspaceId;
      this.clearDrag(g);
      if(!cancel&&g.starting&&target&&valid)try{await this.moveTool(g.shell,g.scope.id,target.side,target.beforeKey,g.revision);}catch(error){this.notify(String(error));}
      else if(g.starting&&valid)g.trigger.focus({preventScroll:true});
      return;
    } else {
      await g.starting;
      if(g.token){try{await this.confirmMove(g.token,()=>this.call('workspace_drag_finish',{token:g.token,cancel,localPoint:g.localPoint}));}catch(error){this.notify(String(error));}}
    }
    this.clearDrag(g);if(!this.disposed)await this.refresh();
  }
  private clearDrag(g:NonNullable<Components['drag']>) {g.ghost?.remove();if(this.drag!==g)return;this.drag=undefined;if(g.handle.hasPointerCapture(g.pointer))g.handle.releasePointerCapture(g.pointer);document.body.classList.remove('workbench-dragging');this.previewTool(g.shell);this.previewDrop({kind:'invalid'});}
  private toolDrop(shell:Shell,x:number,y:number,key:string) {
    for(const side of ['left','right']) {
      const rail=shell.node.querySelector<HTMLElement>(`.${side}-rail`)!,r=rail.getBoundingClientRect();
      if(x<r.left||x>r.right||y<r.top||y>r.bottom)continue;
      const before=[...rail.querySelectorAll<HTMLElement>('[data-tool-key]')].find(button=>button.dataset.toolKey!==key&&y<button.getBoundingClientRect().top+button.offsetHeight/2);
      return {side,beforeKey:before?.dataset.toolKey??null};
    }
  }
  private previewTool(shell:Shell,target?:{side:string;beforeKey:string|null}) {
    for(const rail of shell.node.querySelectorAll<HTMLElement>('.workbench-rail')){delete rail.dataset.toolDrop;rail.querySelector('.tool-insertion')?.remove();}
    if(!target)return;const rail=shell.node.querySelector<HTMLElement>(`.${target.side}-rail`)!;rail.dataset.toolDrop='true';
    const before=target.beforeKey?rail.querySelector<HTMLElement>(`[data-tool-key="${CSS.escape(target.beforeKey)}"]`):undefined;
    const line=document.createElement('span');line.className='tool-insertion';line.setAttribute('aria-hidden','true');
    line.style.top=`${before?before.offsetTop:Math.max(4,...[...rail.querySelectorAll<HTMLElement>('[data-tool-key]')].map(b=>b.offsetTop+b.offsetHeight+3))}px`;rail.append(line);
  }
  private layoutKey=(event:KeyboardEvent)=>{
    if(event.key==='Escape'&&this.splitGesture){event.preventDefault();this.splitGesture.cancel();return;}
    if(event.key==='Escape'&&this.drag){event.preventDefault();void this.finishDrag(true);return;}
    if(!(event.ctrlKey||event.metaKey)||!event.altKey)return;const shell=this.shells.get(this.state?.window.activeWorkspace??'');if(!shell?.ui.host)return;
    if(['h','v'].includes(event.key.toLowerCase())){event.preventDefault();void this.run(()=>this.perform(shell,{op:'split',groupId:shell.ui.host!.focusedGroupId,axis:event.key.toLowerCase()==='h'?'x':'y'}));}
    if(event.key.toLowerCase()==='z'){event.preventDefault();void this.run(()=>this.perform(shell,{op:'zoom',groupId:shell.ui.host!.focusedGroupId}));}
  };
  private geometry(shell: Shell, widths = this.widths(shell.workspace)) {
    const rails = ["left", "right"].reduce((sum, side) => sum + shell.node.querySelector(`.${side}-rail`)!.getBoundingClientRect().width, 0);
    const a = shell.ui.views.find(v => v.id === shell.ui.left), b = shell.ui.views.find(v => v.id === shell.ui.right);
    const minimum=shell.ui.host&&!shell.ui.host.zoomGroupId?centerMinimum(shell.ui.host.center).width:320;
    return panelGeometry(shell.node.getBoundingClientRect().width - rails - Math.max(320,minimum), widths, Boolean(shell.ui.left), Boolean(shell.ui.right), (a?.lastUsed ?? 0) >= (b?.lastUsed ?? 0) ? "left" : "right");
  }
  panelWidth(id: string, side: PanelSide, width?: number, otherWidth?: number) {
    const shell = this.shells.get(id); if (!shell || shell.node.hidden || shell.node.inert) return;
    const widths = this.widths(shell.workspace);
    const geometry = this.geometry(shell, width === undefined ? widths : { ...widths, [`${side}Width`]: width, drag: { side, otherWidth: otherWidth ?? shell.node.querySelector(`.${side === "left" ? "right" : "left"}-panel`)!.getBoundingClientRect().width } });
    return geometry[side] ? geometry[`${side}Width`] : undefined;
  }
  scheduleLayout(id: string) {
    const shell = this.shells.get(id); if (!shell || shell.layoutFrame !== undefined || this.disposed) return;
    shell.layoutFrame = requestAnimationFrame(() => { shell.layoutFrame = undefined; if (!this.disposed && this.shells.get(id) === shell) this.position(shell); });
  }
  private position(shell: Shell) {
    const r = this.surfaceLayer.getBoundingClientRect(), w = shell.ui;
    const center=shell.node.querySelector<HTMLElement>(".workbench-center")!;
    if (r.height > 0) { const capacity = Math.max(1, Math.floor((r.height - 80) / 38)); if (shell.railCapacity !== capacity) { shell.railCapacity = capacity; queueMicrotask(() => this.paint(shell)); return; } }
    let widths = this.widths(shell.workspace), geometry = this.geometry(shell, widths);
    if (widths.drag && (!geometry[widths.drag.side] || shell.node.hidden || shell.node.inert)) { this.cancelResize(); widths = this.widths(shell.workspace); geometry = this.geometry(shell, widths); }
    const { left, right } = geometry;
    shell.node.classList.toggle("left-open", left); shell.node.classList.toggle("right-open", right);
    for (const side of ["left", "right"] as const) {
      for (const [name, value] of [[`--${side}-width`, widths[`${side}Width`]], [`--effective-${side}-width`, geometry[`${side}Width`]]] as const) {
        if (shell.node.style.getPropertyValue(name) !== `${value}px`) shell.node.style.setProperty(name, `${value}px`);
      }
      const handle = shell.node.querySelector(`[data-resize="${side}"]`)!;
      handle.setAttribute("aria-valuemin", "160"); handle.setAttribute("aria-valuemax", "480"); handle.setAttribute("aria-valuenow", String(geometry[`${side}Width`]));
    }
    const centerSize=`${center.clientWidth}:${center.clientHeight}`;
    if(shell.node.dataset.centerSize!==centerSize){shell.node.dataset.centerSize=centerSize;this.paintCenter(shell);}
    const placements = new Map<Surface, DOMRect>();
    for (const surface of shell.surfaces.values()) {
      if(surface.mounted && !surface.readOnly && !surface.frame.dataset.stateReady && this.state?.workspaces.some(w=>w.context.workspaceId===surface.view.context.workspaceId)) {
        surface.frame.dataset.stateReady='true';this.post(surface,{type:'surfaceReady'});
      }
      const id = surface.view.id;
      const group=Object.values(w.host?.groups??{}).find(g=>g.activeViewId===id);
      const slot = group ? shell.node.querySelector(`.view-group[data-group="${CSS.escape(group.id)}"] .central-slot`) : id === w.left && left ? shell.node.querySelector(".left-panel .component-slot") : id === w.right && right ? shell.node.querySelector(".right-panel .component-slot") : null;
      if (slot) placements.set(surface, slot.getBoundingClientRect());
    }
    for (const surface of shell.surfaces.values()) {
      if(this.definitions[surface.view.definitionKey]?.container === "dialog") {
        const modal=surface.node.querySelector<HTMLDialogElement>("dialog")!;
        const visible=!shell.node.hidden&&!shell.node.classList.contains("incoming-component-workspace");
        surface.node.hidden=false;
        if(visible&&!modal.open)modal.showModal();else if(!visible&&modal.open)modal.close();
        if(surface.visible!==visible){surface.visible=visible;this.post(surface,{type:"visibilityChanged",visible});}
        if (visible && surface.focusPending === this.focusEpoch) this.focusSurface(surface);
        continue;
      }
      const s = placements.get(surface);
      const visible = Boolean(s && !shell.node.hidden && !shell.node.classList.contains("incoming-component-workspace"));
      surface.node.hidden = !s||shell.node.hidden;
      surface.node.style.visibility=shell.node.classList.contains("incoming-component-workspace")?"hidden":"";
      if (s) for (const [key, value] of Object.entries({ left: `${s.left-r.left}px`, top: `${s.top-r.top}px`, width: `${s.width}px`, height: `${s.height}px` })) { if (surface.node.style.getPropertyValue(key) !== value) surface.node.style.setProperty(key, value); }
      if (surface.visible !== visible) { if(visible)void this.refreshSettings(surface); if(!visible&&surface.token)this.clipboard.discard(surface.token); surface.visible = visible; this.post(surface, { type: "visibilityChanged", visible }); }
      if (visible && surface.focusPending === this.focusEpoch) this.focusSurface(surface);
    }
  }
  private tokens() { return themeTokens(); }
  private post(surface: Surface, message: Record<string, unknown>) { surface.frame.contentWindow?.postMessage({ channel: this.definitions[surface.view.definitionKey]?.uiApiVersion==="2"?"sodeck-ui-v2":"sodeck-ui-v1", surfaceId: surface.token, ...message }, new URL(surface.frame.src).origin === "null" ? "*" : new URL(surface.frame.src).origin); }
  private broadcast() { for (const s of this.shells.values()) for (const f of s.surfaces.values()) this.post(f, { type: "themeChanged", tokens: this.tokens() }); }
  private systemTheme = () => this.broadcast();
  private async refreshSettings(surface: Surface) {
    const token = surface.token; if (!token || this.disposed) return;
    try { const settings = await this.call("settings_for_surface", {surfaceId: token}); if(surface.token===token && !this.disposed && settings) this.post(surface,{type:"settingsChanged",settings}); }
    catch { /* Older plugins and cores continue with their defaults. */ }
  }
  private outside = (event: PointerEvent) => { if (this.menu && !this.menu.contains(event.target as Node) && !this.menuAnchor?.contains(event.target as Node)) this.closeMenu(); };
  private message = (event: MessageEvent) => {
    if (!event.data || !["sodeck-ui-v1","sodeck-ui-v2"].includes(event.data.channel)) return;
    const surface = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]).find(s => s.frame.contentWindow === event.source);
    if (!surface || event.data.channel!==(this.definitions[surface.view.definitionKey]?.uiApiVersion==="2"?"sodeck-ui-v2":"sodeck-ui-v1")) return;
    const expectedOrigin = new URL(surface.frame.src).origin;
    if (expectedOrigin !== "null" && event.origin !== expectedOrigin) return;
    const message = event.data;
    if (message.type === "ready") {
      if (surface.initialized) return; surface.initialized = true;
      surface.chain = this.invoke<{ surfaceId: string; data: unknown; view: View; readOnly?:boolean; capabilities?:{workbench?:number;sharedState?:number;fileLocation?:number;fileLocationProof?:number;uiOpenView?:number} }>("attach", { viewId: surface.view.id }).then(async result => {
        surface.token = result.surfaceId; surface.readOnly=result.readOnly;
        this.shared.attach(result.surfaceId,result.view.context,message=>this.post(surface,message));
        const settings = await this.call("settings_for_surface", {surfaceId: result.surfaceId}).catch(() => null);
        this.post(surface, { type: "mount", readOnly:result.readOnly, connected: this.shells.get(surface.view.context.workspaceId)?.workspace.connected, context: result.view.context, serverName: this.shells.get(surface.view.context.workspaceId)?.workspace.name, viewId: surface.view.id, viewType: surface.view.definitionKey.split(":")[1], data: result.data, tokens: this.tokens(), settings, capabilities: {workbench:result.capabilities?.workbench,clipboard:1,localFiles:1,settings:1,sharedState:result.capabilities?.sharedState,fileLocation:result.capabilities?.fileLocation,fileLocationProof:result.capabilities?.fileLocationProof,uiOpenView:result.capabilities?.uiOpenView}, visible: surface.visible });
        surface.frame.dataset.surfaceId = result.surfaceId;
      }).catch(error => this.surfaceError(surface, String(error)));
      return;
    }
    if (!surface.token || message.surfaceId !== surface.token) return;
    if (message.type === "interaction") { if (surface.visible && !surface.readOnly && !this.movingViews.has(surface.view.id)) { void this.invoke("action",{context:surface.view.context,action:{op:"focus",viewId:surface.view.id}}).catch(()=>{}); this.interaction(); this.closeMenu(); this.content.dispatchEvent(new Event("surface-interaction")); } return; }
    if ((message.type === "mounted"||message.type === "prepared")) { surface.flushable = message.flush === true; surface.acceptsLocalFiles = message.localFiles === true; clearTimeout(surface.timeout); void this.invoke("ready", { surfaceId: surface.token }).then(async () => { surface.mounted = true; this.scheduleLayout(surface.view.context.workspaceId); await this.refreshSettings(surface); await this.refresh(); if (surface.focusPending === this.focusEpoch && surface.visible) this.focusSurface(surface); }).catch(error => this.surfaceError(surface, String(error))); return; }
    if (message.type === "flushed" && surface.flushing && surface.flushing.id === message.id) { surface.flushing.done(message.error); return; }
    if (message.type !== "request" || typeof message.id !== "string") return;
    const requestId = message.id;
    if(message.method === 'state'){
      const token=surface.token,context=JSON.stringify(surface.view.context);
      void this.shared.request(token,message.params??{}).then(data=>{if(surface.token===token && JSON.stringify(surface.view.context)===context)this.post(surface,{type:'response',id:requestId,data});})
        .catch(error=>{if(surface.token===token)this.post(surface,{type:'response',id:requestId,error:String(error)});});
      return;
    }
    if((this.closeBlocked.has(surface.view.id)||this.movingViews.has(surface.view.id))&&!surface.flushing) {
      if(surface.deferred.size>=64&&!surface.deferred.has(requestId))this.post(surface,{type:"response",id:requestId,error:"组件等待请求已满，请在关闭决定后重试"});
      else surface.deferred.set(requestId,event);
      return;
    }
    if (message.method === "settings") {
      const params = message.params ?? {};
      const command = params.op === "get" ? "settings_for_surface" : params.op === "applied" ? "settings_applied" : params.op === "save" ? "save_settings_for_surface" : "";
      const {op:_,...input}=params;
      void (command ? this.call(command, {surfaceId:surface.token,input}) : Promise.reject(new Error("未知设置操作")))
        .then(data=>this.post(surface,{type:"response",id:requestId,data}))
        .catch(error=>this.post(surface,{type:"response",id:requestId,error:String(error)}));
      return;
    }
    if (message.method === "clipboard") {
      const token=surface.token,context=JSON.stringify(surface.view.context),epoch=this.focusEpoch;
      const valid=()=>!this.disposed&&surface.token===token&&surface.visible&&document.hasFocus()&&document.activeElement===surface.frame&&JSON.stringify(surface.view.context)===context&&this.focusEpoch===epoch;
      void this.clipboard.request(token,message.params??{},valid)
        .then(data=>this.post(surface,{type:"response",id:requestId,data}))
        .catch(error=>this.post(surface,{type:"response",id:requestId,error:String(error)}));
      return;
    }
    if(message.method === "closeView") {
      void surface.chain.then(()=>this.close([surface.view.context.workspaceId],surface.view.id))
        .then(data=>this.post(surface,{type:"response",id:requestId,data}))
        .catch(error=>this.post(surface,{type:"response",id:requestId,error:String(error)}));
      return;
    }
    if(message.method === "closeViews") {
      const ticket = crypto.randomUUID(), ids = message.params?.viewIds;
      this.post(surface, {type:"response",id:requestId,data:{ticket}});
      void (async()=>{
        if(!Array.isArray(ids) || ids.length>64)throw new Error("关闭页面集合无效");
        const shell=this.shells.get(surface.view.context.workspaceId);
        for(const id of ids) {
          const view=shell?.ui.views.find(v=>v.id===id);
          if(!view || view.definitionKey.split(':')[0]!==surface.view.definitionKey.split(':')[0] || id===surface.view.id)throw new Error("页面不属于当前插件工作区");
        }
        return !ids.length||this.requestClose({kind:"views",ids});
      })().then(accepted=>this.post(surface,{type:"viewsClosed",ticket,accepted})).catch(error=>this.post(surface,{type:"viewsClosed",ticket,error:String(error)}));
      return;
    }
    if (message.method === "data") {
      void this.data.request(surface.token, message.params, data => this.post(surface, {type: "data", data}))
        .then(data => this.post(surface, {type: "response", id: requestId, data}))
        .catch(error => this.post(surface, {type: "response", id: requestId, error: String(error)}));
      return;
    }
    const requestFocusEpoch = this.focusEpoch;
    const task = async () => {
      try {
        let data: unknown;
        const shell = this.shells.get(surface.view.context.workspaceId)!;
        if (message.method === "openView") { const key = `${this.definitions[surface.view.definitionKey].pluginId}:${message.params.viewType}`; data = await this.perform(shell, { op: "open", key, params: message.params.params, resourceKey: message.params.resourceKey,sourceViewId:surface.view.id,groupId:shell.ui.host?.focusedGroupId }); if (this.focusEpoch === requestFocusEpoch && this.state?.window.activeWorkspace === shell.workspace.context.workspaceId) this.focus(shell,key); }
        else if (message.method === "event") { data = await this.invoke("event", { surfaceId: surface.token, event: message.params }); await this.refresh(); const viewId=(data as {viewId?:string})?.viewId; if(viewId && this.focusEpoch===requestFocusEpoch && this.state?.window.activeWorkspace===shell.workspace.context.workspaceId){const target=shell.surfaces.get(viewId);if(target)this.focusSurface(target);} }
        else throw new Error("未知插件 UI bridge 方法");
        this.post(surface, { type: "response", id: requestId, data });
      } catch (error) { this.post(surface, { type: "response", id: requestId, error: String(error) }); }
    };
    surface.chain = surface.chain.then(task, task);
  };
  private surfaceError(s: Surface, message: string) {
    clearTimeout(s.timeout);
    let error = s.node.querySelector<HTMLElement>(".component-error");
    if (!error) { error = document.createElement("div"); error.className = "component-error"; error.setAttribute("role", "alert"); s.node.append(error); }
    const definition = this.definitions[s.view.definitionKey];
    error.innerHTML = `<p>${e(message)}</p>${definition ? `<button data-action="locate-plugin" data-id="${e(definition.pluginId)}">查看插件</button><button data-action="plugin-logs" data-id="${e(definition.pluginId)}">查看日志</button>` : '<button data-action="manage-plugins">打开插件管理</button>'}`;
    if(definition?.available){const retry=document.createElement('button');retry.textContent='重试显示';retry.addEventListener('click',()=>{if(this.closeBlocked.has(s.view.id)||s.readOnly)return;const shell=this.shells.get(s.view.context.workspaceId);if(!shell||shell.surfaces.get(s.view.id)!==s)return;this.removeSurface(s);shell.surfaces.delete(s.view.id);this.paint(shell);});error.prepend(retry);}
  }
  private async perform(shell: Shell, action: Record<string, unknown>) {
    if(action.op==='split') {
      if(!this.canSplit(shell,String(action.groupId??shell.ui.host!.focusedGroupId),action.axis as 'x'|'y'))throw new Error('窗口空间不足，请扩大窗口后再拆分');
    }
    const focusEpoch=this.focusEpoch;
    const result = await this.invoke<{viewId?:string;windowId?:string}>("action", { context: shell.workspace.context, action: { requestId: crypto.randomUUID(), ...action } });
    await this.refresh();
    if(result?.viewId&&result.windowId&&result.windowId!==this.state?.window.id&&this.focusEpoch===focusEpoch){await this.call("workbench_locate_view",{context:shell.workspace.context,viewId:result.viewId});}
    return result;
  }
  private sideTools(shell:Shell,side:string) {return (shell.ui.tools??[]).filter(t=>t.location?.hostId===shell.ui.host?.id&&t.location?.side===side).sort((a,b)=>(a.location?.order??0)-(b.location?.order??0));}
  private moveTool(shell:Shell,key:string,side:string,beforeKey:string|null=null,revision=shell.ui.host!.revision) {
    const pendingKey=`${shell.workspace.context.workspaceId}:tool:${key}`,pending=this.pendingMoves.get(pendingKey);if(pending)return pending;
    const requestId=crypto.randomUUID(),focusEpoch=this.focusEpoch,entry=shell.ui.tools?.find(t=>t.key===key);
    const job=(async()=>{
      let result:ToolResult;
      try{result=await this.invoke<ToolResult>('action',{context:shell.workspace.context,action:{op:'moveTool',requestId,key,side,beforeKey,revision}});}
      catch(error){const receipt=await this.invoke<ToolResult>('moveStatus',{id:requestId}).catch(()=>null);if(!receipt)throw error;result=receipt;}
      await this.refresh();
      if(result.status!=='completed')throw new Error(result.error??'工具位置已改变，请重新操作');
      if(result.persistence==='failed'&&this.layoutSaveFailed)this.notify('布局已调整，但未能保存，重启后可能恢复原位置。可点击状态栏重试保存。');
      if(this.focusEpoch===focusEpoch&&this.state?.window.activeWorkspace===shell.workspace.context.workspaceId){
        const surface=entry?.active&&entry.viewId?shell.surfaces.get(entry.viewId):undefined;
        if(surface)this.focusSurface(surface);else shell.node.querySelector<HTMLElement>(`[data-tool-key="${CSS.escape(key)}"]`)?.focus({preventScroll:true});
      }
    })().finally(()=>this.pendingMoves.delete(pendingKey));
    this.pendingMoves.set(pendingKey,job);return job;
  }
  private toolMenu(shell:Shell,key:string,anchor:HTMLElement,keyboard=false) {
    this.cancelGestures();
    const entry=shell.ui.tools?.find(t=>t.key===key),side=entry?.location?.side,host=shell.ui.host;if(!entry||!side||!host)return;
    const keys=this.sideTools(shell,side).map(t=>t.key),index=keys.indexOf(key),revision=host.revision;
    this.showMenu(anchor,[
      {label:side==='left'?'移到右侧':'移到左侧',action:()=>this.moveTool(shell,key,side==='left'?'right':'left',null,revision)},
      {label:'上移',disabled:index<=0,action:()=>this.moveTool(shell,key,side,keys[index-1],revision)},
      {label:'下移',disabled:index<0||index>=keys.length-1,action:()=>this.moveTool(shell,key,side,keys[index+2]??null,revision)},
      {label:'入口置顶',disabled:index===0,action:()=>this.moveTool(shell,key,side,keys[0],revision)},
      {label:entry.pinned?'取消固定':'固定入口',separatorBefore:true,action:()=>this.perform(shell,{op:'preference',key,pinned:!entry.pinned})},
    ],false,undefined,keyboard,anchor,()=>shell.ui.host?.revision===revision&&!this.pendingMoves.has(`${shell.workspace.context.workspaceId}:tool:${key}`));
  }
  private canSplit(shell:Shell,id:string,axis:'x'|'y') {
    const minimum=centerMinimum(withSplit(shell.ui.host!.center,id,axis));
    const rails=['left','right'].reduce((width,side)=>width+shell.node.querySelector(`.${side}-rail`)!.getBoundingClientRect().width,0),center=shell.node.querySelector<HTMLElement>('.workbench-center')!;
    return minimum.width<=shell.node.clientWidth-rails&&minimum.height<=center.clientHeight;
  }
  private async action(id: string, action: string, key: string, anchor: HTMLElement, keyboard = false) {
    const shell = this.shells.get(id);
    if (!shell) return;
    const focusEpoch = this.focusEpoch;
    const groupId=anchor.closest<HTMLElement>("[data-group]")?.dataset.group;
    if(action==="hidden-groups"){this.hiddenGroupsMenu(shell,anchor,keyboard);return;}
    if(action==='retry-layout-save'){const result=await this.invoke<{persistence:string}>('retryLayoutSave');if(result.persistence==='saved'){this.layoutSaveFailed=false;this.notify('工作台布局已保存');await this.refresh();}return;}
    if (["toggle", "locate"].includes(action) && !this.definitions[key]?.available) { this.notify("组件不可用，请查看应用状态"); return; }
    if (["toggle", "locate", "collapse"].includes(action)) { await this.perform(shell, { op: action, key, side: key }); if (this.focusEpoch === focusEpoch && this.state?.window.activeWorkspace === id) this.focus(shell, action === "collapse" ? undefined : key); return; }
    if (action === "select") { if (!shell.ui.views.some(v => v.id === key)) return; await this.perform(shell, { op: "select", viewId: key }); const surface = shell.surfaces.get(key); if (surface && this.focusEpoch === focusEpoch && this.state?.window.activeWorkspace === id) this.focusSurface(surface); return; }
    if (action === "close") { if(this.definitions[shell.ui.views.find(v=>v.id===key)?.definitionKey??""]?.container==="sidebar")await this.perform(shell,{op:"hideTool",viewId:key});else await this.close([id], key); return; }
    if (action === "more") {
      this.showMenu(anchor,(shell.ui.tools??[]).map(t=>{
        const d=this.definitions[t.key],local=t.location?.hostId===shell.ui.host?.id,side=t.location?.side;
        const position=!t.location?'未打开':!local?'其他窗口':side?`${side==='left'?'左侧':'右侧'} · ${t.active?'展开':'收起'}`:'主区域';
        return {label:`${d.title} · ${position}`,disabled:!t.available,action:()=>this.perform(shell,{op:'open',key:t.key}),extra:local&&side?{label:t.pinned?'取消固定':'固定入口',action:()=>this.perform(shell,{op:'preference',key:t.key,pinned:!t.pinned})}:local&&t.viewId?{label:`停靠到${key==='left'?'左':'右'}侧`,action:()=>this.perform(shell,{op:'dock',viewId:t.viewId,side:key})}:undefined};
      }),true,undefined,keyboard);return;
    }
    if (action === "pages") { this.showMenu(anchor, shell.ui.views.filter(v => Object.values(shell.ui.host?.groups??{}).some(g=>g.viewIds.includes(v.id))).map(v => ({ label: v.title, action: () => this.perform(shell, { op: "select", viewId: v.id }) })), true); return; }
    if (action === "create") {
      const d = this.defaultAction;
      if (!d?.available || !shell.workspace.connected || this.creating.has(id) || this.pendingActions.has(id)) return;
      const focusEpoch = this.focusEpoch;
      this.creating.add(id); this.paint(shell);
      const requestId = crypto.randomUUID();
      try {
        this.pendingActions.set(id, {status:"pending",requestId,context:shell.workspace.context,focusEpoch});
        const result = await this.invoke<ActionResult>("invokeAction", {context:shell.workspace.context,requestId,actionKey:d.key,groupId});
        await this.finishAction(result, focusEpoch);
      } catch (error) {
        this.notify(String(error));
        try { await this.finishAction(await this.invoke<ActionResult>('actionStatus',{context:shell.workspace.context,requestId}),focusEpoch); }
        catch(statusError) { if(/不存在|上下文|已关闭|不属于|失效/.test(String(statusError)))this.pendingActions.delete(id); }
      } finally { this.creating.delete(id); if (this.shells.has(id)) this.paint(shell); }
      this.scheduleActionPolling();
    }
  }
  private async finishAction(result: ActionResult, focusEpoch?: number) {
    const id=result.context.workspaceId;
    if(result.status==="pending"){this.pendingActions.set(id,{...result,focusEpoch});return;}
    this.pendingActions.delete(id);
    await this.refresh();
    if(result.error)this.notify(`${result.error}${result.viewId?'；页面已创建':''}`);
    const shell=this.shells.get(id),surface=result.viewId? shell?.surfaces.get(result.viewId):undefined;
    if(surface && focusEpoch!==undefined && this.focusEpoch===focusEpoch && this.state?.window.activeWorkspace===id){
      shell?.node.querySelector<HTMLElement>(`[data-component-action="select"][data-key="${CSS.escape(result.viewId!)}"]`)?.scrollIntoView({block:"nearest",inline:"nearest"});
      this.focusSurface(surface);
    }
  }
  private scheduleActionPolling() {
    clearTimeout(this.actionTimer);
    if(this.disposed||document.hidden||this.pollingActions)return;
    const pending=this.pendingActions.get(this.state?.window.activeWorkspace??"");
    if(!pending||this.creating.has(pending.context.workspaceId))return;
    this.actionTimer=setTimeout(()=>{void this.pollAction(pending);},1000);
  }
  private async pollAction(pending: ActionResult & {focusEpoch?:number}) {
    if(this.disposed||document.hidden||this.state?.window.activeWorkspace!==pending.context.workspaceId)return;
    this.pollingActions=true;
    try{
      const result=await this.invoke<ActionResult>("actionStatus",{context:pending.context,requestId:pending.requestId});
      if(this.pendingActions.get(pending.context.workspaceId)?.requestId===pending.requestId)await this.finishAction(result,pending.focusEpoch);
    }catch(error){const message=String(error);if(/不存在|上下文|已关闭|不属于|失效/.test(message))this.pendingActions.delete(pending.context.workspaceId);if(pending.error!==message){pending.error=message;this.notify(message);}const shell=this.shells.get(pending.context.workspaceId);if(shell)this.paint(shell);}
    finally{this.pollingActions=false;this.scheduleActionPolling();}
  }
  private focus(shell: Shell, key?: string) {
    const selected = shell.ui.views.find(v => v.definitionKey === key && (this.definitions[v.definitionKey]?.container === "dialog" || [shell.ui.left, shell.ui.right, shell.ui.editor].includes(v.id)));
    if (selected) { const surface = shell.surfaces.get(selected.id); if (surface) this.focusSurface(surface); }
    else shell.node.querySelector<HTMLElement>(`[data-key="${CSS.escape(key ?? "")}"]`)?.focus();
  }
  private focusSurface(surface: Surface) { if (!surface.mounted || !surface.visible) { surface.focusPending = this.focusEpoch; return; } surface.focusPending = undefined; surface.frame.focus(); this.post(surface, { type: "focus" });void this.invoke("action",{context:surface.view.context,action:{op:"focus",viewId:surface.view.id}}).catch(()=>{}); }
  private showMenu(anchor: HTMLElement, entries: MenuEntry[], searchable = false, detail?: string, keyboard = false, reference:MenuReference=anchor,valid:()=>boolean=()=>true) {
    if (this.menu && this.menuAnchor === anchor && reference===anchor) { this.closeMenu(); return; }
    this.closeMenu(); const node = document.createElement("div"); node.className = "workspace-menu component-menu sd-menu"; node.tabIndex = -1;
    node.setAttribute("role", "menu");
    if (detail) { const text=document.createElement("p");text.textContent=detail;node.append(text); }
    if (searchable) { const input = document.createElement("input"); input.placeholder = "查找组件或页面"; input.setAttribute("aria-label", "查找组件或页面"); input.addEventListener("input", () => { node.querySelectorAll<HTMLElement>(".component-menu-row").forEach(r => r.hidden = !r.textContent!.toLowerCase().includes(input.value.toLowerCase())); void this.menuPosition?.update(); }); node.append(input); }
    const list = document.createElement("div"); list.className = "component-menu-list sd-menu-list"; node.append(list);
    for (const item of entries) { if (item.separatorBefore) list.append(document.createElement("hr")); const row = document.createElement("div"); row.className = "component-menu-row"; for (const entry of [item, item.extra].filter(Boolean) as MenuEntry[]) { const b = document.createElement("button"); b.setAttribute("role", entry.checked === undefined ? "menuitem" : "menuitemcheckbox"); if (entry.checked !== undefined) { b.setAttribute("aria-checked", String(entry.checked)); b.classList.add("menu-check"); } b.textContent = entry.label; b.disabled = entry.disabled === true; b.addEventListener("click", () => { const accepted=valid();this.closeMenu();if(accepted)void this.run(entry.action); }); row.append(b); } list.append(row); }
    if (!entries.length) { const text = document.createElement("p"); text.textContent = "暂无可用组件"; node.append(text); }
    node.addEventListener("keydown", event => {
      if(event.key==='Tab'){event.preventDefault();this.closeMenu();anchor.focus({preventScroll:true});return;}
      if (event.key === "Escape") { event.preventDefault(); this.closeMenu(); if (anchor.isConnected) anchor.focus({ preventScroll: true }); else this.content.querySelector<HTMLElement>(".server-workbench:not([hidden]) .workbench-rail button")?.focus(); return; }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || (event.target instanceof HTMLInputElement && !["ArrowDown", "ArrowUp"].includes(event.key))) return;
      event.preventDefault(); const buttons = [...node.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")].filter(b => !b.closest("[hidden]")); const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : index < 0 ? (event.key === "ArrowUp" ? buttons.length - 1 : 0) : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    });
    document.body.append(node); this.menu = node; this.menuAnchor = anchor;
    this.menuPosition = attachMenuPosition(node, reference, { isValid: () => this.menu === node&&valid(), onInvalid: () => { if (this.menu === node) this.closeMenu(); } });
    void this.menuPosition.ready.then(ready => { if (ready && this.menu === node) (searchable ? node.querySelector<HTMLElement>("input") : keyboard ? node.querySelector<HTMLElement>("button:not(:disabled)") : node)?.focus({ preventScroll: true }); });
  }
  private closeMenu() { this.menuPosition?.dispose(); this.menuPosition = undefined; this.menu?.remove(); this.menu = undefined; this.menuAnchor = undefined; }
  private closeBlocked = new Set<string>();
  private flushing?: Promise<void>;
  async flush(ids?: string[],freeze=false) {
    if(this.flushing)await this.flushing;
    const surfaces = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]).filter(s=>!ids||ids.includes(s.view.id));
    const identities=new Map(surfaces.map(s=>[s,s.token]));
    const deadline=Date.now()+45000;
    const job=(async()=>{
      await Promise.all(surfaces.filter(s => s.flushable && s.token).map(s => new Promise<void>((resolve,reject) => {
        const id=crypto.randomUUID(), timer=setTimeout(() => {s.flushing=undefined;reject(new Error("草稿同步超时，请返回处理或明确放弃"));}, Math.max(1,deadline-Date.now()));
        s.flushing={id,done:error=>{clearTimeout(timer);s.flushing=undefined;error?reject(new Error(error)):resolve();}};
        this.resumeRequests(s);
        this.post(s,{type:"flush",id,freeze});
      })));
      let timer:ReturnType<typeof setTimeout>|undefined;
      try{await Promise.race([Promise.all(surfaces.map(f => f.chain)),new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error("组件输入结果尚未确认")),Math.max(1,deadline-Date.now()));})]);}
      finally{clearTimeout(timer);}
      if(this.disposed||surfaces.some(s=>!s.mounted||s.token!==identities.get(s)))throw new Error("部分组件界面尚未就绪，无法确认输入");
    })();
    this.flushing=job;
    try{await job;return surfaces.flatMap(s=>s.token?[s.token]:[]);}finally{for(const surface of surfaces)surface.flushing?.done("输入同步已结束");if(this.flushing===job)this.flushing=undefined;}
  }
  close(workspaceIds: string[], viewId?: string) {
    return this.requestClose(viewId?{kind:"views",ids:[viewId]}:{kind:"workspace",id:workspaceIds[0]});
  }
  setCloseBlocked(ids:string[]) {
    const previous=this.closeBlocked;this.closeBlocked=new Set(ids);
    for(const shell of this.shells.values()) {
      const blocked=shell.ui.views.some(v=>this.closeBlocked.has(v.id));shell.node.inert=false;
      for(const surface of shell.surfaces.values()) {const modal=surface.node.querySelector<HTMLDialogElement>("dialog");const frozen=this.closeBlocked.has(surface.view.id);surface.node.inert=frozen||this.movingViews.has(surface.view.id)||Boolean(surface.readOnly);if(modal)modal.inert=frozen;if(!frozen){if(previous.has(surface.view.id))this.post(surface,{type:"transferCancelled"});this.resumeRequests(surface);}}
    }
  }
  private resumeRequests(surface:Surface) {
    const waiting=[...surface.deferred.values()];surface.deferred.clear();
    for(const event of waiting)this.message(event);
  }
  private async updateNetwork() {
    if(this.disposed||this.networkPending||!this.shells.size)return;this.networkPending=true;
    try {const snapshot=await this.call<NetworkSnapshot>("network_snapshot");for(const shell of this.shells.values()){
      const t=snapshot.servers.find(t=>t.serverId===shell.workspace.context.serverId),node=shell.node.querySelector(".workspace-network");
      const rate=(v:number)=>v<1024?`${v.toFixed(0)} B/s`:v<1048576?`${(v/1024).toFixed(1)} KiB/s`:`${(v/1048576).toFixed(1)} MiB/s`;
      if(node)node.textContent=`↓ ${rate(t?.receivedPerSecond??0)} ↑ ${rate(t?.sentPerSecond??0)}`;
    }} catch { /* A diagnostic endpoint must not interrupt the workbench. */ }finally{this.networkPending=false;}
  }
  private async run(action: () => Promise<unknown>) { try { await action(); } catch (error) { this.notify(String(error)); } }
  private removeSurface(s: Surface) { if(s.token)void this.invoke("detach",{surfaceId:s.token}).catch(()=>{}); s.deferred.clear();s.flushing?.done("原有界面已卸载，输入无法确认");clearTimeout(s.timeout); if(s.token)this.clipboard.discard(s.token); this.shared.detach(s.token); this.data.detach(s.token); this.post(s, { type: "unmount" }); s.node.remove(); }
  private destroy(shell: Shell) { if (shell.layoutFrame !== undefined) cancelAnimationFrame(shell.layoutFrame); shell.observer.disconnect(); for (const f of shell.surfaces.values()) this.removeSurface(f); shell.node.remove(); }
  dispose() { this.content.removeEventListener("contextmenu",this.toolMenuEvent);this.content.removeEventListener("keydown",this.toolMenuEvent);this.cancelGestures();this.content.removeEventListener("lostpointercapture",this.dragLost);window.removeEventListener("blur",this.cancelGestures);this.surfaceLayer.remove(); clearTimeout(this.actionTimer); document.removeEventListener("visibilitychange",this.resumeActionPolling); this.shared.dispose();this.content.removeEventListener("pointerdown",this.dragDown);this.content.removeEventListener("pointermove",this.dragMove);this.content.removeEventListener("pointerup",this.dragUp);this.content.removeEventListener("pointercancel",this.dragCancel);document.removeEventListener("keydown",this.layoutKey); this.settingsListener?.();this.locateListener?.(); this.fileDrop.dispose(); this.disposed = true; clearInterval(this.networkTimer); this.closeMenu(); window.removeEventListener("message", this.message); document.removeEventListener("pointerdown", this.outside); document.removeEventListener("pointerdown", this.interaction, true); document.removeEventListener("keydown", this.interaction, true); this.theme.disconnect(); window.matchMedia("(prefers-color-scheme: dark)").removeEventListener("change", this.systemTheme); for (const shell of this.shells.values()) this.destroy(shell); }
}
