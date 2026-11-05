import { invoke } from "@tauri-apps/api/core";
import { escape as e, icon, replace } from "./ui";
import "./workspaces.css";
import { Components } from "./components";

export interface Context { workspaceId: string; serverId: string; connectionId: string; generation: number }
export interface Layout { leftOpen: boolean; rightOpen: boolean; leftWidth: number; rightWidth: number }
export interface Workspace { context: Context; name: string; endpoint: string; connected: boolean; windowId: string; layout: Layout; viewIds: string[]; activeViewId: string | null }
export interface Projection { revision: number; window: { id: string; role: string; workspaceIds: string[]; activeWorkspace: string | null; ready: boolean }; workspaces: Workspace[]; incoming: Workspace | null }
export type DropTarget = { kind: "strip"; windowId: string; index: number } | { kind: "outside"; x: number; y: number } | { kind: "invalid" };
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
interface Gesture { id: string; pointer: number; x: number; y: number; grabX: number; grabY: number; token?: string; starting: boolean; released: boolean; cancelled: boolean; node: HTMLElement; capture: HTMLElement; ghost?: HTMLElement; sampling: boolean }

/** Each view is rendered from its own immutable context; no global active server API. */
export class Workspaces {
  private components: Components;
  private state?: Projection;
  private refreshing?: Promise<void>;
  private again = false;
  private menu?: HTMLElement;
  private gesture?: Gesture;
  private moving = new Set<string>();
  private disposed = false;
  private reportTimer?: ReturnType<typeof setTimeout>;
  private preview?: DropTarget;
  private observer: MutationObserver;
  private resizeObserver: ResizeObserver;

