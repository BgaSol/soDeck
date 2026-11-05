import { convertFileSrc } from "@tauri-apps/api/core";
import { ComponentData } from "./component-data";
import { escape as e, icon, replace } from "./ui";
import type { Context, Projection, Workspace } from "./workspaces";
import type { NetworkSnapshot } from "./core";

type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
interface Definition { key: string; id: string; pluginId: string; pluginName: string; pluginInstanceId: string; title: string; icon: string; container: string; instancePolicy: string; entry: string; available: boolean; createAction?: string; locateEntry: boolean }
interface Preference { side: string; pinned: boolean; startup: string; order: number }
interface View { id: string; definitionKey: string; pluginInstanceId: string; context: Context; title: string; dirty: boolean; busy: boolean; error?: string; lastUsed: number }
interface UiWorkspace { views: View[]; preferences: Record<string, Preference>; left: string | null; right: string | null; editor: string | null }
export interface UiProjection { definitions: Record<string, Definition>; workspaces: Record<string, UiWorkspace>; error?: string }
interface Surface { node: HTMLDivElement; frame: HTMLIFrameElement; view: View; token?: string; chain: Promise<unknown>; visible: boolean; initialized: boolean; focusPending?: number; flushable?: boolean; flushing?: {id: string; done: (error?: string) => void}; timeout: ReturnType<typeof setTimeout> }
interface Shell { node: HTMLElement; surfaces: Map<string, Surface>; observer: ResizeObserver; workspace: Workspace; ui: UiWorkspace; railCapacity?: number }
interface ClosePlan { token: string; views: string[]; dirty: { id: string; title: string; serverName: string; dirty: boolean; busy: boolean; error?: string }[] }
const empty = (): UiWorkspace => ({ views: [], preferences: {}, left: null, right: null, editor: null });

