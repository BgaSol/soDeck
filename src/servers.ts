import {connectionTree} from './connection-tree';
import { open } from "@tauri-apps/plugin-dialog";
import { newDraft, referenced, serverApi, toInput, type Catalog, type Change, type Draft, type ServerApi } from "./server-api";
import { action, editor, page, type PageState } from "./server-views";
import { escape as e } from "./ui";
import { updateDOM } from "./dom-update";
import type { ConnectionView } from "./connections";

interface Frame {
  dialog: HTMLDialogElement; kind: "editor" | "tags" | "tag" | "confirm";
  draft?: Draft; initial?: string; proxy?: boolean; inline?: boolean;
  revision: number; busy: boolean; error: string; tagId?: string;
}
export class ServerManager {
  onSelection?:()=>void;
  private connections: ConnectionView[] = [];
  setConnections(views: ConnectionView[]) { this.connections = views; this.render(); }
  private catalog?: Catalog;
  private state: PageState = { tab: "servers", query: "", tags: [], selected: "", collapsed: new Set(), compact: false };
  private frames: Frame[] = [];
  private busy = false;
  private generation = 0;
  private disposed = false;
  private readError = "";
  private linesFrame?:number;
  private linesObserver = new ResizeObserver(()=>this.scheduleLines());
  private selectionTimer?: ReturnType<typeof setTimeout>;
  constructor(private host: HTMLElement, private notify: (message: string) => void, private api: ServerApi = serverApi) {
    this.linesObserver.observe(host);
    host.addEventListener("keydown",event=>this.treeKey(event));
    host.addEventListener("mousedown", event => {
      if (event.detail === 2 && (event.target as Element).closest("[data-connect-server-on-double-click]")) clearTimeout(this.selectionTimer);
    });
    host.addEventListener("click", event => {
      clearTimeout(this.selectionTimer);
      const b = (event.target as Element).closest<HTMLButtonElement>("[data-server-action]");
      if (!b || b.disabled) return;
      const perform = () => { void this.perform(b).catch(error => notify(String(error))); };
      // Narrow layouts hide the list on selection; allow the second click to reach its row first.
      if (b.hasAttribute("data-connect-server-on-double-click") && event.detail === 1 && matchMedia("(max-width: 760px)").matches) this.selectionTimer = setTimeout(perform, 500);
      else perform();
    });
    host.addEventListener("input", event => {
      clearTimeout(this.selectionTimer);
      const input = event.target as HTMLInputElement;
      if (input.id === "catalog-query") { this.state.query = input.value; this.render(); }
    });
    host.addEventListener("change", event => {
      clearTimeout(this.selectionTimer);
      const input = event.target as HTMLSelectElement;
      if (input.dataset.filterTag) {const id=input.dataset.filterTag;this.state.tags=(input as unknown as HTMLInputElement).checked?[...this.state.tags,id]:this.state.tags.filter(t=>t!==id);this.render();}
    });
  }
  async start() {
    const generation = ++this.generation;
    try {
      const catalog = await this.api.read();
      if (this.disposed || generation !== this.generation) return false;
      if (!this.catalog || catalog.revision >= this.catalog.revision) this.catalog = catalog;
      this.readError = ""; this.render(); return true;
    } catch (error) {
      if (this.disposed || generation !== this.generation) return false;
      this.readError = `无法读取服务器目录：${error}`; this.render(); return false;
    }
  }
  private recordPath(id:string) {if(this.state.tab==='proxies')return JSON.stringify([['socks5',id]]);return [...connectionTree(this.catalog!,{query:'',tags:[]}).index.values()].find(n=>n.recordId===id&&n.target)?.key??'';}
  private selectRecord(id:string) {this.state.selected=this.recordPath(id);}
  private treeKey(event:KeyboardEvent) {
    if(event.isComposing)return;
    const row=(event.target as Element).closest<HTMLElement>('.connection-node');if(!row)return;
    const rows=[...this.host.querySelectorAll<HTMLElement>('.connection-node')],index=rows.indexOf(row);
    if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();rows[event.key==='Home'?0:event.key==='End'?rows.length-1:Math.max(0,Math.min(rows.length-1,index+(event.key==='ArrowUp'?-1:1)))]?.focus();}
    if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();const toggle=row.querySelector<HTMLButtonElement>('[data-server-action="collapse"]');if(toggle&&((event.key==='ArrowLeft'&&row.getAttribute('aria-expanded')==='true')||(event.key==='ArrowRight'&&row.getAttribute('aria-expanded')==='false')))toggle.click();else if(event.key==='ArrowLeft'){this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(row.dataset.parent??'')}"]`)?.focus();}else rows[index+1]?.focus();}
    if(event.key==='Enter'||event.key===' '){event.preventDefault();row.click();}
  }
  private scheduleLines() {if(this.linesFrame!==undefined)return;this.linesFrame=requestAnimationFrame(()=>{this.linesFrame=undefined;this.drawLines();});}
  private drawLines() {
    const tree=this.host.querySelector<HTMLElement>('.connection-tree'),svg=tree?.querySelector<SVGElement>('.tree-lines'),origin=tree?.querySelector<HTMLElement>('.tree-origin');
    if(!tree||!svg||!origin)return;
    const bounds=tree.getBoundingClientRect(),start=origin.getBoundingClientRect(),rows=[...tree.querySelectorAll<HTMLElement>('.connection-node')];
    svg.setAttribute('viewBox',` 0 0 ${bounds.width} ${bounds.height}`.trim());
    svg.innerHTML=rows.map(row=>{const box=row.getBoundingClientRect(),parent=rows.find(n=>n.dataset.path===row.dataset.parent)?.getBoundingClientRect()??start;const x=box.left-bounds.left-10,y=box.top-bounds.top+box.height/2,py=parent.top-bounds.top+parent.height/2;return `<path class="${row.classList.contains('on-route')?'on-route':''}" d="M ${x} ${py} V ${y} H ${box.left-bounds.left}"/>`;}).join('');
  }
  private render() {
    if (this.disposed) return;
    updateDOM(this.host, `${this.catalog ? page(this.catalog, this.state, this.connections) : ""}${this.readError ? `<div class="error-box" role="alert">${e(this.readError)} ${action("reload", "重试")}</div>` : ""}`);
    this.scheduleLines();
  }
  dispose() { this.linesObserver.disconnect();if(this.linesFrame!==undefined)cancelAnimationFrame(this.linesFrame);this.disposed = true; this.generation++; clearTimeout(this.selectionTimer); }
  private top() { return this.frames.at(-1); }
  private frame(kind: Frame["kind"]): Frame {
    const dialog = document.createElement("dialog");
    dialog.id = `catalog-dialog-${crypto.randomUUID()}`;
    dialog.className = `catalog-dialog ${kind === "editor" ? "endpoint-dialog" : "small-dialog"}`;
    dialog.setAttribute("aria-label", kind === "editor" ? "编辑资料" : kind === "tags" ? "管理标签" : kind === "tag" ? "编辑标签" : "确认操作");
    const frame: Frame = { dialog, kind, revision: this.catalog!.revision, busy: false, error: "" };
    dialog.addEventListener("cancel", event => { event.preventDefault(); void this.cancel(frame); });
    dialog.addEventListener("click", event => {
      if (frame !== this.top()) return;
      const button = (event.target as Element).closest<HTMLButtonElement>("[data-server-action]");
      if (button && !button.disabled) void this.perform(button).catch(error => this.notify(String(error)));
    });
    dialog.addEventListener("input", event => this.editField(frame, event.target as HTMLInputElement));
    dialog.addEventListener("change", event => this.editField(frame, event.target as HTMLInputElement));
    dialog.addEventListener("submit", event => { event.preventDefault(); void this.save(frame); });
    this.frames.push(frame); document.body.append(dialog);
    return frame;
  }
  private dismiss(f: Frame) {
    if (f !== this.top()) return;
    f.dialog.close(); f.dialog.remove(); this.frames.pop();
    if (f.draft) f.draft.password = "";
  }
  private renderEditor(f: Frame) {
    updateDOM(f.dialog, editor(this.catalog!, f.draft!, !!f.proxy, f.error, f.busy, f.dialog.id));
  }
  private openEditor(proxy: boolean, id?: string, inline = false) {
    const record = id ? (proxy ? this.catalog!.proxies : this.catalog!.servers).find(r => r.id === id) : undefined;
    if (id && !record) throw new Error("资料已不存在，请刷新目录");
    const f = this.frame("editor"); f.proxy = proxy; f.inline = inline; f.draft = newDraft(record);
    if (proxy && !record) { f.draft.port = "1080"; f.draft.authType = "none"; }
    if (!proxy && !record) f.draft.tagIds = [...this.state.tags];
    f.initial = JSON.stringify(f.draft); this.renderEditor(f); f.dialog.showModal();
    f.dialog.querySelector<HTMLInputElement>('[data-field="host"]')?.focus();
  }
  private editField(f: Frame, input: HTMLInputElement) {
    if (!f.draft || f.busy) return;
    if(input.dataset.tag) {const id=input.dataset.tag;f.draft.tagIds=input.checked?[...new Set([...f.draft.tagIds,id])]:f.draft.tagIds.filter(t=>t!==id);return;}
    const key = input.dataset.field;
    const d = f.draft;
    if (key && key in d) {
      if (key === "remember") d.remember = input.checked;
      else if (key === "routed") d.routed = input.value === "true";
      else if (["name", "host", "port", "username", "privateKeyPath", "password", "authType"].includes(key)) {
        (d as unknown as Record<string, string>)[key] = input.value;
        if (key === "password" && input.value) d.clearSecret = false;
      }
      if (key === "authType") { d.password = ""; d.remember = true; d.clearSecret = false; this.renderEditor(f); }
      if (key === "routed") this.renderEditor(f);
    }
    if (input.dataset.step !== undefined) {
      const step = d.route[Number(input.dataset.step)];
      if (step.kind === "ssh") step.serverId = input.value; else step.proxyId = input.value;
      this.renderEditor(f);
    }
  }
  private dirty(f: Frame) {
    return !!f.draft && f.initial !== JSON.stringify(f.draft) || f.kind === "tag" && f.dialog.querySelector<HTMLInputElement>("#tag-name")?.value !== f.initial;
  }
  private confirm(title: string, text: string, accept = "确定"): Promise<boolean> {
    const f = this.frame("confirm");
    f.dialog.innerHTML = `<header class="catalog-dialog-heading"><h2>${e(title)}</h2></header><div class="confirmation-body"><p>${e(text)}</p></div><footer class="catalog-dialog-footer">${action("confirm-no", "取消")}${action("confirm-yes", e(accept), "", "primary")}</footer>`;
    return new Promise(resolve => {
      const finish = (value: boolean) => { this.dismiss(f); resolve(value); };
      f.dialog.addEventListener("click", event => {
        const name = (event.target as Element).closest<HTMLElement>("[data-server-action]")?.dataset.serverAction;
        if (name === "confirm-yes") finish(true); else if (name === "confirm-no") finish(false);
      });
      f.dialog.addEventListener("cancel", () => finish(false), { once: true });
      f.dialog.showModal(); f.dialog.querySelector<HTMLElement>("button")?.focus();
    });
  }
  private async cancel(f: Frame) {
    if (f.busy || f.kind === "confirm") return;
    if (!this.dirty(f) || await this.confirm("放弃未保存的修改？", "当前表单的修改尚未保存。已经单独保存的代理和标签会保留。", "放弃修改")) this.dismiss(f);
  }
  setCloseBlocked(blocked:boolean) {for(const frame of this.frames)frame.dialog.inert=blocked;}
  async closeInputs(decision:"collect"|"save"|"discard") {
    if(decision!=="collect"&&(this.busy||this.frames.some(f=>f.busy)))throw new Error("服务器资料正在保存，请等待原操作结束");
    const drafts=this.frames.filter(f=>this.dirty(f)||f.busy).map((f,index)=>({id:`server:${index}`,title:"服务器资料表单",dirty:this.dirty(f),busy:f.busy}));
    if(decision==="save"&&drafts.length)throw new Error("请返回服务器资料表单保存，然后重新关闭");
    if(decision==="discard")while(this.frames.length)this.dismiss(this.frames[this.frames.length-1]);
    return decision==="discard"?[]:drafts;
  }
  private async mutate(change: Change, revision = this.catalog!.revision) {
    const result = await this.api.change(revision, change);
    this.catalog = result.catalog; this.render(); return result.selectedId;
  }
  private async save(f: Frame) {
    if (f.busy || f !== this.top()) return;
    f.error = ""; f.busy = true;
    try {
      if (f.kind === "tag") {
        const name = f.dialog.querySelector<HTMLInputElement>("#tag-name")!.value;
        f.dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled = true;
        const id = await this.mutate({ op: "saveTag", id: f.tagId ?? null, name }, f.revision);
        this.dismiss(f); const parent = this.top();
        if (parent?.kind === "editor") { parent.revision = this.catalog!.revision; parent.draft!.tagIds.push(id!); this.renderEditor(parent); }
        else if (parent?.kind === "tags") this.renderTags(parent);
        this.notify("标签已保存");
      } else if (f.kind === "editor") {
        const d = f.draft!;
        if (d.routed && !f.proxy && !d.route.length) throw new Error("请添加中转节点，或选择直接连接");
        if (d.routed && d.route.some(r => r.kind === "ssh" ? !r.serverId : !r.proxyId)) throw new Error("请为每一站选择已保存的资料");
        if (!/^\d+$/.test(d.port) || +d.port < 1 || +d.port > 65535) throw new Error("端口范围为 1–65535");
        this.renderEditor(f);
        const id = await this.mutate({ op: f.proxy ? "saveProxy" : "saveServer", draft: toInput(d, !!f.proxy) }, f.revision);
        this.dismiss(f); const parent = this.top();
        if (f.inline && parent?.draft) {
          parent.revision = this.catalog!.revision;
          parent.draft.routed = true; parent.draft.route.push({ id: "", kind: "socks5", proxyId: id! }); this.renderEditor(parent);
        } else { this.selectRecord(id!); this.render(); }
        this.notify(f.proxy ? "代理资料已保存" : "服务器资料已保存");
      }
    } catch (error) {
      f.error = String(error);
      if (f.kind === "editor") { f.busy = false; this.renderEditor(f); f.dialog.querySelector(".error-box")?.scrollIntoView({ block: "nearest" }); }
      else { f.dialog.querySelector<HTMLElement>(".tag-error")!.textContent = f.error; f.dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled = false; }
    } finally { f.busy = false; }
  }
  private renderTags(f: Frame) {
    updateDOM(f.dialog, `<header class="catalog-dialog-heading"><h2>管理标签</h2>${action("cancel", "关闭")}</header><div class="tag-list">${this.catalog!.tags.map(g => `<div><strong>${e(g.name)}</strong><small>${this.catalog!.servers.filter(s => s.tagIds.includes(g.id)).length} 台服务器</small>${action("rename-tag", "重命名", `data-id="${e(g.id)}"`)}${action("delete-tag", "删除", `data-id="${e(g.id)}"`, "danger")}</div>`).join("") || '<p class="hint">还没有标签</p>'}</div><footer class="catalog-dialog-footer">${action("new-tag", "+ 新建标签", "", "primary")}</footer>`);
  }
  private openTag(id?: string) {
    const name = this.catalog!.tags.find(g => g.id === id)?.name ?? "";
    const parent = this.top();
    const f = this.frame("tag"); f.tagId = id; f.initial = name;
    if (parent?.kind === "editor") f.revision = parent.revision;
    f.dialog.innerHTML = `<form><header class="catalog-dialog-heading"><h2>${id ? "重命名标签" : "新建标签"}</h2></header><div class="confirmation-body"><label class="catalog-field" for="tag-name">标签名称<input id="tag-name" value="${e(name)}" required></label><p class="hint">独立保存，取消服务器表单后仍保留。</p><p class="tag-error" role="alert"></p></div><footer class="catalog-dialog-footer">${action("cancel", "取消")}<button type="submit" class="primary">保存标签</button></footer></form>`;
    f.dialog.showModal(); f.dialog.querySelector<HTMLInputElement>("input")?.focus();
  }
  private async perform(button: HTMLButtonElement) {
    const name = button.dataset.serverAction!; const id = button.dataset.id ?? "";
    const f = this.top(); const d = f?.draft;
    if (this.busy || f?.busy) return;
    if (name === "confirm-no" || name === "confirm-yes") return;
    switch (name) {
      case "tab": this.onSelection?.();this.state.tab = button.dataset.tab as PageState["tab"]; this.state.selected = ""; this.state.query = ""; this.render(); this.host.querySelector<HTMLElement>(`[data-tab="${this.state.tab}"]`)?.focus(); break;
      case "select":
        this.onSelection?.();
        this.state.selected = button.dataset.path ?? this.recordPath(id); this.render();
        this.host.querySelector<HTMLElement>(matchMedia("(max-width: 760px)").matches ? '.catalog-detail [data-connect-server], .catalog-detail [data-server-action="edit"]' : `[data-path="${CSS.escape(this.state.selected)}"]`)?.focus();
        break;
      case "collapse": {const path=button.dataset.path!;this.state.collapsed.has(path)?this.state.collapsed.delete(path):this.state.collapsed.add(path);this.render();break;}
      case "density": this.state.compact=!this.state.compact;this.render();break;
      case "back": {
        const selected = this.state.selected; this.state.selected = ""; this.render();
        this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(selected)}"]`)?.focus();
        break;
      }
      case "new": this.openEditor(this.state.tab === "proxies"); break;
      case "edit": this.openEditor(this.state.tab === "proxies", id); break;
      case "cancel": if (f) await this.cancel(f); break;
      case "reload": await this.start(); break;
      case "clear-filters": this.state.query = ""; this.state.tags = []; this.render(); this.host.querySelector<HTMLElement>("#catalog-query")?.focus(); break;
      case "reload-editor": {
        if (!f || !await this.confirm("重新读取表单？", "当前输入将被目录中最新保存的资料替换。", "重新读取")) break;
        this.catalog = await this.api.read();
        if (this.catalog.error) throw new Error(this.catalog.error);
        const r = (f.proxy ? this.catalog.proxies : this.catalog.servers).find(r => r.id === d?.id);
        if (d?.id && !r) throw new Error("资料已被删除，当前草稿已保留");
        f.draft = newDraft(r); if (f.proxy && !r) { f.draft.port = "1080"; f.draft.authType = "none"; }
        f.initial = JSON.stringify(f.draft); f.revision = this.catalog.revision; f.error = ""; this.renderEditor(f); this.render(); break;
      }
      case "locate": {
        if (f) { if (await this.confirm("离开当前表单？", "先关闭表单，再定位这条资料；未保存的修改将被放弃。", "定位资料")) { this.dismiss(f); } else break; }
        this.onSelection?.();this.state.tab = this.catalog!.servers.some(s => s.id === id) ? "servers" : "proxies";
        this.selectRecord(id); this.state.tags = []; this.state.query = ""; this.render(); break;
      }
      case "copy": await this.listMutation({ op: this.state.tab === "proxies" ? "copyProxy" : "copyServer", id }); this.notify("已复制资料，未复制密码或口令"); break;
      case "delete": {
        const deps = referenced(this.catalog!, id);
        if (deps.length) { await this.confirm("此资料正在被引用", `请先编辑以下服务器的路径：${deps.map(s => s.name).join("、")}`, "知道了"); break; }
        const r = [...this.catalog!.servers, ...this.catalog!.proxies].find(r => r.id === id)!;
        if (await this.confirm("删除这条资料？", `删除「${r.name}」及其当前凭据。远端服务器和本机私钥文件不受影响。`, "删除资料")) {
          await this.listMutation({ op: this.state.tab === "proxies" ? "deleteProxy" : "deleteServer", id }); this.state.selected = ""; this.render(); this.notify("资料已删除");
        } break;
      }
      case "tags": { const tagFrame = this.frame("tags"); this.renderTags(tagFrame); tagFrame.dialog.showModal(); break; }
      case "new-tag": this.openTag(); break;
      case "rename-tag": this.openTag(id); break;
      case "delete-tag":
        if (await this.confirm("删除标签？", "只移除此标签，服务器资料和其他标签会保留。", "删除标签")) {
          await this.listMutation({ op: "deleteTag", id }); this.state.tags=this.state.tags.filter(t=>t!==id);
          this.renderTags(f!); this.render();
        } break;
      case "inline-proxy": this.openEditor(true, undefined, true); if (f) this.top()!.revision = f.revision; break;
      case "add-ssh": case "add-socks":
        if (d) {
          d.route.push(name === "add-ssh" ? { id: "", kind: "ssh", serverId: "" } : { id: "", kind: "socks5", proxyId: "" });
          this.renderEditor(f!); f!.dialog.querySelector<HTMLElement>(`[data-step="${d.route.length - 1}"]`)?.focus();
        }
        break;
      case "step-up": case "step-down": case "step-remove": {
        if (!d) break;
        const i = Number(button.dataset.index); const j = i + (name === "step-up" ? -1 : 1);
        if (name === "step-remove") d.route.splice(i, 1);
        else if (j >= 0 && j < d.route.length) [d.route[i], d.route[j]] = [d.route[j], d.route[i]];
        this.renderEditor(f!); break;
      }
      case "clear-secret": if (d) { d.clearSecret = !d.clearSecret; d.password = ""; this.renderEditor(f!); } break;
      case "choose-key": {
        if (!d) break;
        const path = await open({ multiple: false, directory: false, title: "选择私钥文件" });
        if (typeof path === "string" && f === this.top()) { d.privateKeyPath = path; this.renderEditor(f!); } break;
      }
      case "recover":
        if (await this.confirm("从备份恢复目录？", "会恢复上一份有效目录，当前文件另存保留。备份可能包含旧密码。", "恢复目录")) { this.catalog = await this.api.recover(); this.render(); this.notify("目录已恢复"); } break;
    }
  }
  private async listMutation(change: Change) {
    this.busy = true;
    try { const id = await this.mutate(change); if (id) this.selectRecord(id); this.render(); }
    finally { this.busy = false; }
  }
}