  constructor(private tabs: HTMLElement, private content: HTMLElement, private catalog: HTMLElement, private notify: (message: string) => void, private call: Invoke = invoke) {
    this.components = new Components(content, call, notify, () => this.refresh());
    tabs.addEventListener("click", event => {
      if (this.gesture?.token) return;
      const button = (event.target as Element).closest<HTMLElement>("[data-workspace-action]");
      if (button) void this.run(() => this.action(button.dataset.workspaceAction!, button.dataset.id!));
    });
    tabs.addEventListener("contextmenu", event => {
      const tab = (event.target as Element).closest<HTMLElement>(".server-tab");
      if (!tab) return; event.preventDefault(); this.openMenu(tab.dataset.id!, tab);
    });
    tabs.addEventListener("pointerdown", event => this.down(event));
    tabs.addEventListener("pointermove", event => this.pointerMove(event));
    tabs.addEventListener("pointerup", event => { if (event.pointerId === this.gesture?.pointer) void this.finish(false); });
    tabs.addEventListener("pointercancel", () => void this.finish(true));
    tabs.addEventListener("lostpointercapture", () => { if (this.gesture && !this.gesture.released) void this.finish(true); });
    tabs.addEventListener("scroll", () => this.report(), { passive: true });
    document.querySelector("#catalog-tab")?.addEventListener("click", () => void this.run(() => this.call("workspace_activate", { id: null })));
    document.addEventListener("pointerdown", event => { if (!this.menu?.contains(event.target as Node)) this.closeMenu(); });
    document.addEventListener("keydown", event => this.key(event));
    content.addEventListener("pointerdown", event => this.resize(event));
    this.observer = new MutationObserver(() => this.report());
    this.observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open", "hidden", "inert"] });
    this.resizeObserver = new ResizeObserver(() => this.report()); this.resizeObserver.observe(tabs);
  }
  async start() { await this.refresh(); await this.reportNow(); await this.call("workspace_ready"); }
  refresh(): Promise<void> {
    if (this.refreshing) { this.again = true; return this.refreshing; }
    this.refreshing = (async () => {
      do {
        this.again = false;
        const state = await this.call<Projection>("workspace_snapshot");
        if (!this.state || state.revision >= this.state.revision) { this.state = state; await this.components.sync(state); this.render(); }
      } while (this.again);
    })().finally(() => { this.refreshing = undefined; });
    return this.refreshing;
  }
  private current() { return this.state?.workspaces.find(w => w.context.workspaceId === this.state?.window.activeWorkspace); }
  private async run(fn: () => Promise<unknown>) { try { await fn(); } catch (error) { this.notify(String(error)); } finally { if (!this.disposed) await this.refresh().catch(error => this.notify(String(error))); } }
  private render() {
    const state = this.state!;
    const main = state.window.role === "main";
    const fixed = document.querySelector<HTMLElement>("#catalog-tab");
    if (fixed) { fixed.hidden = !main; fixed.setAttribute("aria-selected", String(!state.window.activeWorkspace)); fixed.tabIndex = state.window.activeWorkspace ? -1 : 0; }
    const focused = document.activeElement as HTMLElement;
    const focusId = this.tabs.contains(focused) ? focused.closest<HTMLElement>(".server-tab")?.dataset.id : undefined;
    const focusAction = focused?.dataset.workspaceAction;
    const scroll = this.tabs.scrollLeft;
    replace(this.tabs, state.workspaces.map(w => {
      const id = e(w.context.workspaceId), active = state.window.activeWorkspace === w.context.workspaceId;
      return `<div class="server-tab ${active ? "selected" : ""} ${this.moving.has(w.context.workspaceId) ? "moving" : ""}" data-id="${id}"><button class="server-tab-select" role="tab" aria-selected="${active}" tabindex="${active ? 0 : -1}" data-workspace-action="select" data-id="${id}" title="${e(w.name)} · ${e(w.endpoint)}"><span class="health-dot ${w.connected ? "good" : "warning"}"></span><span class="tab-name">${e(w.name)}</span></button><button class="tab-menu-button" data-workspace-action="menu" data-id="${id}" aria-label="${e(w.name)} 的菜单" title="工作区菜单">⋯</button><button class="tab-close-button" data-workspace-action="close" data-id="${id}" aria-label="关闭 ${e(w.name)}" title="关闭工作区">${icon("close")}</button></div>`;
    }).join(""));
    this.tabs.scrollLeft = scroll;
    if (focusId) this.tabs.querySelector<HTMLElement>(`[data-id="${CSS.escape(focusId)}"] [data-workspace-action="${focusAction ?? "select"}"]`)?.focus({ preventScroll: true });
    const w = this.current();
    this.catalog.hidden = !main || Boolean(w);
    this.content.hidden = !w && main && !state.incoming;
    if (w) {
      const layout = w.layout;
      this.content.dataset.workspaceId = w.context.workspaceId;
      this.content.inert = this.moving.has(w.context.workspaceId);
      this.content.style.setProperty("--left-width", `${layout.leftWidth}px`);
      this.content.style.setProperty("--right-width", `${layout.rightWidth}px`);
    }
    this.paintPreview(); this.report();
  }
  private async action(action: string, id: string) {
    if (action === "menu") { this.openMenu(id, this.tabs.querySelector<HTMLElement>(`.server-tab[data-id="${CSS.escape(id)}"]`)!); return; }
    this.closeMenu();
    if (action === "select") { await this.call("workspace_activate", { id }); return; }
    if (action === "close") { if (await this.components.close([id])) await this.call("workspace_close", { id }); return; }
    if (this.moving.has(id)) return;
    this.moving.add(id); this.render();
    try { await this.components.flush(); await this.call("workspace_move", { id, target: action === "return" ? "main" : null, index: Number.MAX_SAFE_INTEGER }); }
    finally { this.moving.delete(id); }
  }
  private openMenu(id: string, anchor: HTMLElement) {
    this.closeMenu();
    if (!anchor || !this.state?.workspaces.some(w => w.context.workspaceId === id)) return;
    const node = document.createElement("div"); node.className = "workspace-menu"; node.setAttribute("role", "menu");
    node.innerHTML = `<button role="menuitem" data-choice="detach">${icon("maximize")}移至新窗口</button>${this.state.window.id !== "main" ? `<button role="menuitem" data-choice="return">${icon("arrow")}返回主窗口</button>` : ""}<button role="menuitem" data-choice="close">${icon("close")}关闭工作区</button>`;
    const rect = anchor.getBoundingClientRect(); node.style.left = `${Math.max(4, Math.min(rect.left, innerWidth - 190))}px`; node.style.top = `${rect.bottom + 4}px`;
    node.addEventListener("click", event => { const action = (event.target as Element).closest<HTMLElement>("[data-choice]")?.dataset.choice; if (action) void this.run(() => this.action(action, id)); });
    node.addEventListener("keydown", event => {
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return; event.preventDefault();
      const buttons = [...node.querySelectorAll("button")]; const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    });
    document.body.append(node); this.menu = node; node.querySelector("button")?.focus();
  }
  private closeMenu() { this.menu?.remove(); this.menu = undefined; }
  private key(event: KeyboardEvent) {
    if (event.key === "Escape") {
      if (this.gesture) { event.preventDefault(); void this.finish(true); }
      for (const id of this.moving) void this.call("workspace_cancel_move", { id }).catch(error => this.notify(String(error)));
      if (this.menu) { event.preventDefault(); this.closeMenu(); this.tabs.querySelector<HTMLElement>('[aria-selected="true"]')?.focus(); }
      return;
    }
    if ((event.target as HTMLElement).matches("[data-resize]")) {
      const w = this.current(); if (!w || !["ArrowLeft", "ArrowRight"].includes(event.key)) return;
      event.preventDefault(); const side = (event.target as HTMLElement).dataset.resize!; const key = side === "left" ? "leftWidth" : "rightWidth";
      const delta = (event.key === "ArrowRight" ? 16 : -16) * (side === "left" ? 1 : -1);
      void this.run(() => this.call("workspace_layout", { id: w.context.workspaceId, layout: { ...w.layout, [key]: Math.max(160, Math.min(480, w.layout[key] + delta)) } })); return;
    }
    if (!this.tabs.contains(event.target as Node) && (event.target as HTMLElement).id !== "catalog-tab") return;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault(); const nodes = [...document.querySelectorAll<HTMLElement>('#catalog-tab:not([hidden]), .server-tab-select')];
    const current = nodes.indexOf(document.activeElement as HTMLElement);
    const index = event.key === "Home" ? 0 : event.key === "End" ? nodes.length - 1 : (current + (event.key === "ArrowLeft" ? -1 : 1) + nodes.length) % nodes.length;
    nodes[index]?.click(); nodes[index]?.focus(); nodes[index]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
  private down(event: PointerEvent) {
    if (event.button !== 0 || this.gesture || this.menu || document.querySelector('dialog[open], .modal-backdrop:not([hidden])')) return;
    const node = (event.target as Element).closest<HTMLElement>(".server-tab");
    if (!node || (event.target as Element).closest(".tab-close-button, .tab-menu-button")) return;
    const rect = node.getBoundingClientRect();
    const capture = node.querySelector<HTMLElement>(".server-tab-select")!;
    this.gesture = { id: node.dataset.id!, pointer: event.pointerId, x: event.clientX, y: event.clientY, grabX: event.clientX - rect.left, grabY: event.clientY, starting: false, released: false, cancelled: false, node, capture, sampling: false };
    capture.setPointerCapture(event.pointerId);
  }
  private pointerMove(event: PointerEvent) {
    const g = this.gesture; if (!g || event.pointerId !== g.pointer || g.released) return;
    if (g.ghost) { g.ghost.style.left = `${event.clientX - g.grabX}px`; g.ghost.style.top = `${event.clientY - g.grabY + 9}px`; }
    if (g.token || g.starting || Math.hypot(event.clientX - g.x, event.clientY - g.y) < 8) return;
    g.starting = true; this.closeMenu();
    const ghost = document.createElement("div"); ghost.className = "workspace-drag-ghost"; ghost.textContent = g.node.querySelector(".tab-name")?.textContent ?? "工作区"; ghost.style.left = `${event.clientX - g.grabX}px`; ghost.style.top = `${event.clientY - g.grabY + 9}px`; document.body.append(ghost); g.ghost = ghost; g.node.classList.add("dragging");
    void this.call<string>("workspace_drag_start", { id: g.id, offsetX: g.grabX, offsetY: g.grabY }).then(token => { g.token = token; if (g.released) void this.completeGesture(g); else void this.sample(g); }).catch(error => { this.notify(String(error)); this.clearGesture(g); });
  }
  private async sample(g: Gesture) {
    if (g.released || !g.token || g.sampling) return;
    g.sampling = true;
    try { const target = await this.call<DropTarget>("workspace_drag_sample", { token: g.token }); this.previewDrop(target); if (g.ghost) g.ghost.dataset.drop = target.kind; }
    catch (error) { this.notify(String(error)); await this.finish(true); }
    finally { g.sampling = false; }
    if (!g.released) setTimeout(() => void this.sample(g), 32);
  }
  private async finish(cancel: boolean) {
    const g = this.gesture; if (!g || g.released) return;
    g.released = true; g.cancelled = cancel;
    if (g.token) await this.completeGesture(g);
    else if (!g.starting) this.clearGesture(g);
  }
  private async completeGesture(g: Gesture) {
    this.moving.add(g.id);
    try { await this.components.flush(); await this.call("workspace_drag_finish", { token: g.token, cancel: g.cancelled }); }
    catch (error) { this.notify(String(error)); }
    finally { this.moving.delete(g.id); this.clearGesture(g); await this.refresh().catch(() => {}); }
  }
  private clearGesture(g: Gesture) {
    g.ghost?.remove(); g.node.classList.remove("dragging"); if (g.capture.hasPointerCapture(g.pointer)) g.capture.releasePointerCapture(g.pointer);
    if (this.gesture === g) this.gesture = undefined; this.previewDrop({ kind: "invalid" });
  }
  previewDrop(target: DropTarget) { this.preview = target; this.paintPreview(); }
  private paintPreview() {
    this.tabs.querySelector(".tab-insertion")?.remove();
    const target = this.preview; if (target?.kind !== "strip" || target.windowId !== this.state?.window.id) return;
    const tabs = [...this.tabs.querySelectorAll<HTMLElement>(".server-tab")].filter(t => t.dataset.id !== this.gesture?.id);
    const next = tabs[target.index]; const last = tabs.at(-1); const rect = this.tabs.getBoundingClientRect();
    const left = (next?.getBoundingClientRect().left ?? last?.getBoundingClientRect().right ?? rect.left) - rect.left + this.tabs.scrollLeft;
    const line = document.createElement("span"); line.className = "tab-insertion"; line.style.left = `${left}px`; this.tabs.append(line);
  }
  private report() { if (!this.reportTimer) this.reportTimer = setTimeout(() => { this.reportTimer = undefined; void this.reportNow().catch(() => {}); }, 30); }
  private async reportNow() {
    const rect = this.tabs.getBoundingClientRect();
    await this.call("workspace_surface", { surface: { x: rect.x, y: rect.y, width: rect.width, height: rect.height, blocked: Boolean(document.querySelector('dialog[open], #overlay:not([hidden])')), tabs: [...this.tabs.querySelectorAll<HTMLElement>(".server-tab")].map(tab => { const r = tab.getBoundingClientRect(); return { id: tab.dataset.id!, center: r.x + r.width / 2 }; }) } });
  }
  private resize(event: PointerEvent) {
    const handle = (event.target as Element).closest<HTMLElement>("[data-resize]"); const w = this.current();
    if (!handle || !w || event.button !== 0) return; event.preventDefault();
    const side = handle.dataset.resize!, key = side === "left" ? "leftWidth" : "rightWidth", x = event.clientX, initial = w.layout[key];
    let width = initial; handle.setPointerCapture(event.pointerId);
    const move = (ev: PointerEvent) => { width = Math.max(160, Math.min(480, Math.round(initial + (ev.clientX - x) * (side === "left" ? 1 : -1)))); this.content.querySelector<HTMLElement>(`.server-workbench[data-id="${CSS.escape(w.context.workspaceId)}"]`)?.style.setProperty(`--${side}-width`, `${width}px`); };
    const end = (ev: PointerEvent) => { handle.removeEventListener("pointermove", move); handle.removeEventListener("pointerup", end); handle.removeEventListener("pointercancel", end); if (handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId); void this.run(() => this.call("workspace_layout", { id: w.context.workspaceId, layout: { ...w.layout, [key]: ev.type === "pointercancel" ? initial : width } })); };
    handle.addEventListener("pointermove", move); handle.addEventListener("pointerup", end); handle.addEventListener("pointercancel", end);
  }
  allowExit() { return this.components.allowExit(); }
  flush() { return this.components.flush(); }
  async closeWindow() { if (await this.components.close(this.state?.workspaces.map(w => w.context.workspaceId) ?? [])) await this.call("workspace_close_window"); }
}
