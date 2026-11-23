import {connectionTree, filterTree, routeTo, targetPath, ancestors, visibleNodes, type ConnectionTree} from './connection-tree';
import { open } from "@tauri-apps/plugin-dialog";
import { commitTagInput, newDraft, newCopyDraft, newViaDraft, referenced, serverApi, toInput, type Catalog, type Change, type Draft, type ServerApi } from "./server-api";
import { action, editor, page, type PageState } from "./server-views";
import { escape as e } from "./ui";
import { enterMotion, exitMotion } from './motion.js';
import { updateDOM } from "./dom-update";
import type { ConnectionView } from "./connections";
import {type MenuReference} from './menu-position.js';
import {serverActions,type ServerCommand,type ConnectionActions} from './server-actions';
import {ServerMenu} from './server-menu';

type EditorIntent={kind:'new-server';path?:string}|{kind:'new-proxy';parent?:Frame}|{kind:'edit'|'copy';id:string};
interface LocalAction {name:string;id:string;path?:string;index?:string}


interface Frame {
  dialog: HTMLDialogElement; kind: "editor" | "confirm";
  draft?: Draft; initial?: string; proxy?: boolean;
  revision: number; busy: boolean; error: string;
  returnFocus?: HTMLElement; returnPath?:string; notice?: string; parent?:Frame;
}
export class ServerManager {
  onSelection?:()=>void;
  private connections: ConnectionView[] = [];
  setConnections(views: ConnectionView[]) { this.connections = views; this.render(); }
  private catalog?: Catalog;
  private tree?:ConnectionTree;
  connectionActions?:ConnectionActions;
  private menuTarget?:{path?:string;role?:boolean;stamp?:string;toolbar?:boolean};
  private menu=new ServerMenu(command=>{void this.execute(command,true).catch(error=>this.notify(String(error)));});
  private state: PageState = { query: "", tags: [], selected: "", collapsed: new Set(), detailOpen: false };
  private frames: Frame[] = [];
  private busy = false;
  private generation = 0;
  private disposed = false;
  private readError = "";

