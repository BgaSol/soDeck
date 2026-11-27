import { invoke } from "@tauri-apps/api/core";
import { escape as e, icon } from "./ui";
import { DomRegion } from "./dom-update";
import { Components } from "./components";
import { CloseCoordinator, type HostInputs } from "./close";
import { attachMenuPosition, type MenuPosition, type MenuReference } from "./menu-position.js";
import type { PanelSide, PanelWidths } from "./panel-layout";
import { TabStrip } from './tab-strip';
import { TabPicker } from './tab-picker';
import { WorkbenchDrag, type DropPreview, type DragEvent } from './workbench-drag';

export interface Context { workspaceId: string; serverId: string; connectionId: string; generation: number }
export interface Layout { leftOpen: boolean; rightOpen: boolean; leftWidth: number; rightWidth: number }
export interface Workspace { context: Context; name: string; endpoint: string; connected: boolean; windowId: string; layout: Layout; viewIds: string[]; activeViewId: string | null }
interface WorkspaceWindow {id:string;role:string;workspaceIds:string[];activeWorkspace:string|null;ready:boolean}
export interface Projection { revision: number; window: WorkspaceWindow; workspaces: Workspace[]; incoming: Workspace | null; incomingWorkspaces?:Workspace[]; windows?:WorkspaceWindow[] }
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
interface PanelGesture { id: string; side: PanelSide; pointer: number; x: number; initial: number; width: number; otherWidth: number; handle: HTMLElement; focus?: HTMLElement }
interface WidthChange { width: number; sequence: number }
interface WidthState { values: Partial<Record<PanelSide, WidthChange>>; flight?: Promise<void>; failed?: boolean }

/** Each view is rendered from its own immutable context; no global active server API. */
export class Workspaces {
  private components: Components;
  private tabRegion: DomRegion;
  private closer: CloseCoordinator;
  private state?: Projection;
  private refreshing?: Promise<void>;
  private again = false;
  private menu?: HTMLElement;
  private menuAnchor?: HTMLElement;
  private menuPosition?: MenuPosition;
  private drag: WorkbenchDrag;
  private panelGesture?: PanelGesture;
  private widths = new Map<string, WidthState>();
  private widthSequence = 0;
  private moving = new Set<string>();
  private disposed = false;
  private events = new AbortController();
  private reportTimer?: ReturnType<typeof setTimeout>;
  private preview?: DropPreview;
  private observer: MutationObserver;
  private resizeObserver: ResizeObserver;
  private strip?: TabStrip;
  private picker?: TabPicker;

