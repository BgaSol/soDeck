import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { themeTokens } from "./theme";
import { ClipboardBridge } from "./clipboard";
import { LocalFileDrop } from "./local-files";
import { ComponentData } from "./component-data";
import { ComponentState } from "./component-state";
import { attachMenuPosition, type MenuPosition } from "./menu-position.js";
import { escape as e, icon, replace, patchChrome } from "./ui";
import type { Context, Projection, Workspace } from "./workspaces";
import type { NetworkSnapshot } from "./core";
import type { CloseTarget } from "./close";
import { panelGeometry, type PanelWidths, type PanelSide } from "./panel-layout";

type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
interface Definition { key: string; id: string; pluginId: string; pluginName: string; pluginInstanceId: string; title: string; icon: string; container: string; instancePolicy: string; entry: string; available: boolean; createAction?: string; locateEntry: boolean }
interface Preference { side: string; pinned: boolean; startup: string; order: number }
interface View { id: string; definitionKey: string; pluginInstanceId: string; context: Context; title: string; dirty: boolean; busy: boolean; error?: string; lastUsed: number; uiRequested?: boolean }
interface UiWorkspace { views: View[]; preferences: Record<string, Preference>; left: string | null; right: string | null; editor: string | null }
interface ActionDefinition { key: string; title: string; available: boolean; reason?: string }
interface ActionResult { status: "pending" | "completed" | "failed" | "invalidated"; requestId: string; context: Context; viewId?: string; error?: string }
export interface UiProjection { definitions: Record<string, Definition>; actions?: Record<string, ActionDefinition>; defaultCreateAction?: string; pendingAction?: ActionResult[]; workspaces: Record<string, UiWorkspace>; error?: string }
interface Surface { node: HTMLDivElement; frame: HTMLIFrameElement; view: View; token?: string; chain: Promise<unknown>; deferred: Map<string,MessageEvent>; visible: boolean; initialized: boolean; mounted?: boolean; acceptsLocalFiles?: boolean; focusPending?: number; flushable?: boolean; flushing?: {id: string; done: (error?: string) => void}; timeout: ReturnType<typeof setTimeout> }
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
  private shells = new Map<string, Shell>();
  private definitions: Record<string, Definition> = {};
  private state?: Projection;
  private menu?: HTMLElement;
  private menuAnchor?: HTMLElement;
  private menuPosition?: MenuPosition;
  private boundTabs = new WeakSet<Element>();
  private disposed = false;
  private creating = new Set<string>();
  private defaultAction?: ActionDefinition;
  private pendingActions = new Map<string, ActionResult & { focusEpoch?: number }>();
  private actionTimer?: ReturnType<typeof setTimeout>;
  private pollingActions = false;
  private resumeActionPolling = () => this.scheduleActionPolling();
  private layoutError?: string;
  private focusEpoch = 0;
  private interaction = () => { this.focusEpoch++; };
  private theme = new MutationObserver(() => this.broadcast());
  private networkTimer = setInterval(() => { void this.updateNetwork(); }, 1000);
  private networkPending = false;
  private settingsListener?: UnlistenFn;
  constructor(private content: HTMLElement, private call: Call, private notify: (message: string) => void, private refresh: () => Promise<void>, private requestClose: (target: CloseTarget) => Promise<boolean>, private widths: (workspace: Workspace) => PanelWidths, private cancelResize: () => void) {
    this.data = new ComponentData(call);
    this.shared = new ComponentState(call);
    void listen<{pluginId:string}>("core-settings-changed", event => {
      for (const shell of this.shells.values()) for (const surface of shell.surfaces.values()) if (surface.token && this.definitions[surface.view.definitionKey]?.pluginId === event.payload.pluginId) void this.refreshSettings(surface);
    }).then(unlisten => {if(this.disposed)unlisten();else this.settingsListener=unlisten;}).catch(()=>{});
    window.addEventListener("message", this.message);
    document.addEventListener("visibilitychange", this.resumeActionPolling);
    this.theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", this.systemTheme);
    content.addEventListener("click", event => {
      const button = (event.target as Element).closest<HTMLElement>("[data-component-action]");
      const shell = button?.closest<HTMLElement>(".server-workbench");
      if (button && shell) void this.run(() => this.action(shell.dataset.id!, button.dataset.componentAction!, button.dataset.key ?? "", button, (event as MouseEvent).detail === 0));
    });
    document.addEventListener("pointerdown", this.outside);
    document.addEventListener("pointerdown", this.interaction, true);
    document.addEventListener("keydown", this.interaction, true);
  }
  private invoke<T>(operation: string, args: Record<string, unknown> = {}) { return this.call<T>("component_ui", { operation, args }); }
  async sync(state: Projection) {
    const ui = await this.invoke<UiProjection>("snapshot");
    if (this.disposed) return;
    this.state = state; this.definitions = ui.definitions;
    this.defaultAction = ui.defaultCreateAction ? ui.actions?.[ui.defaultCreateAction] : undefined;
    if (ui.error && ui.error !== this.layoutError) { this.layoutError = ui.error; this.notify(ui.error); }
    const wanted = [...state.workspaces, ...(state.incoming ? [state.incoming] : [])];
    for (const [id, pending] of this.pendingActions) if (!wanted.some(w => JSON.stringify(w.context) === JSON.stringify(pending.context))) this.pendingActions.delete(id);
    for (const pending of ui.pendingAction ?? []) if (!this.pendingActions.has(pending.context.workspaceId)) this.pendingActions.set(pending.context.workspaceId, pending);
    for (const [id, shell] of this.shells) if (!wanted.some(w => w.context.workspaceId === id)) { this.destroy(shell); this.shells.delete(id); }
    this.content.querySelector(".receiving-workspace")?.remove();
    for (const w of wanted) {
      const id = w.context.workspaceId;
      let shell = this.shells.get(id);
      if (!shell) {
        const node = document.createElement("section"); node.className = "server-workbench"; node.dataset.id = id;
        node.innerHTML = `<aside class="workbench-rail left-rail sd-edge-right" aria-label="左侧组件"></aside><aside class="workbench-panel left-panel sd-edge-right"><header class="sd-panel-header sd-edge-bottom"></header><div class="component-slot"></div><div role="separator" aria-label="左侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="left"></div></aside><section class="workbench-center"><div class="component-tabs sd-tab-strip sd-edge-bottom"></div><div class="component-slot central-slot"></div></section><aside class="workbench-panel right-panel sd-edge-left"><header class="sd-panel-header sd-edge-bottom"></header><div class="component-slot"></div><div role="separator" aria-label="右侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="right"></div></aside><aside class="workbench-rail right-rail sd-edge-left" aria-label="右侧组件"></aside><footer class="workspace-status sd-status-bar sd-edge-top"></footer><div class="component-surfaces"></div>`;
        const observer = new ResizeObserver(() => this.scheduleLayout(id)); observer.observe(node);
        node.querySelectorAll(".component-slot").forEach(slot => observer.observe(slot));
        shell = { node, surfaces: new Map(), observer, workspace: w, ui: empty() }; this.shells.set(id, shell); this.content.append(node);
      }
      if (shell.workspace.connected !== w.connected) for (const surface of shell.surfaces.values()) this.post(surface, {type: "connectionChanged", connected: w.connected});
      shell.workspace = w; shell.ui = ui.workspaces[id] ?? empty();
      const incoming = state.incoming?.context.workspaceId === id;
      shell.node.hidden = !incoming && state.window.activeWorkspace !== id;
      shell.node.classList.toggle("incoming-component-workspace", incoming);
      shell.node.setAttribute("aria-label", `${w.name} 的工作区`);
      this.paint(shell);
      this.setCloseBlocked([...this.closeBlocked]);
    }
    if (!wanted.length) { const placeholder = document.createElement("div"); placeholder.className = "workspace-empty receiving-workspace"; placeholder.textContent = "此窗口没有工作区"; this.content.append(placeholder); }
    this.scheduleActionPolling();
  }
  private paint(shell: Shell) {
    const { node, ui: w } = shell;
    for (const side of ["left", "right"]) {
      const selected = side === "left" ? w.left : w.right;
      const entries = Object.values(this.definitions).filter(d => w.preferences[d.key]?.side === side && (d.container === "sidebar" || d.locateEntry)).sort((a,b) => (w.preferences[a.key].order - w.preferences[b.key].order) || a.key.localeCompare(b.key));
      patchChrome(node.querySelector(`.${side}-rail`)!, entries.filter(d => w.preferences[d.key].pinned).slice(0, shell.railCapacity ?? 20).map(d => {
        const active = d.container === "sidebar" ? w.views.some(v => v.id === selected && v.definitionKey === d.key) : w.views.some(v => v.id === w.editor && v.definitionKey === d.key);
        return `<button class="sd-icon" data-component-action="${d.container === "sidebar" ? "toggle" : "locate"}" data-key="${e(d.key)}" aria-label="${e(d.container === "sidebar" ? d.title : `定位${d.title}`)}" aria-expanded="${active}" title="${e(d.title)} · ${e(d.pluginName)}" ${!d.available ? "disabled" : ""}>${icon(d.icon)}</button>`;
      }).join("") + `<button class="component-more sd-icon" data-component-action="more" data-key="${side}" aria-label="${side === "left" ? "左" : "右"}侧更多组件">${icon("more")}</button>`);
      const view = w.views.find(v => v.id === selected);
      patchChrome(node.querySelector(`.${side}-panel header`)!, view ? `<span title="${e(this.definitions[view.definitionKey]?.pluginName)}">${e(view.title)}</span><div><button class="sd-icon" data-component-action="panel-menu" data-key="${e(view.definitionKey)}" aria-label="${e(view.title)} 面板菜单">${icon("more")}</button><button class="sd-icon" data-component-action="collapse" data-key="${side}" aria-label="收起${e(view.title)}">${icon("minus")}</button></div>` : "");
    }
    const editors = w.views.filter(v => this.definitions[v.definitionKey]?.container === "editor");
    const defaultAction = this.defaultAction;
    const pending = this.pendingActions.has(shell.workspace.context.workspaceId);
    const canCreate = shell.workspace.connected && defaultAction?.available && !pending && !this.creating.has(shell.workspace.context.workspaceId);
    const createHint = !shell.workspace.connected ? "连接已断开，请先重新连接" : pending ? "结果尚未确认" : this.creating.has(shell.workspace.context.workspaceId) ? "正在执行默认行为…" : !defaultAction ? "未设置可用的默认行为，请查看插件管理" : defaultAction.available ? defaultAction.title : defaultAction.reason || "默认行为提供者不可用";
    const tabScroll=node.querySelector(".component-tab-list")?.scrollLeft??0;
    patchChrome(node.querySelector(".component-tabs")!, `<div class="component-tab-list" role="tablist" aria-label="组件页面">${editors.map(v => `<div class="component-tab sd-tab ${w.editor === v.id ? "selected" : ""}" draggable="true" data-view="${e(v.id)}"><button role="tab" aria-selected="${w.editor === v.id}" data-component-action="select" data-key="${e(v.id)}" title="${e(v.title)}">${icon(this.definitions[v.definitionKey]?.icon)}<span>${e(v.title)}${v.dirty ? " ●" : ""}</span></button><button class="sd-icon" data-component-action="close" data-key="${e(v.id)}" aria-label="关闭 ${e(v.title)}">${icon("close")}</button></div>`).join("")}</div><button class="sd-icon" data-component-action="create" aria-label="${e(defaultAction?.title || "执行默认行为")}" title="${e(createHint)}" ${canCreate ? "" : "disabled"}>${icon("plus")}</button>${shell.workspace.connected && !defaultAction?.available ? `<button class="sd-icon" data-action="manage-plugins" aria-label="打开插件管理" title="默认行为不可用 · 打开插件管理">${icon("plug")}</button>` : ""}<button class="sd-icon" data-component-action="pages" aria-label="全部组件页面" title="全部组件页面">${icon("chevron")}</button>`);
    const list = node.querySelector(".component-tab-list")!;
    list.scrollLeft=tabScroll;
    if (!this.boundTabs.has(list)) {
      this.boundTabs.add(list);
      list.addEventListener("dragstart", event => { const ev = event as DragEvent; const id = (ev.target as Element).closest<HTMLElement>("[data-view]")?.dataset.view; if (id) ev.dataTransfer?.setData("text/sodeck-view", id); });
      list.addEventListener("dragover", event => event.preventDefault());
      list.addEventListener("drop", event => { event.preventDefault(); const ev = event as DragEvent, id = ev.dataTransfer?.getData("text/sodeck-view"), target = (ev.target as Element).closest<HTMLElement>("[data-view]")?.dataset.view; if (id && target) void this.run(() => this.perform(shell, { op: "reorder", viewId: id, index: shell.ui.views.findIndex(v => v.id === target) })); });
    }
    const slot = node.querySelector<HTMLElement>(".central-slot")!;
    if (!editors.length) replace(slot, `<div class="workspace-empty"><div class="empty-icon">${icon("plug")}</div><h2>${e(shell.workspace.name)}</h2><p>${!shell.workspace.connected ? "连接已断开，请先重新连接。" : !defaultAction?.available ? e(createHint) : "从侧栏打开组件，或点击＋执行默认行为。"}</p>${!shell.workspace.connected ? `<button data-reconnect-server="${e(shell.workspace.context.serverId)}">重新连接</button>` : !defaultAction?.available ? '<button data-action="manage-plugins">打开插件管理</button>' : ""}<div class="workspace-context">${e(shell.workspace.endpoint)}</div></div>`); else replace(slot, "");
    patchChrome(node.querySelector(".workspace-status")!, `<div class="workspace-connection" role="status" aria-label="连接状态" title="${e(shell.workspace.endpoint)}"><span class="health-dot ${shell.workspace.connected ? "good" : "warning"}"></span><span>${shell.workspace.connected ? "已连接" : "连接已断开"}</span><span class="workspace-endpoint">${e(shell.workspace.endpoint)}</span></div><button class="sd-status-action" data-reconnect-server="${e(shell.workspace.context.serverId)}" ${shell.workspace.connected ? "hidden" : ""}>重新连接</button><span class="workspace-status-spacer"></span><button class="workspace-network sd-status-action" data-action="status" title="服务器连接传输速率；点击查看应用状态与插件业务量">↓ 0 B/s ↑ 0 B/s</button>`);
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
        node.querySelector(".component-surfaces")!.append(host);
        surface = { node: host, frame, view: v, chain: Promise.resolve(), deferred:new Map(), visible: false, initialized: false, timeout: setTimeout(() => this.surfaceError(surface!, "组件界面未在 8 秒内就绪"), 8000) };
        shell.surfaces.set(v.id, surface);
      }
      surface.view = v;
      surface.frame.title = `${v.title} · ${shell.workspace.name}`;
      if (!d.available) this.surfaceError(surface, "插件已停止，页面状态仍保留。请查看应用状态与日志。");
    }
    this.scheduleLayout(shell.workspace.context.workspaceId);
  }
  private geometry(shell: Shell, widths = this.widths(shell.workspace)) {
    const rails = ["left", "right"].reduce((sum, side) => sum + shell.node.querySelector(`.${side}-rail`)!.getBoundingClientRect().width, 0);
    const a = shell.ui.views.find(v => v.id === shell.ui.left), b = shell.ui.views.find(v => v.id === shell.ui.right);
    return panelGeometry(shell.node.getBoundingClientRect().width - rails - 320, widths, Boolean(shell.ui.left), Boolean(shell.ui.right), (a?.lastUsed ?? 0) >= (b?.lastUsed ?? 0) ? "left" : "right");
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
    const r = shell.node.getBoundingClientRect(), w = shell.ui;
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
    const placements = new Map<Surface, DOMRect>();
    for (const surface of shell.surfaces.values()) {
      if(surface.mounted && !surface.frame.dataset.stateReady && this.state?.workspaces.some(w=>w.context.workspaceId===surface.view.context.workspaceId)) {
        surface.frame.dataset.stateReady='true';this.post(surface,{type:'surfaceReady'});
      }
      const id = surface.view.id;
      const slot = id === w.editor ? shell.node.querySelector(".central-slot") : id === w.left && left ? shell.node.querySelector(".left-panel .component-slot") : id === w.right && right ? shell.node.querySelector(".right-panel .component-slot") : null;
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
      surface.node.hidden = !s;
      if (s) for (const [key, value] of Object.entries({ left: `${s.left-r.left}px`, top: `${s.top-r.top}px`, width: `${s.width}px`, height: `${s.height}px` })) { if (surface.node.style.getPropertyValue(key) !== value) surface.node.style.setProperty(key, value); }
      if (surface.visible !== visible) { if(visible)void this.refreshSettings(surface); if(!visible&&surface.token)this.clipboard.discard(surface.token); surface.visible = visible; this.post(surface, { type: "visibilityChanged", visible }); }
      if (visible && surface.focusPending === this.focusEpoch) this.focusSurface(surface);
    }
  }
  private tokens() { return themeTokens(); }
  private post(surface: Surface, message: Record<string, unknown>) { surface.frame.contentWindow?.postMessage({ channel: "sodeck-ui-v1", surfaceId: surface.token, ...message }, new URL(surface.frame.src).origin === "null" ? "*" : new URL(surface.frame.src).origin); }
  private broadcast() { for (const s of this.shells.values()) for (const f of s.surfaces.values()) this.post(f, { type: "themeChanged", tokens: this.tokens() }); }
  private systemTheme = () => this.broadcast();
  private async refreshSettings(surface: Surface) {
    const token = surface.token; if (!token || this.disposed) return;
    try { const settings = await this.call("settings_for_surface", {surfaceId: token}); if(surface.token===token && !this.disposed && settings) this.post(surface,{type:"settingsChanged",settings}); }
    catch { /* Older plugins and cores continue with their defaults. */ }
  }
  private outside = (event: PointerEvent) => { if (this.menu && !this.menu.contains(event.target as Node) && !this.menuAnchor?.contains(event.target as Node)) this.closeMenu(); };
  private message = (event: MessageEvent) => {
    if (!event.data || event.data.channel !== "sodeck-ui-v1") return;
    const surface = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]).find(s => s.frame.contentWindow === event.source);
    if (!surface) return;
    const expectedOrigin = new URL(surface.frame.src).origin;
    if (expectedOrigin !== "null" && event.origin !== expectedOrigin) return;
    const message = event.data;
    if (message.type === "ready") {
      if (surface.initialized) return; surface.initialized = true;
      surface.chain = this.invoke<{ surfaceId: string; data: unknown; view: View; capabilities?:{sharedState?:number;fileLocation?:number;fileLocationProof?:number;uiOpenView?:number} }>("attach", { viewId: surface.view.id }).then(async result => {
        surface.token = result.surfaceId;
        this.shared.attach(result.surfaceId,result.view.context,message=>this.post(surface,message));
        const settings = await this.call("settings_for_surface", {surfaceId: result.surfaceId}).catch(() => null);
        this.post(surface, { type: "mount", connected: this.shells.get(surface.view.context.workspaceId)?.workspace.connected, context: result.view.context, serverName: this.shells.get(surface.view.context.workspaceId)?.workspace.name, viewId: surface.view.id, viewType: surface.view.definitionKey.split(":")[1], data: result.data, tokens: this.tokens(), settings, capabilities: {clipboard:1,localFiles:1,settings:1,sharedState:result.capabilities?.sharedState,fileLocation:result.capabilities?.fileLocation,fileLocationProof:result.capabilities?.fileLocationProof,uiOpenView:result.capabilities?.uiOpenView}, visible: surface.visible });
        surface.frame.dataset.surfaceId = result.surfaceId;
      }).catch(error => this.surfaceError(surface, String(error)));
      return;
    }
    if (!surface.token || message.surfaceId !== surface.token) return;
    if (message.type === "interaction") { if (surface.visible) { this.interaction(); this.closeMenu(); this.content.dispatchEvent(new Event("surface-interaction")); } return; }
    if (message.type === "mounted") { surface.flushable = message.flush === true; surface.acceptsLocalFiles = message.localFiles === true; clearTimeout(surface.timeout); void this.invoke("ready", { surfaceId: surface.token }).then(async () => { surface.mounted = true; this.scheduleLayout(surface.view.context.workspaceId); await this.refreshSettings(surface); await this.refresh(); if (surface.focusPending === this.focusEpoch && surface.visible) this.focusSurface(surface); }).catch(error => this.surfaceError(surface, String(error))); return; }
    if (message.type === "flushed" && surface.flushing && surface.flushing.id === message.id) { surface.flushing.done(message.error); return; }
    if (message.type !== "request" || typeof message.id !== "string") return;
    const requestId = message.id;
    if(message.method === 'state'){
      const token=surface.token,context=JSON.stringify(surface.view.context);
      void this.shared.request(token,message.params??{}).then(data=>{if(surface.token===token && JSON.stringify(surface.view.context)===context)this.post(surface,{type:'response',id:requestId,data});})
        .catch(error=>{if(surface.token===token)this.post(surface,{type:'response',id:requestId,error:String(error)});});
      return;
    }
    if(this.closeBlocked.has(surface.view.context.workspaceId)&&!surface.flushing) {
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
        if (message.method === "openView") { const key = `${this.definitions[surface.view.definitionKey].pluginId}:${message.params.viewType}`; data = await this.perform(shell, { op: "open", key, params: message.params.params, resourceKey: message.params.resourceKey }); if (this.focusEpoch === requestFocusEpoch && this.state?.window.activeWorkspace === shell.workspace.context.workspaceId) this.focus(shell,key); }
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
  }
  private async perform(shell: Shell, action: Record<string, unknown>) {
    const result = await this.invoke("action", { context: shell.workspace.context, action: { requestId: crypto.randomUUID(), ...action } });
    await this.refresh(); return result;
  }
  private async action(id: string, action: string, key: string, anchor: HTMLElement, keyboard = false) {
    const shell = this.shells.get(id);
    if (!shell) return;
    const focusEpoch = this.focusEpoch;
    if (["toggle", "locate", "panel-menu"].includes(action) && !this.definitions[key]?.available) { this.notify("组件不可用，请查看应用状态"); return; }
    if (["toggle", "locate", "collapse"].includes(action)) { await this.perform(shell, { op: action, key, side: key }); if (this.focusEpoch === focusEpoch && this.state?.window.activeWorkspace === id) this.focus(shell, action === "collapse" ? undefined : key); return; }
    if (action === "select") { if (!shell.ui.views.some(v => v.id === key)) return; await this.perform(shell, { op: "select", viewId: key }); const surface = shell.surfaces.get(key); if (surface && this.focusEpoch === focusEpoch && this.state?.window.activeWorkspace === id) this.focusSurface(surface); return; }
    if (action === "close") { await this.close([id], key); return; }
    if (action === "more") { this.showMenu(anchor, Object.values(this.definitions).filter(d => shell.ui.preferences[d.key]?.side === key && (d.container === "sidebar" || d.locateEntry)).map(d => ({ label: d.title, action: () => this.perform(shell, { op: d.container === "sidebar" ? "open" : "locate", key: d.key }), extra: { label: shell.ui.preferences[d.key].pinned ? "取消固定" : "固定", action: () => this.perform(shell, { op: "preference", key: d.key, pinned: !shell.ui.preferences[d.key].pinned }) } })), true); return; }
    if (action === "panel-menu") {
      const p = shell.ui.preferences[key];
      if (!p) return;
      const sideKeys = Object.keys(shell.ui.preferences).filter(k => shell.ui.preferences[k].side === p.side).sort((a,b) => shell.ui.preferences[a].order-shell.ui.preferences[b].order);
      this.showMenu(anchor, [
        { label: p.side === "left" ? "移到右侧" : "移到左侧", action: () => this.perform(shell, { op: "preference", key, side: p.side === "left" ? "right" : "left" }) },
        { label: p.pinned ? "隐藏入口" : "固定入口", action: () => this.perform(shell, { op: "preference", key, pinned: !p.pinned }) },
        { label: "入口置顶", action: () => this.perform(shell, { op: "preference", key, order: Math.max(0, Math.min(...sideKeys.map(k => shell.ui.preferences[k].order)) - 1) }) },
        { label: "新工作区自动展开", checked: p.startup !== "none", separatorBefore: true, action: () => this.perform(shell, { op: "preference", key, startup: p.startup === "none" ? "reveal" : "none" }) },
        { label: "恢复推荐布局", action: () => this.perform(shell, { op: "recommend", key }) },
      ], false, undefined, keyboard); return;
    }
    if (action === "pages") { this.showMenu(anchor, shell.ui.views.filter(v => this.definitions[v.definitionKey]?.container === "editor").map(v => ({ label: v.title, action: () => this.perform(shell, { op: "select", viewId: v.id }) })), true); return; }
    if (action === "create") {
      const d = this.defaultAction;
      if (!d?.available || !shell.workspace.connected || this.creating.has(id) || this.pendingActions.has(id)) return;
      const focusEpoch = this.focusEpoch;
      this.creating.add(id); this.paint(shell);
      const requestId = crypto.randomUUID();
      try {
        this.pendingActions.set(id, {status:"pending",requestId,context:shell.workspace.context,focusEpoch});
        const result = await this.invoke<ActionResult>("invokeAction", {context:shell.workspace.context,requestId,actionKey:d.key});
        await this.finishAction(result, focusEpoch);
      } catch (error) {
        this.notify(String(error));
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
  private focusSurface(surface: Surface) { if (!surface.mounted || !surface.visible) { surface.focusPending = this.focusEpoch; return; } surface.focusPending = undefined; surface.frame.focus(); this.post(surface, { type: "focus" }); }
  private showMenu(anchor: HTMLElement, entries: MenuEntry[], searchable = false, detail?: string, keyboard = false) {
    if (this.menu && this.menuAnchor === anchor) { this.closeMenu(); return; }
    this.closeMenu(); const node = document.createElement("div"); node.className = "workspace-menu component-menu sd-menu"; node.tabIndex = -1;
    node.setAttribute("role", "menu");
    if (detail) { const text=document.createElement("p");text.textContent=detail;node.append(text); }
    if (searchable) { const input = document.createElement("input"); input.placeholder = "查找组件或页面"; input.setAttribute("aria-label", "查找组件或页面"); input.addEventListener("input", () => { node.querySelectorAll<HTMLElement>(".component-menu-row").forEach(r => r.hidden = !r.textContent!.toLowerCase().includes(input.value.toLowerCase())); void this.menuPosition?.update(); }); node.append(input); }
    const list = document.createElement("div"); list.className = "component-menu-list sd-menu-list"; node.append(list);
    for (const item of entries) { if (item.separatorBefore) list.append(document.createElement("hr")); const row = document.createElement("div"); row.className = "component-menu-row"; for (const entry of [item, item.extra].filter(Boolean) as MenuEntry[]) { const b = document.createElement("button"); b.setAttribute("role", entry.checked === undefined ? "menuitem" : "menuitemcheckbox"); if (entry.checked !== undefined) { b.setAttribute("aria-checked", String(entry.checked)); b.classList.add("menu-check"); } b.textContent = entry.label; b.disabled = entry.disabled === true; b.addEventListener("click", () => { this.closeMenu(); void this.run(entry.action); }); row.append(b); } list.append(row); }
    if (!entries.length) { const text = document.createElement("p"); text.textContent = "暂无可用组件"; node.append(text); }
    node.addEventListener("keydown", event => {
      if (event.key === "Escape") { event.preventDefault(); this.closeMenu(); if (anchor.isConnected) anchor.focus({ preventScroll: true }); else this.content.querySelector<HTMLElement>(".server-workbench:not([hidden]) .workbench-rail button")?.focus(); return; }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || (event.target instanceof HTMLInputElement && !["ArrowDown", "ArrowUp"].includes(event.key))) return;
      event.preventDefault(); const buttons = [...node.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")].filter(b => !b.closest("[hidden]")); const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : index < 0 ? (event.key === "ArrowUp" ? buttons.length - 1 : 0) : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    });
    document.body.append(node); this.menu = node; this.menuAnchor = anchor;
    this.menuPosition = attachMenuPosition(node, anchor, { isValid: () => this.menu === node, onInvalid: () => { if (this.menu === node) this.closeMenu(); } });
    void this.menuPosition.ready.then(ready => { if (ready && this.menu === node) (searchable ? node.querySelector<HTMLElement>("input") : keyboard ? node.querySelector<HTMLElement>("button:not(:disabled)") : node)?.focus({ preventScroll: true }); });
  }
  private closeMenu() { this.menuPosition?.dispose(); this.menuPosition = undefined; this.menu?.remove(); this.menu = undefined; this.menuAnchor = undefined; }
  private closeBlocked = new Set<string>();
  private flushing?: Promise<void>;
  async flush(ids?: string[]) {
    if(this.flushing)await this.flushing;
    const surfaces = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]).filter(s=>!ids||ids.includes(s.view.id));
    const deadline=Date.now()+45000;
    const job=(async()=>{
      await Promise.all(surfaces.filter(s => s.flushable && s.token).map(s => new Promise<void>((resolve,reject) => {
        const id=crypto.randomUUID(), timer=setTimeout(() => {s.flushing=undefined;reject(new Error("草稿同步超时，请返回处理或明确放弃"));}, Math.max(1,deadline-Date.now()));
        s.flushing={id,done:error=>{clearTimeout(timer);s.flushing=undefined;error?reject(new Error(error)):resolve();}};
        this.resumeRequests(s);
        this.post(s,{type:"flush",id});
      })));
      let timer:ReturnType<typeof setTimeout>|undefined;
      try{await Promise.race([Promise.all(surfaces.map(f => f.chain)),new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error("组件输入结果尚未确认")),Math.max(1,deadline-Date.now()));})]);}
      finally{clearTimeout(timer);}
      if(surfaces.some(s=>!s.mounted))throw new Error("部分组件界面尚未就绪，无法确认输入");
    })();
    this.flushing=job;
    try{await job;return surfaces.flatMap(s=>s.token?[s.token]:[]);}finally{for(const surface of surfaces)surface.flushing?.done("输入同步已结束");if(this.flushing===job)this.flushing=undefined;}
  }
  close(workspaceIds: string[], viewId?: string) {
    return this.requestClose(viewId?{kind:"views",ids:[viewId]}:{kind:"workspace",id:workspaceIds[0]});
  }
  setCloseBlocked(ids:string[]) {
    this.closeBlocked=new Set(ids);
    for(const shell of this.shells.values()) {
      const blocked=this.closeBlocked.has(shell.workspace.context.workspaceId);shell.node.inert=blocked;
      for(const surface of shell.surfaces.values()) {const modal=surface.node.querySelector<HTMLDialogElement>("dialog");if(modal)modal.inert=blocked;if(!blocked)this.resumeRequests(surface);}
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
  private removeSurface(s: Surface) { s.deferred.clear();s.flushing?.done("原有界面已卸载，输入无法确认");clearTimeout(s.timeout); if(s.token)this.clipboard.discard(s.token); this.shared.detach(s.token); this.data.detach(s.token); this.post(s, { type: "unmount" }); s.node.remove(); }
  private destroy(shell: Shell) { if (shell.layoutFrame !== undefined) cancelAnimationFrame(shell.layoutFrame); shell.observer.disconnect(); for (const f of shell.surfaces.values()) this.removeSurface(f); shell.node.remove(); }
  dispose() { clearTimeout(this.actionTimer); document.removeEventListener("visibilitychange",this.resumeActionPolling); this.shared.dispose(); this.settingsListener?.(); this.fileDrop.dispose(); this.disposed = true; clearInterval(this.networkTimer); this.closeMenu(); window.removeEventListener("message", this.message); document.removeEventListener("pointerdown", this.outside); document.removeEventListener("pointerdown", this.interaction, true); document.removeEventListener("keydown", this.interaction, true); this.theme.disconnect(); window.matchMedia("(prefers-color-scheme: dark)").removeEventListener("change", this.systemTheme); for (const shell of this.shells.values()) this.destroy(shell); }
}
