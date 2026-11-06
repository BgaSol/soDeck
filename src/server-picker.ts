import { address, serverApi, type Catalog } from "./server-api";
import type { ConnectionView } from "./connections";
import { escape as e } from "./ui";

/** A view of the saved catalog; connection and authentication stay in ConnectionsUi. */
export class ServerPicker {
  private node = document.createElement("div");
  private search: HTMLInputElement;
  private list: HTMLElement;
  private catalog?: Catalog;
  private connections: ConnectionView[] = [];
  private loading = false;
  private error = "";
  private generation = 0;
  constructor(private anchor: HTMLButtonElement, private connect: (id: string) => Promise<void>, private notify: (text: string) => void) {
    this.node.className = "server-picker sd-menu";
    this.node.id = "server-picker";
    this.node.popover = "auto";
    this.node.setAttribute("role", "dialog");
    this.node.setAttribute("aria-label", "选择服务器");
    this.node.innerHTML = `<input type="search" aria-label="搜索服务器" placeholder="搜索名称、地址、账号或分组" autofocus><div class="server-picker-list"></div>`;
    this.search = this.node.querySelector("input")!;
    this.list = this.node.querySelector(".server-picker-list")!;
    document.body.append(this.node);
    anchor.setAttribute("aria-controls", this.node.id);
    anchor.popoverTargetElement = this.node;
    this.node.addEventListener("beforetoggle", event => {
      if (event.newState === "open") { this.search.value = ""; this.loading = true; this.error = ""; this.render(); }
      else this.generation++;
    });
    this.node.addEventListener("toggle", () => {
      anchor.setAttribute("aria-expanded", String(this.open));
      if (this.open) { this.position(); this.search.focus(); void this.refresh(); }
    });
    this.search.addEventListener("input", () => this.render());
    this.node.addEventListener("keydown", event => this.key(event));
    this.node.addEventListener("click", event => {
      const button = (event.target as Element).closest<HTMLButtonElement>("button");
      if (!button || button.disabled) return;
      if (button.hasAttribute("data-picker-retry")) { void this.refresh(); return; }
      const id = button.dataset.serverId;
      if (id) { this.close(true); void this.connect(id).catch(error => this.notify(String(error))); }
    });
    window.addEventListener("resize", () => { if (this.open) this.position(); });
  }
  private get open() { return this.node.matches(":popover-open"); }
  setConnections(views: ConnectionView[]) { this.connections = views; if (this.open) this.render(); }
  async refresh() {
    if (!this.open) return;
    const generation = ++this.generation;
    this.loading = true; this.error = ""; this.render();
    try {
      const catalog = await serverApi.read();
      if (generation !== this.generation) return;
      this.catalog = catalog; this.error = catalog.error ?? "";
    } catch (error) { if (generation === this.generation) this.error = String(error); }
    finally { if (generation === this.generation) { this.loading = false; if (this.open) this.render(); } }
  }
  private render() {
    const focused = this.list.contains(document.activeElement) ? (document.activeElement as HTMLElement).dataset.serverId : undefined;
    const query = this.search.value.trim().toLocaleLowerCase();
    const group = (id: string | null) => this.catalog?.groups.find(g => g.id === id)?.name ?? "未分组";
    const servers = (this.catalog?.servers ?? []).filter(s => [s.name, address(s), s.username, group(s.groupId)].join(" ").toLocaleLowerCase().includes(query))
      .sort((a, b) => a.name.localeCompare(b.name, "zh-CN", { numeric: true }));
    this.list.innerHTML = this.error ? `<p role="alert">${e(this.error)}</p><button data-picker-retry>重新读取</button>`
      : this.loading && !this.catalog ? `<p role="status">正在读取服务器…</p>` : servers.length ? servers.map(s => {
        const v = this.connections.find(v => v.serverId === s.id), busy = v?.state === "connecting";
        const status = busy ? v.question ? "等待确认" : "正在连接" : v?.state === "connected" ? "已打开" : v?.state === "disconnected" ? "已断线" : "";
        return `<button data-server-id="${e(s.id)}" ${busy ? "disabled" : ""} title="${e(`${s.username}@${address(s)} · ${group(s.groupId)}`)}"><span class="server-picker-record"><strong>${e(s.name)}</strong><span>${e(s.username)}@${e(address(s))} · ${e(group(s.groupId))}</span></span><span class="server-picker-state">${status}</span></button>`;
      }).join("") : `<p role="status">${this.catalog?.servers.length ? "没有匹配的服务器" : "暂无服务器，请先在服务器管理中添加"}</p>`;
    if (focused) (this.list.querySelector<HTMLButtonElement>(`[data-server-id="${CSS.escape(focused)}"]:not(:disabled)`) ?? this.search).focus();
    this.position();
  }
  private key(event: KeyboardEvent) {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); this.close(true); return; }
    const buttons = [...this.list.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    if (event.key === "Enter" && event.target === this.search) { event.preventDefault(); buttons[0]?.click(); }
    if (["ArrowDown", "ArrowUp"].includes(event.key) || (event.target !== this.search && ["Home", "End"].includes(event.key))) {
      event.preventDefault();
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : event.key === "ArrowUp" ? (index < 0 ? buttons.length - 1 : (index - 1 + buttons.length) % buttons.length) : (index + 1) % buttons.length;
      buttons[next]?.focus();
    }
  }
  private close(focus: boolean) { this.node.hidePopover(); if (focus) this.anchor.focus(); }
  private position() {
    const r = this.anchor.getBoundingClientRect();
    this.node.style.maxHeight = `${Math.max(80, innerHeight - r.bottom - 12)}px`;
    this.node.style.left = `${Math.max(8, Math.min(r.left, innerWidth - this.node.offsetWidth - 8))}px`;
    this.node.style.top = `${r.bottom + 4}px`;
  }
}