  constructor(private tabs: HTMLElement, private content: HTMLElement, private notify: (message: string) => void, private call: Invoke = invoke) {
    this.drag = new WorkbenchDrag(call, notify, preview => this.renderDrop(preview));
    this.tabRegion = new DomRegion(tabs, () => this.render());
    this.closer = new CloseCoordinator(call,()=>this.state?.window.id??"main",async ids=>{this.finishResize(true);await this.flushLayouts().catch(error=>notify(String(error)));return this.components.flush(ids,true);},(ids,blocked,host)=>{
      if (blocked) {void this.drag.cancel();this.finishResize(true);this.closeMenu();}
      this.components.setCloseBlocked(blocked?ids:[]);
      this.blockHost?.(host);
      this.hostBlock?.(host);
    },notify);
    this.components = new Components(content, call, notify, () => this.refresh(),async target=>{
      this.finishResize(true);
      const accepted=await this.closer.request(target);
      await this.refresh();
      return accepted;
    }, workspace => this.displayWidths(workspace), () => this.finishResize(true), this.drag);
    const controls = tabs.parentElement?.querySelector<HTMLElement>('.tab-controls');
    if (controls) {
      this.strip = new TabStrip(tabs, controls, () => { this.paintPreview(); this.report(); });
      controls.addEventListener('click', event => {
        const anchor = (event.target as Element).closest<HTMLElement>('[data-tab-action="pick"]');
        if (!anchor) return;
        if (anchor.getAttribute('aria-expanded') === 'true') { this.picker?.dispose(true); return; }
        this.closeMenu(); this.components.closeTabPicker();
        const windowId = this.state?.window.id;
        this.picker = new TabPicker(anchor, '服务器标签', () => (this.state?.workspaces ?? []).map(w => ({id:w.context.workspaceId,title:w.name,detail:w.endpoint,icon:'server',current:w.context.workspaceId===this.state?.window.activeWorkspace,status:w.connected?'已连接':'已断开',disabled:this.moving.has(w.context.workspaceId),closeLabel:'关闭'})),
          id => this.run(async () => { await this.action('select',id); await this.refresh(); this.strip?.reveal(id); this.tabs.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"] [role="tab"]`)?.focus({preventScroll:true}); }),
          id => this.run(() => this.action('close',id)), () => this.state?.window.id === windowId && !document.querySelector('dialog[open], .workspace[inert]'), notify);
      }, {signal:this.events.signal});
    }
    document.addEventListener('tab-picker-open', () => this.closeMenu(false), {signal:this.events.signal});
    tabs.addEventListener("click", event => {
      const button = (event.target as Element).closest<HTMLElement>("[data-workspace-action]");
      if (button) void this.run(() => this.action(button.dataset.workspaceAction!, button.dataset.id!));
    }, {signal: this.events.signal});
    tabs.addEventListener("contextmenu", event => {
      const tab = (event.target as Element).closest<HTMLElement>(".server-tab");
      if (!tab) return; event.preventDefault(); this.openMenu(tab.dataset.id!, tab.querySelector<HTMLElement>('.server-tab-select')!,{x:event.clientX,y:event.clientY,contextElement:tab});
    }, {signal: this.events.signal});
    tabs.addEventListener("pointerdown", event => this.down(event), {signal: this.events.signal});
    tabs.addEventListener("scroll", () => this.report(), { passive: true, signal: this.events.signal});
    document.addEventListener("pointerdown", event => { if (this.menu && !this.menu.contains(event.target as Node) && !this.menuAnchor?.contains(event.target as Node)) this.closeMenu(); }, {signal: this.events.signal});
    content.addEventListener("surface-interaction", () => this.closeMenu(), {signal: this.events.signal});
    content.addEventListener('tab-geometry-changed', () => this.report(), {signal:this.events.signal});
    document.addEventListener("keydown", event => this.key(event), {signal: this.events.signal});
    content.addEventListener("pointerdown", event => this.resize(event), {signal: this.events.signal});
    content.addEventListener("pointermove", event => this.resizeMove(event), {signal: this.events.signal});
    content.addEventListener("pointerup", event => { if (event.pointerId === this.panelGesture?.pointer) this.finishResize(false, event); }, {signal: this.events.signal});
    for (const name of ["pointercancel", "lostpointercapture"] as const) content.addEventListener(name, event => { if (event.pointerId === this.panelGesture?.pointer) this.finishResize(true); }, {signal: this.events.signal});
    window.addEventListener("blur", () => {this.finishResize(true);this.closeMenu();}, {signal: this.events.signal});
    this.observer = new MutationObserver(() => this.report());
    this.observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open", "hidden", "inert"] });
    this.resizeObserver = new ResizeObserver(() => this.report()); this.resizeObserver.observe(tabs);
    const blank = document.querySelector('#drag-region'); if (blank) this.resizeObserver.observe(blank);
  }
  async start() { await this.refresh(); await this.closer.start(); await this.reportNow(); await this.call("workspace_ready"); }
  refresh(): Promise<void> {
    if(this.disposed)return Promise.resolve();
    if (this.refreshing) { this.again = true; return this.refreshing; }
    this.refreshing = (async () => {
      do {
        this.again = false;
        const state = await this.call<Projection>("workspace_snapshot");
        if(this.disposed)return;
        if (!this.state || state.revision >= this.state.revision) {
          if (this.panelGesture && state.window.activeWorkspace !== this.panelGesture.id) this.finishResize(true);
          for (const [id, widths] of this.widths) if (!state.workspaces.some(w => w.context.workspaceId === id) || (widths.failed && !widths.flight)) this.widths.delete(id);
          this.state = state; await this.components.sync(state); this.drag.validate(); this.render();
        }
      } while (this.again);
    })().finally(() => { this.refreshing = undefined; });
    return this.refreshing;
  }
  private projectionChanged?: (state:Projection)=>void;
  private hostVisible=true;
  private selectedHost?: ()=>void;
  private selectingHost?: ()=>void;
  private blockHost?: (blocked:boolean)=>void;
  setHostCallbacks(changed:(state:Projection)=>void,selected:()=>void,blocked:(blocked:boolean)=>void,selecting:()=>void) {this.projectionChanged=changed;this.selectedHost=selected;this.blockHost=blocked;this.selectingHost=selecting;}
  setHostVisible(visible:boolean) {this.hostVisible=visible;this.drag.setVisible(visible);this.finishResize(true);this.closeMenu();this.components.setHostVisible(visible);if(this.state)this.render();}
  get layoutSaveFailed() {return this.components.layoutSaveFailed;}
  async retryLayoutSave() {await this.components.retryLayoutSave();await this.refresh();}
  private current() { return this.state?.workspaces.find(w => w.context.workspaceId === this.state?.window.activeWorkspace); }
  private async run(fn: () => Promise<unknown>) { try { await fn(); } catch (error) { this.notify(String(error)); } finally { if (!this.disposed) await this.refresh().catch(error => this.notify(String(error))); } }
  private render() {
    const state = this.state!;
    const main = state.window.role === "main";
    document.body.classList.toggle("secondary-window", !main);
    const fixed = document.querySelector<HTMLElement>("#catalog-tab");
    if (fixed) fixed.hidden = !main;
    const focused = document.activeElement as HTMLElement;
    const focusId = this.tabs.contains(focused) ? focused.closest<HTMLElement>(".server-tab")?.dataset.id : undefined;
    const focusAction = focused?.dataset.workspaceAction;
    const scroll = this.tabs.scrollLeft;
    this.tabRegion.update(this.tabs, state.workspaces.map(w => {
      const id = e(w.context.workspaceId), active = this.hostVisible && state.window.activeWorkspace === w.context.workspaceId;
      return `<div data-motion="item" class="server-tab sd-tab ${active ? "selected" : ""} ${this.moving.has(w.context.workspaceId) ? "moving" : ""}" data-id="${id}" data-tab-id="${id}" data-tauri-drag-region="false"><button class="server-tab-select" role="tab" aria-selected="${active}" tabindex="${active ? 0 : -1}" data-workspace-action="select" data-id="${id}" title="${e(w.name)} · ${e(w.endpoint)}"><span class="health-dot ${w.connected ? "good" : "warning"}"></span><span class="tab-name">${e(w.name)}</span></button><button class="tab-close-button sd-icon" data-workspace-action="close" data-id="${id}" aria-label="关闭 ${e(w.name)}" title="关闭此窗口的服务器标签">${icon("close")}</button></div>`;
    }).join(""));
    this.tabs.scrollLeft = scroll;
    this.strip?.update(state.window.activeWorkspace); this.picker?.update();
    if (focusId) this.tabs.querySelector<HTMLElement>(`[data-id="${CSS.escape(focusId)}"] [data-workspace-action="${focusAction ?? "select"}"]`)?.focus({ preventScroll: true });
    const w = this.current();
    this.projectionChanged?.(state);
    if (w) {
      this.content.dataset.workspaceId = w.context.workspaceId;
      this.content.inert = this.moving.has(w.context.workspaceId);
    }
    void this.menuPosition?.update(); this.paintPreview(); this.report();
  }
  private async action(action: string, id: string) {
    if (!this.state?.workspaces.some(w => w.context.workspaceId === id)) return;
    if (action !== 'close') this.closeMenu();
    this.finishResize(true);
    if (action === "select") { this.selectingHost?.();await this.call("workspace_activate", { id }); await this.refresh(); this.selectedHost?.(); return; }
    if (action === "close") { await this.closer.request({kind:"host",id}); return; }
    if (this.moving.has(id)) return;
    this.moving.add(id); this.render();
    try { await this.flushLayouts(); await this.components.moveHost(id, action === "return" ? "main" : action.startsWith('window:')?action.slice(7):undefined); }
    finally { this.moving.delete(id); }
  }
  private openMenu(id: string, anchor: HTMLElement, reference:MenuReference=anchor) {
    this.finishResize(true);void this.drag.cancel();
    this.closeMenu();
    if (!anchor || !this.state?.workspaces.some(w => w.context.workspaceId === id)) return;
    const node = document.createElement("div"); node.className = "workspace-menu sd-menu"; node.setAttribute("role", "menu");
    const secondary=this.state.window.id!=="main",moving=this.moving.has(id);
    node.innerHTML = `<div class="sd-menu-list">${secondary?`<button role="menuitem" data-choice="return" ${moving?'disabled':''}>返回主窗口</button>`:""}${!secondary||this.state.workspaces.length>1?`<button role="menuitem" data-choice="detach" ${moving?'disabled':''}>移到新窗口</button>`:""}<hr><button role="menuitem" data-choice="close" ${moving?'disabled':''}>关闭标签</button></div>`;
    node.addEventListener("click", event => { const action = (event.target as Element).closest<HTMLElement>("[data-choice]")?.dataset.choice; if (action) void this.run(() => this.action(action, id)); });
    node.addEventListener("keydown", event => {
      if(event.key==="Tab"){event.preventDefault();this.closeMenu();anchor.focus({preventScroll:true});return;}
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return; event.preventDefault();
      const buttons = [...node.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")]; const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    });
    document.body.append(node); this.menu = node; this.menuAnchor = anchor;
    const active = this.state.window.activeWorkspace;
    this.menuPosition = attachMenuPosition(node, reference, {
      isValid: () => this.menu === node && this.state?.window.activeWorkspace === active && Boolean(this.state.workspaces.some(w => w.context.workspaceId === id)),
      onInvalid: () => { if (this.menu === node) this.closeMenu(); },
    });
    void this.menuPosition.ready.then(ready => { if (ready && this.menu === node) node.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus(); });
  }
  private closeMenu(picker = true) { if (picker) this.picker?.dispose(); this.menuPosition?.dispose(); this.menuPosition = undefined; this.menu?.remove(); this.menu = undefined; this.menuAnchor = undefined; }
  private key(event: KeyboardEvent) {
    if((event.key==="ContextMenu"||event.shiftKey&&event.key==="F10")&&this.tabs.contains(event.target as Node)) {
      const tab=(event.target as HTMLElement).closest<HTMLElement>('.server-tab');if(!tab)return;
      event.preventDefault();this.openMenu(tab.dataset.id!,tab.querySelector<HTMLElement>('.server-tab-select')!);return;
    }
    if (event.key === "Escape") {
      if (this.panelGesture) { event.preventDefault(); this.finishResize(true); }
      for (const id of this.moving) void this.call("workspace_cancel_move", { id }).catch(error => this.notify(String(error)));
      if (this.menu) { event.preventDefault(); const anchor = this.menuAnchor; this.closeMenu(); (anchor?.isConnected ? anchor : this.tabs.querySelector<HTMLElement>('[aria-selected="true"]'))?.focus({ preventScroll: true }); }
      return;
    }
    if ((event.target as HTMLElement).matches("[data-resize]")) {
      const w = this.current(); if (!w || !["ArrowLeft", "ArrowRight"].includes(event.key)) return;
      event.preventDefault(); this.finishResize(true); const side = (event.target as HTMLElement).dataset.resize as PanelSide;
      const delta = (event.key === "ArrowRight" ? 16 : -16) * (side === "left" ? 1 : -1);
      const initial = this.components.panelWidth(w.context.workspaceId, side);
      const width = initial === undefined ? undefined : this.components.panelWidth(w.context.workspaceId, side, initial + delta);
      if (width !== undefined && width !== initial) this.setWidth(w.context.workspaceId, side, width);
      return;
    }
    if (!this.tabs.contains(event.target as Node) && (event.target as HTMLElement).id !== "catalog-tab") return;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault(); const nodes = [...document.querySelectorAll<HTMLElement>('#catalog-tab:not([hidden]), .server-tab-select')];
    const current = nodes.indexOf(document.activeElement as HTMLElement);
    const index = event.key === "Home" ? 0 : event.key === "End" ? nodes.length - 1 : (current + (event.key === "ArrowLeft" ? -1 : 1) + nodes.length) % nodes.length;
    nodes[index]?.click(); nodes[index]?.focus({preventScroll:true}); this.strip?.reveal(nodes[index]?.dataset.id);
  }
  private down(event: PointerEvent) {
    if (this.menu || document.querySelector('dialog[open], .modal-backdrop:not([hidden])')) return;
    const node = (event.target as Element).closest<HTMLElement>('.server-tab');
    if (!node || (event.target as Element).closest('.tab-close-button')) return;
    const workspace = this.state?.workspaces.find(w => w.context.workspaceId === node.dataset.id); if (!workspace) return;
    const context = JSON.stringify(workspace.context);
    this.finishResize(true); this.components.cancelSizeGesture();
    this.drag.begin(event, {
      workspaceId: workspace.context.workspaceId, scope: { kind: 'host' }, trigger: node.querySelector<HTMLElement>('.server-tab-select')!,
      label: workspace.name, threshold: 8,
      valid: () => !this.disposed && !this.content.inert && Boolean(this.state?.workspaces.some(w => w.context.workspaceId === workspace.context.workspaceId && JSON.stringify(w.context) === context)),
      start: () => this.closeMenu(), prepare: async () => { await this.flushLayouts(); await this.components.flush(); },
      confirm: async (token, operation) => { this.moving.add(workspace.context.workspaceId); try { await this.components.confirmMove(token, operation); } finally { this.moving.delete(workspace.context.workspaceId); } },
    });
  }
  receiveDrag(event: DragEvent) { this.drag.receive(event); }
  private renderDrop(target: DropPreview) {
    if (!this.components) return;
    this.preview = target;
    this.strip?.drag(target.kind === 'strip' && target.windowId === this.state?.window.id ? target.point ?? undefined : undefined);
    this.components.previewDrop(target); this.paintPreview();
  }
  private paintPreview() {
    this.tabs.querySelector(".tab-insertion")?.remove();
    const target = this.preview; if (target?.kind !== "strip" || target.windowId !== this.state?.window.id) return;
    const tabs = [...this.tabs.querySelectorAll<HTMLElement>(".server-tab")].filter(t => t.dataset.id !== (this.drag.source?.scope.kind === 'host' ? this.drag.source.workspaceId : undefined));
    const next = tabs[target.index]; const last = tabs.at(-1); const rect = this.tabs.getBoundingClientRect();
    const left = (next?.getBoundingClientRect().left ?? last?.getBoundingClientRect().right ?? rect.left) - rect.left + this.tabs.scrollLeft;
    const line = document.createElement("span"); line.className = "tab-insertion"; line.style.left = `${left}px`; this.tabs.append(line);
  }
  private report() { if (!this.reportTimer) this.reportTimer = setTimeout(() => { this.reportTimer = undefined; void this.reportNow().catch(() => {}); }, 30); }
  async settleDrop(event: DragEvent) { this.drag.receive(event); await this.reportNow(event.token); }
  private async reportNow(dragToken?:string) {
    // An empty strip takes no visible space; the title-bar blank area still accepts dragged tabs.
    const target = this.tabs.querySelector('.server-tab') ? this.tabs : document.querySelector('#drag-region') ?? this.tabs;
    const rect = target.getBoundingClientRect();
    await this.call("workspace_surface", { dragToken, surface: { x: rect.x, y: rect.y, width: rect.width, height: rect.height, groups:this.components.dropZones(),docks:this.components.dockZones(),blocked: Boolean(document.querySelector('dialog[open], #overlay:not([hidden])')), tabs: [...this.tabs.querySelectorAll<HTMLElement>(".server-tab")].map(tab => { const r = tab.getBoundingClientRect(); return { id: tab.dataset.id!, center: r.x + r.width / 2 }; }) } });
  }
  private resize(event: PointerEvent) {
    const handle = (event.target as Element).closest<HTMLElement>("[data-resize]");
    if (handle) void this.drag.cancel(); const w = this.current();
    if (!handle || !w || event.button !== 0 || !event.isPrimary || this.panelGesture || this.moving.has(w.context.workspaceId)) return;
    const side = handle.dataset.resize as PanelSide;
    if (this.components.panelWidth(w.context.workspaceId, side) === undefined) return;
    const initial = handle.closest(".server-workbench")!.querySelector(`.${side}-panel`)!.getBoundingClientRect().width;
    event.preventDefault(); handle.setPointerCapture(event.pointerId);
    const focus = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    handle.focus({ preventScroll: true });
    const otherWidth = handle.closest(".server-workbench")!.querySelector(`.${side === "left" ? "right" : "left"}-panel`)!.getBoundingClientRect().width;
    this.panelGesture = { id: w.context.workspaceId, side, pointer: event.pointerId, x: event.clientX, initial, width: initial, otherWidth, handle, focus };
    document.body.classList.add("panel-resizing");
  }
  private resizeMove(event: PointerEvent) {
    const g = this.panelGesture; if (!g || g.pointer !== event.pointerId) return;
    const width = this.components.panelWidth(g.id, g.side, g.initial + (event.clientX - g.x) * (g.side === "left" ? 1 : -1), g.otherWidth);
    if (width === undefined) { this.finishResize(true); return; }
    g.width = width; this.components.scheduleLayout(g.id);
  }
  private finishResize(cancel: boolean, event?: PointerEvent) {
    const g = this.panelGesture; if (!g) return;
    if (event) this.resizeMove(event);
    if (this.panelGesture !== g) return;
    this.panelGesture = undefined; document.body.classList.remove("panel-resizing");
    if (g.handle.hasPointerCapture(g.pointer)) g.handle.releasePointerCapture(g.pointer);
    if (document.hasFocus() && document.activeElement === g.handle && g.focus?.isConnected) g.focus.focus({ preventScroll: true });
    if (!cancel && g.width !== g.initial && !this.disposed) this.setWidth(g.id, g.side, g.width);
    this.components.scheduleLayout(g.id);
  }
  private displayWidths(workspace: Workspace): PanelWidths {
    const id = workspace.context.workspaceId, values = this.widths.get(id)?.values;
    const widths: PanelWidths = { leftWidth: values?.left?.width ?? workspace.layout.leftWidth, rightWidth: values?.right?.width ?? workspace.layout.rightWidth };
    const g = this.panelGesture;
    if (g?.id === id) { widths[`${g.side}Width`] = g.width; widths.drag = { side: g.side, otherWidth: g.otherWidth }; }
    return widths;
  }
  private setWidth(id: string, side: PanelSide, width: number) {
    let state = this.widths.get(id);
    if (!state) { state = { values: {} }; this.widths.set(id, state); }
    state.values[side] = { width, sequence: ++this.widthSequence }; state.failed = false;
    this.components.scheduleLayout(id);
    if (!state.flight) this.startSave(id, state);
  }
  private startSave(id: string, state: WidthState) {
    state.flight = this.saveWidths(id, state).finally(() => {
      state.flight = undefined;
      if (this.disposed || this.widths.get(id) !== state) return;
      if (Object.keys(state.values).length && !state.failed) this.startSave(id, state);
      else if (!state.failed) this.widths.delete(id);
    });
  }
  private async saveWidths(id: string, pending: WidthState) {
    while (!this.disposed && this.widths.get(id) === pending && !pending.failed) {
      const next = Object.entries(pending.values)[0] as [PanelSide, WidthChange] | undefined; if (!next) break;
      const [side, change] = next;
      try {
        const result = await this.call<{ revision: number; layout: Layout }>("workspace_layout", { id, side, width: change.width });
        if (this.disposed || this.widths.get(id) !== pending) return;
        if (this.state && result.revision >= this.state.revision) {
          const workspace = this.state.workspaces.find(w => w.context.workspaceId === id);
          if (workspace) workspace.layout = result.layout;
          this.state.revision = result.revision;
        }
      } catch (error) {
        if (this.disposed || this.widths.get(id) !== pending) return;
        try { await this.refresh(); } catch { pending.failed = true; }
        const confirmed = this.state?.workspaces.find(w => w.context.workspaceId === id)?.layout[`${side}Width`];
        if (pending.failed || confirmed !== change.width) this.notify(`${pending.failed ? "面板宽度保存状态未确认" : "面板宽度未保存"}：${String(error)}`);
      }
      if (this.disposed || this.widths.get(id) !== pending) return;
      if (!pending.failed && pending.values[side]?.sequence === change.sequence) delete pending.values[side];
      this.components.scheduleLayout(id);
    }
    if (!this.disposed) await this.refresh().catch(() => {});
  }
  private async flushLayouts() {
    while ([...this.widths.values()].some(state => state.flight)) await Promise.all([...this.widths.values()].map(state => state.flight));
    if ([...this.widths.values()].some(state => state.failed)) throw new Error("面板宽度保存状态未确认，请重试");
  }
  dispose() {
    if(this.disposed)return;
    this.drag.dispose();this.finishResize(true);this.disposed=true;this.widths.clear();this.events.abort();clearTimeout(this.reportTimer);
    this.observer.disconnect();this.resizeObserver.disconnect();this.closeMenu();this.strip?.dispose();
    this.tabRegion.dispose();this.closer.dispose();this.components.dispose();
  }
  private hostBlock?: (blocked:boolean)=>void;
  setCloseParticipants(inputs:HostInputs,block:(blocked:boolean)=>void){this.closer.hostInputs=inputs;this.hostBlock=block;}
  exit() { this.finishResize(true); return this.closer.request({kind:"application",action:"exit"}); }
  canBackground(){return !this.drag.busy&&!this.moving.size&&this.components.canBackground()&&!document.querySelector('dialog[open], .workspace[inert]');}
  restart(install:boolean) { this.finishResize(true); return this.closer.request({kind:"application",action:install?"installAndRestart":"restart"}); }
  async flush() { this.finishResize(true); await this.flushLayouts(); return this.components.flush(); }
  closeWindow() { this.finishResize(true); return this.closer.request({kind:"window",id:this.state?.window.id??"main"}); }
}
