import { invoke } from "@tauri-apps/api/core";
import { address, type Catalog } from "./server-api";
import type { ConnectionView } from "./connections";
import { escape as e, icon } from "./ui";

type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** A connection launcher; successful connections alone create workspace tabs. */
export class ServerPicker {
  private node?: HTMLElement;
  constructor(private anchor: HTMLButtonElement, private connect: (id: string) => Promise<void>, private create: () => Promise<void>, private notify: (text: string) => void, private call: Call = invoke) {
    anchor.addEventListener("click", () => { if (this.node) this.close(); else void this.show(); });
    document.addEventListener("pointerdown", event => { if (!this.node?.contains(event.target as Node) && !anchor.contains(event.target as Node)) this.close(); });
    window.addEventListener("resize", () => this.close());
  }
  private close(focus = false) {
    this.node?.remove(); this.node = undefined; this.anchor.setAttribute("aria-expanded", "false");
    if (focus) this.anchor.focus();
  }
  private async show() {
    const node = document.createElement("section"); this.node = node;
    node.className = "server-picker sd-menu"; node.setAttribute("role", "dialog"); node.setAttribute("aria-label", "连接服务器");
    node.innerHTML = `<input type="search" aria-label="搜索服务器" placeholder="搜索名称、地址或分组"><div class="server-picker-list"><p role="status">正在读取服务器…</p></div><hr><button type="button" data-new-server>${icon("plus")}<span>新建服务器</span></button>`;
    document.body.append(node); this.anchor.setAttribute("aria-expanded", "true");
    const rect = this.anchor.getBoundingClientRect();
    node.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - node.offsetWidth - 8))}px`; node.style.top = `${rect.bottom + 6}px`;
    const input = node.querySelector<HTMLInputElement>("input")!; input.focus();
    node.addEventListener("keydown", event => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); this.close(true); }
      if (!["ArrowDown", "ArrowUp", "Home", "End", "Enter"].includes(event.key) || event.isComposing) return;
      const buttons = [...node.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      if (event.key === "Enter") { if (event.target === input) { event.preventDefault(); node.querySelector<HTMLButtonElement>("[data-server-id]")?.click(); } return; }
      if (event.target === input && ["Home", "End"].includes(event.key)) return;
      event.preventDefault(); const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : index < 0 ? (event.key === "ArrowUp" ? buttons.length - 1 : 0) : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    });
    node.addEventListener("click", event => {
      const button = (event.target as Element).closest<HTMLButtonElement>("button"); if (!button || button.disabled) return;
      const id = button.dataset.serverId; this.close();
      void (id ? this.connect(id) : this.create()).catch(error => { this.notify(String(error)); this.anchor.focus(); });
    });
    try {
      const [catalog, connections] = await Promise.all([this.call<Catalog>("server_catalog"), this.call<ConnectionView[]>("connection", {command: {op: "snapshot"}})]);
      if (this.node !== node) return;
      if (catalog.error) throw new Error(catalog.error);
      const render = () => {
        const query = input.value.trim().toLocaleLowerCase();
        const servers = catalog.servers.filter(s => `${s.name} ${s.username} ${s.host} ${s.port} ${catalog.groups.find(g => g.id === s.groupId)?.name ?? ""}`.toLocaleLowerCase().includes(query));
        node.querySelector(".server-picker-list")!.innerHTML = servers.map(s => {
          const state = connections.find(c => c.serverId === s.id)?.state;
          const status = state === "connected" ? "前往工作区" : state === "connecting" ? "继续连接" : "连接";
          return `<button type="button" data-server-id="${e(s.id)}"><span class="server-picker-record"><span>${e(s.name)}</span><span class="server-picker-address">${e(s.username)}@${e(address(s))}</span></span><span class="server-picker-state">${status}</span></button>`;
        }).join("") || `<p role="status">${query ? "没有匹配的服务器" : "暂无服务器，请先新建资料"}</p>`;
      };
      input.addEventListener("input", render); render();
    } catch (error) {
      if (this.node !== node) return;
      node.querySelector(".server-picker-list")!.innerHTML = `<p role="alert">无法读取服务器：${e(error)}</p>`;
    }
  }
}