  private linesFrame?:number;
  private linesTree?:HTMLElement;
  private linesObserver = new ResizeObserver(()=>this.scheduleLines());
  private selectionTimer?: ReturnType<typeof setTimeout>;
  private route?: Element;
  private routeObserver = new IntersectionObserver(entries => {
    for (const entry of entries) if (entry.target === this.route) this.host.classList.toggle('route-motion-paused', !entry.isIntersecting);
  });
  private routeEvents = new AbortController();
  constructor(private host: HTMLElement, private notify: (message: string) => void, private api: ServerApi = serverApi) {
    this.linesObserver.observe(host);
    const visibility = () => host.classList.toggle('route-background', document.hidden);
    visibility();
    document.addEventListener('visibilitychange', visibility, { signal: this.routeEvents.signal });
    const signal=this.routeEvents.signal;
    host.addEventListener('contextmenu',event=>this.contextMenu(event),{signal});
    host.addEventListener('dblclick',event=>{
      const row=(event.target as Element).closest<HTMLElement>('[data-connect-server-on-double-click]');
      if(row&&!(event.target as Element).closest('.tree-toggle')){clearTimeout(this.selectionTimer);void this.execute({kind:'connect',id:row.dataset.id!,path:row.dataset.path!}).catch(error=>notify(String(error)));}
    },{signal});
    host.addEventListener("keydown",event=>this.treeKey(event),{signal});
    host.addEventListener("mousedown", event => {
      if (event.detail === 2 && (event.target as Element).closest("[data-connect-server-on-double-click]")) clearTimeout(this.selectionTimer);
    },{signal});
    host.addEventListener("click", event => {
      clearTimeout(this.selectionTimer);
      const b = (event.target as Element).closest<HTMLButtonElement>("[data-server-command],[data-server-action]");
      if (!b || b.disabled) return;
      const perform = () => { void this.handleButton(b).catch(error => notify(String(error))); };
      // Narrow layouts hide the list on selection; allow the second click to reach its row first.
      if (b.hasAttribute("data-connect-server-on-double-click") && event.detail === 1 && matchMedia("(max-width: 760px)").matches) this.selectionTimer = setTimeout(perform, 500);
      else perform();
    },{signal});
    host.addEventListener("input", event => {
      clearTimeout(this.selectionTimer);
      const input = event.target as HTMLInputElement;
      if (input.id === "catalog-query") { this.state.query = input.value; this.render(); }
    },{signal});
  }
  async start() {
    const generation = ++this.generation;
    try {
      const catalog = await this.api.read();
      if (this.disposed || generation !== this.generation) return false;
      this.acceptCatalog(catalog); this.render(); return !catalog.error;
    } catch (error) {
      if (this.disposed || generation !== this.generation) return false;
      this.readError = `无法读取服务器目录：${error}`; this.render(); return false;
    }
  }
  private acceptCatalog(catalog:Catalog) {
    if(catalog.error){this.readError=catalog.error;this.catalog=this.catalog?{...this.catalog,error:catalog.error,canRecover:catalog.canRecover}:catalog;return;}
    if(this.catalog&&catalog.revision<this.catalog.revision)return;
    const previous=this.tree?.index.get(this.state.selected);
    this.catalog=catalog;this.tree=connectionTree(catalog);this.readError='';
    const tree=this.tree!,used=new Set(catalog.servers.flatMap(server=>server.tagIds));
    this.state.tags=this.state.tags.filter(id=>used.has(id));
    if(this.state.selected&&!tree.index.has(this.state.selected)){this.state.selected=previous?this.recordPath(previous.recordId):'';if(!this.state.selected){this.state.detailOpen=false;this.notify('所选资料已不存在');}}
    this.state.collapsed=new Set([...this.state.collapsed].filter(key=>tree.index.has(key)));
  }
  private recordPath(id:string,preferred=this.state.selected) {
    const tree=this.tree!,old=tree.index.get(preferred);
    if(old?.recordId===id)return old.key;
    const rows=visibleNodes(tree.roots,new Set(),false);
    return rows.find(node=>node.recordId===id&&(node.kind==='socks5'||node.target))?.key??'';
  }
  private selectRecord(id:string,path?:string) {
    const tree=this.tree!;
    this.state.selected=path&&tree.index.get(path)?.recordId===id?path:this.recordPath(id);
    this.state.detailOpen=!!this.state.selected;
    for(const key of ancestors(tree.index.get(this.state.selected),tree.index))if(key!==this.state.selected)this.state.collapsed.delete(key);
  }
  private treeKey(event:KeyboardEvent) {
    if(event.isComposing)return;
    if(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10'){
      const anchor=(event.target as Element).closest<HTMLElement>('.connection-node,.tree-origin');
      if(anchor){event.preventDefault();event.stopPropagation();this.openNodeMenu(anchor);}return;
    }
    const row=(event.target as Element).closest<HTMLElement>('.connection-node');if(!row||event.target!==row)return;
    const rows=[...this.host.querySelectorAll<HTMLElement>('.connection-node')],index=rows.indexOf(row);
    if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();rows[event.key==='Home'?0:event.key==='End'?rows.length-1:Math.max(0,Math.min(rows.length-1,index+(event.key==='ArrowUp'?-1:1)))]?.focus();}
    if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();const toggle=row.querySelector<HTMLButtonElement>('[data-server-action="collapse"]');if(toggle&&!toggle.disabled&&((event.key==='ArrowLeft'&&row.getAttribute('aria-expanded')==='true')||(event.key==='ArrowRight'&&row.getAttribute('aria-expanded')==='false')))toggle.click();else if(event.key==='ArrowLeft'){this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(row.dataset.parent??'')}"]`)?.focus();}else rows.find(child=>child.dataset.parent===row.dataset.path)?.focus();}
    if(event.key==='Enter'||event.key===' '){event.preventDefault();row.click();}
  }
  private scheduleLines() {if(this.linesFrame!==undefined)return;this.linesFrame=requestAnimationFrame(()=>{this.linesFrame=undefined;this.drawLines();});}
  private drawLines() {
    const tree=this.host.querySelector<HTMLElement>('.connection-tree'),svg=tree?.querySelector<SVGElement>('.tree-lines'),origin=tree?.querySelector<HTMLElement>('.tree-origin');
    if(!tree||!svg||!origin)return;
    const bounds=tree.getBoundingClientRect(),rows=[...tree.querySelectorAll<HTMLElement>('.connection-node')];
    if(!bounds.width||!bounds.height){svg.innerHTML='';return;}
    const index=new Map(rows.map(row=>[row.dataset.path,row]));
    const originBox=origin.querySelector('.tree-origin-symbol')!.getBoundingClientRect();
    svg.setAttribute('viewBox',` 0 0 ${bounds.width} ${bounds.height}`.trim());
    const lines=rows.map(row=>{
      const parent=index.get(row.dataset.parent),anchor=parent?.querySelector('.node-card')!.getBoundingClientRect()??originBox;
      const box=row.querySelector('.node-card')!.getBoundingClientRect();
      const inset=parent?Math.min(originBox.width/2,box.left-anchor.left-4):originBox.width/2;
      const x=Math.round(anchor.left-bounds.left+inset)+.5,py=Math.round(anchor.bottom-bounds.top)+.5,y=Math.round(box.top-bounds.top+box.height/2)+.5;
      return {d:`M ${x} ${py} V ${y-4} Q ${x} ${y} ${x+4} ${y} H ${box.left-bounds.left}`,selected:row.classList.contains('on-route')};
    });
    svg.innerHTML=lines.map(line=>`<path d="${line.d}"/>`).join('')+lines.filter(line=>line.selected).map(line=>`<path class="on-route" d="${line.d}"/>`).join('');
  }
  private render(motion=false) {
    if (this.disposed) return;
    updateDOM(this.host, this.catalog ? page({...this.catalog,error:this.readError||this.catalog.error}, this.state, this.connections,this.tree) : this.readError ? `<div class="error-box" role="alert">${e(this.readError)} ${action("reload", "重试")}</div>` : "", motion);
    const tree=this.host.querySelector<HTMLElement>('.connection-tree')??undefined;
    if(this.linesTree!==tree){if(this.linesTree)this.linesObserver.unobserve(this.linesTree);this.linesTree=tree;if(tree)this.linesObserver.observe(tree);}
    const route = this.host.querySelector('.catalog-detail .route-preview') ?? undefined;
    if (this.route !== route) { if (this.route) this.routeObserver.unobserve(this.route); this.route = route; if (route) this.routeObserver.observe(route); }
    this.updateMenu();
    this.scheduleLines();
  }
  dispose() { this.closeMenu();this.linesObserver.disconnect();this.routeObserver.disconnect();this.routeEvents.abort();this.host.classList.add('route-motion-paused');if(this.linesFrame!==undefined)cancelAnimationFrame(this.linesFrame);this.disposed = true; this.generation++; clearTimeout(this.selectionTimer); while(this.frames.length)this.dismiss(this.frames[this.frames.length-1]); }
  private top() { return this.frames.at(-1); }
  private frame(kind: Frame["kind"]): Frame {
    const dialog = document.createElement("dialog");
    dialog.id = `catalog-dialog-${crypto.randomUUID()}`;
    dialog.className = `catalog-dialog ${kind === "editor" ? "endpoint-dialog" : "small-dialog"}`;
    dialog.setAttribute("aria-label", kind === "editor" ? "编辑资料" : "确认操作");
    const frame: Frame = { dialog, kind, revision: this.catalog!.revision, busy: false, error: "" };
    dialog.addEventListener("cancel", event => { event.preventDefault(); void this.cancel(frame); });
    dialog.addEventListener("click", event => {
      if (frame !== this.top()) return;
      const button = (event.target as Element).closest<HTMLButtonElement>("[data-server-action]");
      if (button && !button.disabled) void this.handleButton(button).catch(error => this.notify(String(error)));
    });
    dialog.addEventListener("input", event => this.editField(frame, event.target as HTMLInputElement));
    dialog.addEventListener("change", event => this.editField(frame, event.target as HTMLInputElement));
    dialog.addEventListener("keydown", event => {
      const input=event.target as HTMLInputElement;
      if(input.dataset.field==='tagInput'&&event.key==='Enter'&&!event.isComposing){event.preventDefault();if(frame.draft&&!frame.busy){commitTagInput(frame.draft,this.catalog!);this.renderEditor(frame);}}
    });
    dialog.addEventListener("submit", event => { event.preventDefault(); void this.save(frame); });
    this.frames.push(frame); document.body.append(dialog);
    return frame;
  }
  private dismiss(f: Frame) {
    if (f !== this.top()) return;
    exitMotion(f.dialog);f.dialog.close(); f.dialog.remove(); this.frames.pop();
    if (f.draft) f.draft.password = "";
    if(f.returnPath)this.focusPath(f.returnPath);
    else if (f.returnFocus?.isConnected) f.returnFocus.focus({preventScroll:true});
    else if (f.draft?.copySource) this.host.querySelector<HTMLElement>('.catalog-detail [data-server-action="copy"]')?.focus();
  }
  private renderEditor(f: Frame) {
    updateDOM(f.dialog, editor(this.catalog!, f.draft!, !!f.proxy, f.error, f.busy, f.dialog.id, f.notice));
  }
  private openEditor(intent:EditorIntent,returnPath?:string) {
    const copy=intent.kind==='copy',id='id'in intent?intent.id:undefined;
    const record=id?[...(this.catalog?.servers??[]),...(this.catalog?.proxies??[])].find(r=>r.id===id):undefined;
    if(id&&!record)throw new Error('资料已不存在，请刷新目录');
    const proxy=intent.kind==='new-proxy'||record?.protocol==='socks5';
    const draft=intent.kind==='new-server'&&intent.path?newViaDraft(routeTo(this.tree!,intent.path),this.state.tags):copy&&record?.protocol==='ssh'?newCopyDraft(record):newDraft(record);
    if(proxy&&!record){draft.port='1080';draft.authType='none';}
    if(!proxy&&!record)draft.tagIds=[...this.state.tags];
    const returnFocus=copy&&!returnPath?this.host.querySelector<HTMLElement>('.catalog-detail [data-server-action="copy"]')??undefined:document.activeElement instanceof HTMLElement?document.activeElement:undefined;
    this.closeMenu();this.onSelection?.();const f=this.frame('editor');
    f.proxy=proxy;f.draft=draft;f.returnPath=returnPath;f.returnFocus=returnFocus;
    if(intent.kind==='new-proxy'&&intent.parent){f.parent=intent.parent;f.revision=intent.parent.revision;}
    if(intent.kind==='new-server'&&intent.path)f.notice=`已带入经「${this.tree!.index.get(intent.path)!.record!.name}」的访问路径，可调整。`;
    if(copy)f.dialog.setAttribute('aria-label','复制服务器');
    f.initial=JSON.stringify(draft);this.renderEditor(f);f.dialog.showModal();enterMotion(f.dialog);
    const host=f.dialog.querySelector<HTMLInputElement>('[data-field="host"]');host?.focus();if(copy)host?.select();
  }
  private editField(f: Frame, input: HTMLInputElement) {
    if (!f.draft || f.busy) return;
    const key = input.dataset.field;
    const d = f.draft;
    if (key && key in d) {
      if (key === "remember") d.remember = input.checked;
      else if (key === "routed") d.routed = input.value === "true";
      else if (["name", "host", "port", "username", "privateKeyPath", "password", "authType", "tagInput"].includes(key)) {
        (d as unknown as Record<string, string>)[key] = input.value;
        if (key === "password" && input.value && !d.copySource) d.clearSecret = false;
      }
      if (key === "authType") { d.password = ""; d.remember = true; d.clearSecret = !!d.copySource; this.renderEditor(f); }
      if (key === "privateKeyPath" && d.copySource) { d.clearSecret = true; this.renderEditor(f); }
      if (key === "password" && d.copySource) this.renderEditor(f);
      if (key === "routed") this.renderEditor(f);
    }
    if (input.dataset.step !== undefined) {
      const step = d.route[Number(input.dataset.step)];
      if (step.kind === "ssh") step.serverId = input.value; else step.proxyId = input.value;
      this.renderEditor(f);
    }
  }
  private dirty(f: Frame) {
    return !!f.draft && f.initial !== JSON.stringify(f.draft);
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
      f.dialog.showModal();enterMotion(f.dialog); f.dialog.querySelector<HTMLElement>("button")?.focus();
    });
  }
  private async cancel(f: Frame) {
    if (f.busy || f.kind === "confirm") return;
    if (!this.dirty(f) || await this.confirm("放弃未保存的修改？", "当前表单的修改尚未保存。已经单独保存的代理会保留；未保存的标签会随表单丢弃。", "放弃修改")) this.dismiss(f);
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
    this.generation++;
    const result = await this.api.change(revision, change);
    if(!this.disposed){this.acceptCatalog(result.catalog);this.render();}return result.selectedId;
  }
  private async save(f: Frame) {
    if (f.busy || f !== this.top()) return;
    f.error = ""; f.busy = true;
    try {
      if (f.kind === "editor") {
        if(this.readError||this.catalog?.error)throw new Error(this.readError||this.catalog!.error!);
        const d = f.draft!;
        if(!f.proxy)commitTagInput(d,this.catalog!);
        if (d.routed && !f.proxy && !d.route.length) throw new Error("请添加中转节点，或选择直接连接");
        if (d.routed && d.route.some(r => r.kind === "ssh" ? !r.serverId : !r.proxyId)) throw new Error("请为每一站选择已保存的资料");
        if (!/^\d+$/.test(d.port) || +d.port < 1 || +d.port > 65535) throw new Error("端口范围为 1–65535");
        this.renderEditor(f);
        const id = await this.mutate({ op: f.proxy ? "saveProxy" : "saveServer", draft: toInput(d, !!f.proxy) }, f.revision);
        if(this.disposed||f!==this.top())return;
        this.dismiss(f); const parent = f.parent;
        if (parent===this.top() && parent?.draft) {
          parent.revision = this.catalog!.revision;
          parent.draft.routed = true; parent.draft.route.push({ id: crypto.randomUUID(), kind: "socks5", proxyId: id! }); this.renderEditor(parent);
        } else { const saved=this.catalog!.servers.find(s=>s.id===id);this.selectRecord(id!,d.id&&f.returnPath?f.returnPath:saved?targetPath(saved):undefined); this.render(); }
        this.notify(f.proxy ? "代理资料已保存" : !d.id ? "服务器资料已创建"+(this.state.query||this.state.tags.length?"；当前筛选未显示时可点击显示此资料":"") : "服务器资料已保存");
        if (d.copySource) this.host.querySelector<HTMLElement>('.catalog-detail [data-connect-server]')?.focus();
      }
    } catch (error) {
      f.error = String(error);
      if (f.kind === "editor") { f.busy = false; this.renderEditor(f); f.dialog.querySelector(".error-box")?.scrollIntoView({ block: "nearest" }); }
    } finally { f.busy = false; }
  }
  private closeMenu(focus=false){this.menu.close(focus);this.menuTarget=undefined;}
  private focusPath(path:string){
    if(path==='@root'||path==='@new'){this.host.querySelector<HTMLElement>(path==='@root'?'#tree-origin':'#catalog-new-menu')?.focus({preventScroll:true});return;}
    const row=this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)
      ??this.host.querySelector<HTMLElement>('.connection-node[tabindex="0"],.connection-node,#tree-origin,#catalog-query');
    row?.focus({preventScroll:true});
  }
  private unavailable(){return this.readError||this.catalog?.error||(this.busy?'资料正在保存，请等待完成':'');}
  private pathStamp(path:string){
    return JSON.stringify([...ancestors(this.tree?.index.get(path),this.tree!.index)].map(key=>{
      const node=this.tree!.index.get(key)!,r=node.record;
      return [key,node.target,r?.host,r?.port,r&&'username'in r?r.username:undefined,r?.authentication.type,r?.authentication.privateKeyPath,r?.authentication.username];
    }));
  }
  private actions(path?:string){return serverActions(path?this.tree?.index.get(path):undefined,this.tree!,this.connections.find(v=>v.serverId===this.tree?.index.get(path??'')?.recordId),this.unavailable());}
  private contextMenu(event:MouseEvent){
    const anchor=(event.target as Element).closest<HTMLElement>('.connection-node,.tree-origin');if(!anchor||!this.catalog)return;
    event.preventDefault();this.openNodeMenu(anchor,{x:event.clientX,y:event.clientY,contextElement:anchor});
  }
  private openNodeMenu(anchor:HTMLElement,reference:MenuReference=anchor){
    clearTimeout(this.selectionTimer);const path=anchor.dataset.path;
    if(path){this.onSelection?.();this.state.selected=path;this.render();anchor=this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)!;}
    const role=path?this.tree!.index.get(path)?.target:undefined;
    const stamp=path?this.pathStamp(path):undefined;this.menuTarget={path,role,stamp};
    this.menu.show(path?`${this.tree!.index.get(path)?.record?.name??'资料已不存在'} · ${this.tree!.index.get(path)?.kind==='ssh'?'SSH':'SOCKS5'}`:'本机',this.actions(path),anchor,reference,()=>path?this.focusPath(path):this.host.querySelector<HTMLElement>('#tree-origin')?.focus(),()=>!this.disposed&&(!path||this.tree?.index.get(path)?.target===role&&!!this.tree?.index.has(path)&&this.pathStamp(path)===stamp));
  }
  private openMenu(anchor:HTMLElement){
    if(this.menu.open&&this.menuTarget?.toolbar){this.closeMenu(true);return;}
    this.menuTarget={toolbar:true};
    this.menu.show('新增资料',this.actions().filter(a=>a.group==='create'),anchor,anchor,()=>anchor.focus(),()=>!this.disposed&&!this.unavailable());
  }
  private updateMenu(){
    if(!this.menu.open)return;const target=this.menuTarget;if(!target)return;
    if(target.path&&(!this.tree?.index.has(target.path)||this.tree.index.get(target.path)?.target!==target.role||this.pathStamp(target.path)!==target.stamp)){this.closeMenu();this.notify('节点位置已变化，请重新打开菜单');return;}
    this.menu.update(this.actions(target.path).filter(a=>!target.toolbar||a.group==='create'),target.path?`${this.tree!.index.get(target.path)?.record?.name??'资料已不存在'} · ${this.tree!.index.get(target.path)?.kind==='ssh'?'SSH':'SOCKS5'}`:undefined);
  }
  private async handleButton(button:HTMLElement){
    if(button.dataset.serverCommand){await this.execute(JSON.parse(button.dataset.serverCommand) as ServerCommand);return;}
    const {serverAction:name,id='',path,index}=button.dataset;
    if(['edit','copy','delete'].includes(name!)){await this.execute({kind:name as 'edit'|'copy'|'delete',id,path});return;}
    if(['new-server','new-proxy','reload'].includes(name!)){await this.execute({kind:name as 'new-server'|'new-proxy'|'reload'});return;}
    await this.perform({name:name!,id,path,index});
  }
  private async execute(command:ServerCommand,fromMenu=false){
    if(this.disposed||this.busy||this.top()?.busy)return;
    const path='path'in command?command.path:undefined;
    if(path&&!this.tree?.index.has(path))throw new Error('节点位置已变化，请重新打开菜单');
    const item=this.actions(path).find(a=>a.command.kind===command.kind&&(!('id'in command)||'id'in a.command&&a.command.id===command.id));
    if(item?.disabled)throw new Error(item.disabled);
    if(['edit','copy','delete','new-server','new-proxy','new-via','connect'].includes(command.kind)&&this.unavailable())throw new Error(this.unavailable());
    const origin=fromMenu?(this.menuTarget?.toolbar?'@new':path??'@root'):undefined;
    const id='id'in command?command.id:'',record=[...(this.catalog?.servers??[]),...(this.catalog?.proxies??[])].find(r=>r.id===id);
    if(['edit','copy','delete','locate-target'].includes(command.kind)&&!record)throw new Error('资料已不存在，请刷新目录');
    switch(command.kind){
      case 'reload':await this.start();break;
      case 'new-server':this.openEditor({kind:'new-server'},origin);break;
      case 'new-proxy':this.openEditor({kind:'new-proxy'},origin);break;
      case 'new-via':this.openEditor({kind:'new-server',path:command.path},command.path);break;
      case 'edit':this.openEditor({kind:'edit',id},origin);break;
      case 'copy':if(record!.protocol==='ssh')this.openEditor({kind:'copy',id},fromMenu?path:undefined);else{await this.listMutation({op:'copyProxy',id});this.notify('已复制代理资料，未复制密码或口令');}break;
      case 'locate-target':await this.perform({name:'locate-target',id});break;
      case 'delete':{
        if(this.connections.some(v=>v.serverId===id&&v.state==='connecting'))throw new Error('请先取消连接并等待结束');
        const deps=referenced(this.catalog!,id);if(deps.length){await this.confirm('此资料正在被引用',`请先编辑以下服务器的路径：${deps.map(s=>s.name).join('、')}`,'知道了');if(path)this.focusPath(path);break;}
        const revision=this.catalog!.revision;
        if(await this.confirm('删除这条资料？',`删除「${record!.name}」及其当前凭据。远端服务器和本机私钥文件不受影响。`,'删除资料')){
          this.busy=true;try{await this.mutate({op:record!.protocol==='socks5'?'deleteProxy':'deleteServer',id},revision);this.state.selected='';this.render();this.focusPath(path??'');this.notify('资料已删除');}finally{this.busy=false;this.render();}
        }else if(path)this.focusPath(path);break;
      }
      case 'connect':{
        const node=this.tree!.index.get(command.path);if(!node?.target||node.kind!=='ssh'||node.recordId!==id)throw new Error('此位置不是服务器目标');
        if(!this.connectionActions)throw new Error('连接服务尚未就绪');await this.connectionActions.connect(id);break;
      }
      case 'focus':if(!this.connectionActions)throw new Error('连接服务尚未就绪');await this.connectionActions.focusWorkspace(id,command.attempt);break;
      case 'cancel-attempt':if(!this.connectionActions)throw new Error('连接服务尚未就绪');await this.connectionActions.cancelAttempt(command.attempt);break;
      case 'continue-attempt':if(!this.connectionActions)throw new Error('连接服务尚未就绪');await this.connectionActions.continueAttempt(command.attempt,command.question);break;
    }
  }
  private async performEditor(input:LocalAction){
    const {name,id}=input,f=this.top(),d=f?.draft;
    if(!f||this.disposed||this.busy||f.busy)return;
    switch(name){
      case "remove-tag":if(d){d.tagIds=d.tagIds.filter(tag=>tag!==id);this.renderEditor(f!);}break;
      case "remove-new-tag":if(d){d.newTagNames.splice(Number(input.index),1);this.renderEditor(f!);}break;
      case "cancel": if (f) await this.cancel(f); break;
      case "reload-editor": {
        if (f && d && !d.id) {
          if (!await this.start()) throw new Error(this.readError || "无法读取服务器目录");
          f.revision = this.catalog!.revision; f.error = "";
          f.notice = d.copySource?"输入已保留；沿用凭据时将使用原服务器当前的值，记住选项保持当前选择。":"输入已保留，请核对路径与标签后再次保存。";
          this.renderEditor(f); break;
        }
        if (!f || !await this.confirm("重新读取表单？", "当前输入将被目录中最新保存的资料替换。", "重新读取")) break;
        if (!await this.start()) throw new Error(this.readError || "无法读取服务器目录");
        const r = (f.proxy ? this.catalog!.proxies : this.catalog!.servers).find(r => r.id === d?.id);
        if (d?.id && !r) throw new Error("资料已被删除，当前草稿已保留");
        f.draft = newDraft(r); if (f.proxy && !r) { f.draft.port = "1080"; f.draft.authType = "none"; }
        f.initial = JSON.stringify(f.draft); f.revision = this.catalog!.revision; f.error = ""; this.renderEditor(f); this.render(); break;
      }
      case "inline-proxy": this.openEditor({kind:'new-proxy',parent:f}); break;
      case "add-ssh": case "add-socks":
        if (d) {
          d.route.push(name === "add-ssh" ? { id: crypto.randomUUID(), kind: "ssh", serverId: "" } : { id: crypto.randomUUID(), kind: "socks5", proxyId: "" });
          this.renderEditor(f!); f!.dialog.querySelector<HTMLElement>(`[data-step="${d.route.length - 1}"]`)?.focus();
        }
        break;
      case "step-up": case "step-down": case "step-remove": {
        if (!d) break;
        const i = Number(input.index); const j = i + (name === "step-up" ? -1 : 1);
        if (name === "step-remove") d.route.splice(i, 1);
        else if (j >= 0 && j < d.route.length) [d.route[i], d.route[j]] = [d.route[j], d.route[i]];
        this.renderEditor(f!); break;
      }
      case "clear-secret": if (d) { d.clearSecret = !d.clearSecret; d.password = ""; this.renderEditor(f!); } break;
      case "choose-key": {
        if (!d) break;
        const path = await open({ multiple: false, directory: false, title: "选择私钥文件" });
        if (typeof path === "string" && f === this.top()) { if (d.copySource && path !== d.privateKeyPath) d.clearSecret = true; d.privateKeyPath = path; this.renderEditor(f!); } break;
      }
    }
  }
  private async perform(input:LocalAction) {
    const {name,id}=input;
    const f = this.top();
    if (this.disposed || this.busy || f?.busy) return;
    if (name === "confirm-no" || name === "confirm-yes") return;
    if(name==='new-menu'&&(this.readError||this.catalog?.error))throw new Error(this.readError||this.catalog!.error!);
    if(["remove-tag","remove-new-tag","cancel","reload-editor","inline-proxy","add-ssh","add-socks","step-up","step-down","step-remove","clear-secret","choose-key"].includes(name))return this.performEditor(input);
    switch (name) {
      case "select":
        this.onSelection?.();
        this.selectRecord(id,input.path); this.render();
        if(matchMedia('(max-width: 760px)').matches)enterMotion(this.host.querySelector<HTMLElement>('.catalog-detail')!,'x');
        this.host.querySelector<HTMLElement>(matchMedia("(max-width: 760px)").matches ? '.catalog-detail [data-connect-server], .catalog-detail [data-server-action="edit"]' : `[data-path="${CSS.escape(this.state.selected)}"]`)?.focus();
        break;
      case "collapse": {const path=input.path!;this.state.collapsed.has(path)?this.state.collapsed.delete(path):this.state.collapsed.add(path);this.render(true);this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus();break;}
      case "back": {
        const selected = this.state.selected; this.state.detailOpen = false; this.render();
          if(matchMedia('(max-width: 760px)').matches)enterMotion(this.host.querySelector<HTMLElement>('.catalog-directory')!,'back');
        this.host.querySelector<HTMLElement>(`[data-path="${CSS.escape(selected)}"]`)?.focus();
        break;
      }

      case "new-menu": this.openMenu(this.host.querySelector<HTMLElement>('#catalog-new-menu')!); break;
      case "toggle-tag": this.state.tags=this.state.tags.includes(id)?this.state.tags.filter(tag=>tag!==id):[...this.state.tags,id];this.render();break;
      case "clear-tags":this.state.tags=[];this.render();break;
      case "reveal":this.state.query="";this.state.tags=[];this.selectRecord(this.tree!.index.get(this.state.selected)?.recordId??"");this.render();break;
      case "clear-filters": this.state.query = ""; this.state.tags = []; this.render(); this.host.querySelector<HTMLElement>("#catalog-query")?.focus(); break;
      case "locate": case "locate-target": {
        if (f) { if (await this.confirm("离开当前表单？", "先关闭表单，再定位这条资料；未保存的修改将被放弃。", "定位资料")) { this.dismiss(f); } else break; }
        this.onSelection?.();this.selectRecord(id,name==="locate"?input.path:this.catalog!.servers.find(s=>s.id===id)?targetPath(this.catalog!.servers.find(s=>s.id===id)!):undefined);
        const filtered=filterTree(this.tree!,this.catalog!,{query:this.state.query,tags:this.state.tags});if(!filtered.index.has(this.state.selected)||name==="locate-target"&&!filtered.index.get(this.state.selected)?.target){this.state.tags=[];this.state.query="";}this.render();break;
      }
      case "recover":
        if (await this.confirm("从备份恢复目录？", "会恢复上一份有效目录，当前文件另存保留。备份可能包含旧密码。", "恢复目录")) { this.acceptCatalog(await this.api.recover()); this.render(); this.notify("目录已恢复"); } break;
    }
  }
  private async listMutation(change: Change) {
    this.busy = true;
    try { const id = await this.mutate(change); if (id) this.selectRecord(id); this.render(); }
    finally { this.busy = false; this.render(); }
  }
}