/** Shell and frames have different lifetimes. Rendering chrome never reconstructs a mounted iframe. */
export class Components {
  private data: ComponentData;
  private shells = new Map<string, Shell>();
  private definitions: Record<string, Definition> = {};
  private state?: Projection;
  private menu?: HTMLElement;
  private dialog?: HTMLDialogElement;
  private disposed = false;
  private layoutError?: string;
  private focusEpoch = 0;
  private interaction = () => { this.focusEpoch++; };
  private theme = new MutationObserver(() => this.broadcast());
  private networkTimer = setInterval(() => { void this.updateNetwork(); }, 1000);
  private networkPending = false;
  constructor(private content: HTMLElement, private call: Call, private notify: (message: string) => void, private refresh: () => Promise<void>) {
    this.data = new ComponentData(call);
    window.addEventListener("message", this.message);
    this.theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", this.systemTheme);
    content.addEventListener("click", event => {
      const button = (event.target as Element).closest<HTMLElement>("[data-component-action]");
      const shell = button?.closest<HTMLElement>(".server-workbench");
      if (button && shell) void this.run(() => this.action(shell.dataset.id!, button.dataset.componentAction!, button.dataset.key ?? "", button));
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
    if (ui.error && ui.error !== this.layoutError) { this.layoutError = ui.error; this.notify(ui.error); }
    const wanted = [...state.workspaces, ...(state.incoming ? [state.incoming] : [])];
    for (const [id, shell] of this.shells) if (!wanted.some(w => w.context.workspaceId === id)) { this.destroy(shell); this.shells.delete(id); }
    this.content.querySelector(".receiving-workspace")?.remove();
    for (const w of wanted) {
      const id = w.context.workspaceId;
      let shell = this.shells.get(id);
      if (!shell) {
        const node = document.createElement("section"); node.className = "server-workbench"; node.dataset.id = id;
        node.innerHTML = `<aside class="workbench-rail left-rail" aria-label="左侧组件"></aside><aside class="workbench-panel left-panel"><header></header><div class="component-slot"></div><div role="separator" aria-label="左侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="left"></div></aside><section class="workbench-center"><div class="component-tabs"></div><div class="component-slot central-slot"></div></section><aside class="workbench-panel right-panel"><header></header><div class="component-slot"></div><div role="separator" aria-label="右侧面板宽度" aria-orientation="vertical" tabindex="0" data-resize="right"></div></aside><aside class="workbench-rail right-rail" aria-label="右侧组件"></aside><footer class="workspace-status"></footer><div class="component-surfaces"></div>`;
        const observer = new ResizeObserver(() => this.position(shell!)); observer.observe(node);
        node.querySelectorAll(".component-slot").forEach(slot => observer.observe(slot));
        shell = { node, surfaces: new Map(), observer, workspace: w, ui: empty() }; this.shells.set(id, shell); this.content.append(node);
      }
      shell.workspace = w; shell.ui = ui.workspaces[id] ?? empty();
      const incoming = state.incoming?.context.workspaceId === id;
      shell.node.hidden = !incoming && state.window.activeWorkspace !== id;
      shell.node.classList.toggle("incoming-component-workspace", incoming);
      shell.node.setAttribute("aria-label", `${w.name} 的工作区`);
      shell.node.style.setProperty("--left-width", `${w.layout.leftWidth}px`); shell.node.style.setProperty("--right-width", `${w.layout.rightWidth}px`);
      this.paint(shell);
    }
    if (!wanted.length) { const placeholder = document.createElement("div"); placeholder.className = "workspace-empty receiving-workspace"; placeholder.textContent = "此窗口没有工作区"; this.content.append(placeholder); }
  }
  private paint(shell: Shell) {
    const { node, ui: w } = shell;
    node.classList.toggle("left-open", Boolean(w.left)); node.classList.toggle("right-open", Boolean(w.right));
    for (const side of ["left", "right"]) {
      const selected = side === "left" ? w.left : w.right;
      const entries = Object.values(this.definitions).filter(d => w.preferences[d.key]?.side === side && (d.container === "sidebar" || d.locateEntry)).sort((a,b) => (w.preferences[a.key].order - w.preferences[b.key].order) || a.key.localeCompare(b.key));
      replace(node.querySelector(`.${side}-rail`)!, entries.filter(d => w.preferences[d.key].pinned).slice(0, shell.railCapacity ?? 20).map(d => {
        const active = d.container === "sidebar" ? w.views.some(v => v.id === selected && v.definitionKey === d.key) : w.views.some(v => v.id === w.editor && v.definitionKey === d.key);
        return `<button data-component-action="${d.container === "sidebar" ? "toggle" : "locate"}" data-key="${e(d.key)}" aria-label="${e(d.container === "sidebar" ? d.title : `定位${d.title}`)}" aria-expanded="${active}" title="${e(d.title)} · ${e(d.pluginName)}" ${!d.available ? "disabled" : ""}>${icon(d.icon)}</button>`;
      }).join("") + `<button class="component-more" data-component-action="more" data-key="${side}" aria-label="${side === "left" ? "左" : "右"}侧更多组件">⋯</button>`);
      const view = w.views.find(v => v.id === selected);
      replace(node.querySelector(`.${side}-panel header`)!, view ? `<span title="${e(this.definitions[view.definitionKey]?.pluginName)}">${e(view.title)}</span><div><button data-component-action="panel-menu" data-key="${e(view.definitionKey)}" aria-label="${e(view.title)} 面板菜单">⋯</button><button data-component-action="collapse" data-key="${side}" aria-label="收起${e(view.title)}">${icon("minus")}</button></div>` : "");
    }
    const editors = w.views.filter(v => this.definitions[v.definitionKey]?.container === "editor");
    replace(node.querySelector(".component-tabs")!, `<div class="component-tab-list" role="tablist" aria-label="组件页面">${editors.map(v => `<div class="component-tab ${w.editor === v.id ? "selected" : ""}" draggable="true" data-view="${e(v.id)}"><button role="tab" aria-selected="${w.editor === v.id}" data-component-action="select" data-key="${e(v.id)}" title="${e(v.title)}">${icon(this.definitions[v.definitionKey]?.icon)}<span>${e(v.title)}${v.dirty ? " ●" : ""}</span></button><button data-component-action="close" data-key="${e(v.id)}" aria-label="关闭 ${e(v.title)}">${icon("close")}</button></div>`).join("")}</div><button data-component-action="create" aria-label="新建组件页面" title="新建组件页面">＋</button><button data-component-action="pages" aria-label="全部组件页面" title="全部组件页面">⌄</button>`);
    const list = node.querySelector(".component-tab-list")!;
    list.addEventListener("dragstart", event => { const ev = event as DragEvent; const id = (ev.target as Element).closest<HTMLElement>("[data-view]")?.dataset.view; if (id) ev.dataTransfer?.setData("text/sodeck-view", id); });
    list.addEventListener("dragover", event => event.preventDefault());
    list.addEventListener("drop", event => { event.preventDefault(); const ev = event as DragEvent, id = ev.dataTransfer?.getData("text/sodeck-view"), target = (ev.target as Element).closest<HTMLElement>("[data-view]")?.dataset.view; if (id && target) void this.run(() => this.perform(shell, { op: "reorder", viewId: id, index: w.views.findIndex(v => v.id === target) })); });
    const slot = node.querySelector<HTMLElement>(".central-slot")!;
    if (!editors.length) replace(slot, `<div class="workspace-empty"><div class="empty-icon">${icon("plug")}</div><h2>${e(shell.workspace.name)}</h2><p>${Object.keys(this.definitions).length ? "从侧栏打开组件，或点击＋新建页面" : "安装并启用带界面的插件后，可在这里使用组件"}</p><div class="workspace-context">${e(shell.workspace.endpoint)}</div></div>`); else replace(slot, "");
    replace(node.querySelector(".workspace-status")!, `<span class="health-dot ${shell.workspace.connected ? "good" : "warning"}"></span><span>${shell.workspace.connected ? "已连接" : "连接已断开"}</span><span class="workspace-endpoint">${e(shell.workspace.endpoint)}</span><button data-disconnect-context="${e(JSON.stringify(shell.workspace.context))}" ${shell.workspace.connected ? "" : "hidden"}>断开</button><button data-reconnect-server="${e(shell.workspace.context.serverId)}" ${shell.workspace.connected ? "hidden" : ""}>重新连接</button><span class="workspace-owner">${this.state?.window.role === "main" ? "主窗口" : "独立窗口"}</span>`);
    const network=document.createElement("button");network.className="workspace-network";network.dataset.action="status";network.title="服务器连接传输速率；点击查看应用状态与插件业务量";network.textContent="↓ 0 B/s ↑ 0 B/s";node.querySelector(".workspace-status")!.append(network);
    for (const [id, surface] of shell.surfaces) if (!w.views.some(v => v.id === id)) { this.removeSurface(surface); shell.surfaces.delete(id); }
    for (const v of w.views) {
      const d = this.definitions[v.definitionKey]; if (!d) continue;
      let surface = shell.surfaces.get(v.id);
      if (surface && (surface.view.context.connectionId !== v.context.connectionId || surface.view.context.generation !== v.context.generation)) {
        this.post(surface, {type: "contextChanged", context: v.context});
      }
      if (!surface) {
        const host = document.createElement("div"); host.className = "component-surface"; host.dataset.viewId = v.id;
        const frame = document.createElement("iframe"); frame.title = `${v.title} · ${shell.workspace.name}`;
        frame.src = new URL(`${d.pluginId}/${d.entry}`, convertFileSrc("", "sodeck-plugin")).href;
        host.append(frame); node.querySelector(".component-surfaces")!.append(host);
        surface = { node: host, frame, view: v, chain: Promise.resolve(), visible: false, initialized: false, timeout: setTimeout(() => this.surfaceError(surface!, "组件界面未在 8 秒内就绪"), 8000) };
        shell.surfaces.set(v.id, surface);
      }
      surface.view = v;
      surface.frame.title = `${v.title} · ${shell.workspace.name}`;
      if (!d.available) this.surfaceError(surface, "插件已停止，页面状态仍保留。请查看应用状态与日志。");
    }
    requestAnimationFrame(() => this.position(shell));
  }
  private position(shell: Shell) {
    const r = shell.node.getBoundingClientRect(), w = shell.ui;
    if (r.height > 0) { const capacity = Math.max(1, Math.floor((r.height - 80) / 38)); if (shell.railCapacity !== capacity) { shell.railCapacity = capacity; queueMicrotask(() => this.paint(shell)); return; } }
    const preferredLeft = Number.parseFloat(shell.node.style.getPropertyValue("--left-width")) || 220;
    const preferredRight = Number.parseFloat(shell.node.style.getPropertyValue("--right-width")) || 220;
    let left = Boolean(w.left), right = Boolean(w.right);
    const room = Math.max(0, r.width - 84 - 320);
    if (left && right && preferredLeft + preferredRight > room) {
      const a = w.views.find(v => v.id === w.left), b = w.views.find(v => v.id === w.right);
      left = (a?.lastUsed ?? 0) >= (b?.lastUsed ?? 0); right = !left;
    }
    shell.node.classList.toggle("left-open", left); shell.node.classList.toggle("right-open", right);
    shell.node.style.setProperty("--effective-left-width", `${Math.min(preferredLeft, room)}px`);
    shell.node.style.setProperty("--effective-right-width", `${Math.min(preferredRight, room)}px`);
    for (const surface of shell.surfaces.values()) {
      const id = surface.view.id;
      let slot: HTMLElement | null = null;
      if (w.left === id && left) slot = shell.node.querySelector(".left-panel .component-slot");
      if (w.right === id && right) slot = shell.node.querySelector(".right-panel .component-slot");
      if (w.editor === id) slot = shell.node.querySelector(".central-slot");
      const visible = Boolean(slot && !shell.node.hidden && !shell.node.classList.contains("incoming-component-workspace"));
      surface.node.hidden = !slot;
      if (slot) { const s = slot.getBoundingClientRect(); Object.assign(surface.node.style, { left: `${s.left-r.left}px`, top: `${s.top-r.top}px`, width: `${s.width}px`, height: `${s.height}px` }); }
      if (surface.visible !== visible) { surface.visible = visible; this.post(surface, { type: "visibilityChanged", visible }); }
    }
  }
  private tokens() {
    const css = getComputedStyle(document.documentElement), names = ["canvas", "panel", "subtle", "text", "muted", "border", "accent", "accent-bg", "hover"];
    return Object.fromEntries(names.map(name => [name, css.getPropertyValue(`--${name}`).trim()]));
  }
  private post(surface: Surface, message: Record<string, unknown>) { surface.frame.contentWindow?.postMessage({ channel: "sodeck-ui-v1", surfaceId: surface.token, ...message }, new URL(surface.frame.src).origin === "null" ? "*" : new URL(surface.frame.src).origin); }
  private broadcast() { for (const s of this.shells.values()) for (const f of s.surfaces.values()) this.post(f, { type: "themeChanged", tokens: this.tokens() }); }
  private systemTheme = () => this.broadcast();
  private outside = (event: PointerEvent) => { if (this.menu && !this.menu.contains(event.target as Node)) this.closeMenu(); };
  private message = (event: MessageEvent) => {
    if (!event.data || event.data.channel !== "sodeck-ui-v1") return;
    const surface = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]).find(s => s.frame.contentWindow === event.source);
    if (!surface) return;
    const expectedOrigin = new URL(surface.frame.src).origin;
    if (expectedOrigin !== "null" && event.origin !== expectedOrigin) return;
    const message = event.data;
    if (message.type === "ready") {
      if (surface.initialized) return; surface.initialized = true;
      surface.chain = this.invoke<{ surfaceId: string; data: unknown; view: View }>("attach", { viewId: surface.view.id }).then(result => {
        surface.token = result.surfaceId; this.post(surface, { type: "mount", context: result.view.context, serverName: this.shells.get(surface.view.context.workspaceId)?.workspace.name, viewId: surface.view.id, viewType: surface.view.definitionKey.split(":")[1], data: result.data, tokens: this.tokens(), visible: surface.visible });
        surface.frame.dataset.surfaceId = result.surfaceId;
      }).catch(error => this.surfaceError(surface, String(error)));
      return;
    }
    if (!surface.token || message.surfaceId !== surface.token) return;
    if (message.type === "mounted") { surface.flushable = message.flush === true; clearTimeout(surface.timeout); void this.invoke("ready", { surfaceId: surface.token }).then(async () => { await this.refresh(); if (surface.focusPending === this.focusEpoch && surface.visible) this.focusSurface(surface); }).catch(error => this.surfaceError(surface, String(error))); return; }
    if (message.type === "flushed" && surface.flushing && surface.flushing.id === message.id) { surface.flushing.done(message.error); return; }
    if (message.type !== "request" || typeof message.id !== "string") return;
    const requestId = message.id;
    if (message.method === "data") {
      void this.data.request(surface.token, message.params, data => this.post(surface, {type: "data", data}))
        .then(data => this.post(surface, {type: "response", id: requestId, data}))
        .catch(error => this.post(surface, {type: "response", id: requestId, error: String(error)}));
      return;
    }
    const task = async () => {
      try {
        let data: unknown;
        const shell = this.shells.get(surface.view.context.workspaceId)!;
        if (message.method === "openView") { const key = `${this.definitions[surface.view.definitionKey].pluginId}:${message.params.viewType}`; data = await this.perform(shell, { op: "open", key, params: message.params.params, resourceKey: message.params.resourceKey }); this.focus(shell,key); }
        else if (message.method === "event") { data = await this.invoke("event", { surfaceId: surface.token, event: message.params }); await this.refresh(); }
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
    error.textContent = message;
  }
  private async perform(shell: Shell, action: Record<string, unknown>) {
    const result = await this.invoke("action", { context: shell.workspace.context, action: { requestId: crypto.randomUUID(), ...action } });
    await this.refresh(); return result;
  }
  private async action(id: string, action: string, key: string, anchor: HTMLElement) {
    const shell = this.shells.get(id)!;
    if (["toggle", "locate", "collapse"].includes(action)) { await this.perform(shell, { op: action, key, side: key }); this.focus(shell, action === "collapse" ? undefined : key); return; }
    if (action === "select") { await this.perform(shell, { op: "select", viewId: key }); const surface = shell.surfaces.get(key); if (surface) this.focusSurface(surface); return; }
    if (action === "close") { await this.close([id], key); return; }
    if (action === "more") { this.showMenu(anchor, Object.values(this.definitions).filter(d => shell.ui.preferences[d.key]?.side === key && (d.container === "sidebar" || d.locateEntry)).map(d => ({ label: d.title, action: () => this.perform(shell, { op: d.container === "sidebar" ? "open" : "locate", key: d.key }), extra: { label: shell.ui.preferences[d.key].pinned ? "取消固定" : "固定", action: () => this.perform(shell, { op: "preference", key: d.key, pinned: !shell.ui.preferences[d.key].pinned }) } })), true); return; }
    if (action === "panel-menu") {
      const p = shell.ui.preferences[key];
      const sideKeys = Object.keys(shell.ui.preferences).filter(k => shell.ui.preferences[k].side === p.side).sort((a,b) => shell.ui.preferences[a].order-shell.ui.preferences[b].order);
      this.showMenu(anchor, [
        { label: p.side === "left" ? "移到右侧" : "移到左侧", action: () => this.perform(shell, { op: "preference", key, side: p.side === "left" ? "right" : "left" }) },
        { label: p.pinned ? "从工具条隐藏" : "固定到工具条", action: () => this.perform(shell, { op: "preference", key, pinned: !p.pinned }) },
        { label: "入口移到最前", action: () => this.perform(shell, { op: "preference", key, order: Math.max(0, Math.min(...sideKeys.map(k => shell.ui.preferences[k].order)) - 1) }) },
        { label: p.startup === "none" ? "新工作区默认展开" : "新工作区默认隐藏", action: () => this.perform(shell, { op: "preference", key, startup: p.startup === "none" ? "reveal" : "none" }) },
        { label: "采用推荐布局", action: () => this.perform(shell, { op: "recommend", key }) },
      ]); return;
    }
    if (action === "pages") { this.showMenu(anchor, shell.ui.views.filter(v => this.definitions[v.definitionKey]?.container === "editor").map(v => ({ label: v.title, action: () => this.perform(shell, { op: "select", viewId: v.id }) })), true); return; }
    const actions = Object.values(this.definitions).filter(d => d.available && d.container === "editor" && d.createAction);
    if (actions.length === 1) { await this.perform(shell, { op: "open", key: actions[0].key }); this.focus(shell, actions[0].key); }
    else this.showMenu(anchor, actions.map(d => ({ label: `${d.pluginName} · ${d.createAction}`, action: () => this.perform(shell, { op: "open", key: d.key }) })));
  }
  private focus(shell: Shell, key?: string) {
    const selected = shell.ui.views.find(v => v.definitionKey === key && [shell.ui.left, shell.ui.right, shell.ui.editor].includes(v.id));
    if (selected) { const surface = shell.surfaces.get(selected.id); if (surface) this.focusSurface(surface); }
    else shell.node.querySelector<HTMLElement>(`[data-key="${CSS.escape(key ?? "")}"]`)?.focus();
  }
  private focusSurface(surface: Surface) { if (!surface.token) { surface.focusPending = this.focusEpoch; return; } surface.focusPending = undefined; surface.frame.focus(); this.post(surface, { type: "focus" }); }
  private showMenu(anchor: HTMLElement, entries: { label: string; action: () => Promise<unknown>; extra?: { label: string; action: () => Promise<unknown> } }[], searchable = false) {
    this.closeMenu(); const node = document.createElement("div"); node.className = "workspace-menu component-menu";
    if (searchable) { const input = document.createElement("input"); input.placeholder = "查找组件或页面"; input.setAttribute("aria-label", "查找组件或页面"); input.addEventListener("input", () => node.querySelectorAll<HTMLElement>(".component-menu-row").forEach(r => r.hidden = !r.textContent!.toLowerCase().includes(input.value.toLowerCase()))); node.append(input); }
    for (const item of entries) { const row = document.createElement("div"); row.className = "component-menu-row"; for (const entry of [item, item.extra].filter(Boolean) as { label: string; action: () => Promise<unknown> }[]) { const b = document.createElement("button"); b.textContent = entry.label; b.addEventListener("click", () => { this.closeMenu(); void this.run(entry.action); }); row.append(b); } node.append(row); }
    if (!entries.length) { const text = document.createElement("p"); text.textContent = "暂无可用组件"; node.append(text); }
    const r = anchor.getBoundingClientRect(); Object.assign(node.style, { left: `${Math.max(4, Math.min(r.left, innerWidth-280))}px`, top: `${Math.min(r.bottom+4, innerHeight-260)}px` });
    node.addEventListener("keydown", event => { if (event.key === "Escape") { this.closeMenu(); anchor.focus(); } }); document.body.append(node); this.menu = node; node.querySelector<HTMLElement>("input,button")?.focus();
  }
  private closeMenu() { this.menu?.remove(); this.menu = undefined; }
  async flush() {
    const surfaces = [...this.shells.values()].flatMap(s => [...s.surfaces.values()]);
    await Promise.all(surfaces.filter(s => s.flushable && s.token).map(s => new Promise<void>((resolve,reject) => {
      const id=crypto.randomUUID(), timer=setTimeout(() => {s.flushing=undefined;reject(new Error("草稿同步超时，操作已取消"));}, 45000);
      s.flushing={id,done:error=>{clearTimeout(timer);s.flushing=undefined;error?reject(new Error(error)):resolve();}};
      this.post(s,{type:"flush",id});
    })));
    await Promise.all(surfaces.map(f => f.chain));
  }
  async close(workspaceIds: string[], viewId?: string) {
    if (this.dialog) return false;
    await this.flush();
    const plan = await this.invoke<ClosePlan>("prepareClose", { workspaceIds, viewId });
    let decision = "discard";
    try {
      if (plan.dirty.length) decision = await new Promise<string>(resolve => {
        const dialog = document.createElement("dialog"); dialog.className = "component-close-dialog"; this.dialog = dialog;
        dialog.innerHTML = `<h2>关闭组件页面</h2><p>以下页面有未保存内容、运行任务或无法确认的状态：</p><ul>${plan.dirty.map(v => `<li>${e(v.serverName)} · ${e(v.title)}${v.busy ? " · 任务运行中" : ""}${v.error ? ` · ${e(v.error)}` : ""}</li>`).join("")}</ul><footer><button data-decision="cancel">取消</button><button data-decision="discard">放弃并关闭</button><button class="primary" data-decision="save">保存并关闭</button></footer>`;
        const finish = (d: string) => { dialog.remove(); this.dialog = undefined; resolve(d); };
        dialog.addEventListener("cancel", event => { event.preventDefault(); finish("cancel"); });
        dialog.addEventListener("click", event => { const d = (event.target as Element).closest<HTMLElement>("[data-decision]")?.dataset.decision; if (d) finish(d); });
        document.body.append(dialog); dialog.showModal();
      });
      await this.invoke("finishClose", { ...plan, decision }); await this.refresh(); return decision !== "cancel";
    } catch (error) { await this.invoke("finishClose", { ...plan, decision: "cancel" }).catch(() => {}); throw error; }
  }
  async allowExit() { return this.close(await this.invoke<string[]>("allWorkspaces")); }
  private async updateNetwork() {
    if(this.disposed||this.networkPending||!this.shells.size)return;this.networkPending=true;
    try {const snapshot=await this.call<NetworkSnapshot>("network_snapshot");for(const shell of this.shells.values()){
      const t=snapshot.servers.find(t=>t.serverId===shell.workspace.context.serverId),node=shell.node.querySelector(".workspace-network");
      const rate=(v:number)=>v<1024?`${v.toFixed(0)} B/s`:v<1048576?`${(v/1024).toFixed(1)} KiB/s`:`${(v/1048576).toFixed(1)} MiB/s`;
      if(node)node.textContent=`↓ ${rate(t?.receivedPerSecond??0)} ↑ ${rate(t?.sentPerSecond??0)}`;
    }} catch { /* A diagnostic endpoint must not interrupt the workbench. */ }finally{this.networkPending=false;}
  }
  private async run(action: () => Promise<unknown>) { try { await action(); } catch (error) { this.notify(String(error)); } }
  private removeSurface(s: Surface) { clearTimeout(s.timeout); this.data.detach(s.token); this.post(s, { type: "unmount" }); s.node.remove(); }
  private destroy(shell: Shell) { shell.observer.disconnect(); for (const f of shell.surfaces.values()) this.removeSurface(f); shell.node.remove(); }
  dispose() { this.disposed = true; clearInterval(this.networkTimer); this.closeMenu(); window.removeEventListener("message", this.message); document.removeEventListener("pointerdown", this.outside); document.removeEventListener("pointerdown", this.interaction, true); document.removeEventListener("keydown", this.interaction, true); this.theme.disconnect(); window.matchMedia("(prefers-color-scheme: dark)").removeEventListener("change", this.systemTheme); for (const shell of this.shells.values()) this.destroy(shell); }
}
