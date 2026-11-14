import { core, type Snapshot } from './core';
import { button as b, escape as e, icon } from './ui';
import { tabControls } from './tab-strip';
import type { Workspaces, Projection, Context } from './workspaces';
import type { ConnectionsUi } from './connections';
import { applyTheme } from './theme';

export const consolePages = [
  ['connections', '服务器连接', 'server'], ['plugins', '插件', 'plug'],
  ['settings', '设置', 'settings'], ['messages', '消息', 'bell'],
  ['logs', '日志', 'log'], ['status', '状态', 'info'], ['updates', '更新', 'download'],
] as const;
export type ConsolePage = typeof consolePages[number][0];
export type ShellRoute = {kind:'console';page:ConsolePage} | {kind:'workbench'};

/** Owns navigation and application reads; component surfaces belong to Workspaces. */
export class Shell {
  route: ShellRoute;
  snapshot?: Snapshot;
  error = '';
  networkStale = true;
  private projection?: Projection;
  private revision = 0;
  private disposed = false;
  private workspaces?: Workspaces;
  private connections?: ConnectionsUi;
  onPage?: (page: ConsolePage) => void;
  readonly catalog: HTMLElement;
  readonly content: HTMLElement;
  readonly tabs: HTMLElement;
  readonly pickerButton: HTMLButtonElement;
  readonly page: HTMLElement;
  constructor(readonly root: HTMLElement, private main: boolean) {
    this.route = main ? {kind:'console',page:'connections'} : {kind:'workbench'};
    root.innerHTML = `<header class="topbar" data-tauri-drag-region="deep"><button id="catalog-tab" aria-label="SoDeck 管理台" aria-selected="true">${icon('grid')}<span>SoDeck</span></button><div class="tab-navigation"><div class="server-tabs" id="server-tabs" role="tablist" aria-label="服务器工作区"></div>${tabControls('服务器标签')}</div><button id="new-workspace" class="sd-icon" aria-label="选择服务器" title="选择服务器" aria-haspopup="dialog" aria-expanded="false">${icon('plus')}</button><div id="drag-region" class="drag-region" aria-hidden="true"></div><nav class="global-tools" aria-label="应用工具">${b('messages','<span id="message-count">消息</span>','bell','icon-label','id="messages-button"')}${b('settings','设置','settings','icon-label','id="settings-button"')}</nav><div id="window-controls" class="window-controls">${b('minimize','','minus','','aria-label="最小化"')}${b('maximize','','maximize','','id="maximize-button" aria-label="最大化或还原"')}${b('close-window','','close','close-window','aria-label="关闭窗口"')}</div></header><main class="workspace" id="server-workspace"><section id="management-console" class="management-console"><nav class="console-nav" aria-label="管理台">${consolePages.map(([id,label,symbol])=>`<button data-console-page="${id}" title="${label}">${icon(symbol)}<span>${label}</span></button>`).join('')}<div class="console-nav-bottom">管理台</div></nav><div class="console-main"><header class="console-heading"><span>管理台</span>${icon('chevron')}<strong id="console-title">服务器连接</strong><button id="return-workbench" class="sd-quiet" hidden>返回工作台</button></header><div id="server-catalog"></div><section id="console-page" class="console-page" hidden><p id="layer-error" role="status" class="error-box" hidden></p><div id="layer-content" class="layer-content"></div></section></div></section><div id="workspace-content" hidden></div></main><footer id="shell-status" class="shell-status sd-edge-top"></footer><div id="toasts" class="toasts" aria-live="polite"></div>`;
    this.catalog=root.querySelector('#server-catalog')!;
    this.content=root.querySelector('#workspace-content')!;
    this.tabs=root.querySelector('#server-tabs')!;
    this.pickerButton=root.querySelector('#new-workspace')!;
    this.page=root.querySelector('#console-page')!;
    root.querySelector('#catalog-tab')!.addEventListener('click',()=>this.navigate('connections'));
    root.querySelector('#return-workbench')!.addEventListener('click',()=>this.showWorkbench());
    root.querySelector('.console-nav')!.addEventListener('click',event=>{
      const id=(event.target as Element).closest<HTMLElement>('[data-console-page]')?.dataset.consolePage;
      if(id)this.navigate(id as ConsolePage);
    });
    if(main)this.pickerButton.addEventListener('click',()=>this.navigate('connections'));
    this.paint();
  }
  attach(workspaces: Workspaces, connections: ConnectionsUi) {
    this.workspaces=workspaces;this.connections=connections;
    workspaces.setHostVisible(this.route.kind==='workbench');
    workspaces.setHostCallbacks(state=>{this.projection=state;if(this.route.kind==='workbench'&&!state.window.activeWorkspace&&this.main)this.navigate('connections');else this.paint();},()=>this.showWorkbench(),blocked=>this.setBlocked(blocked),()=>this.selectionChanged());
    connections.setNavigation({begin:()=>++this.revision,accept:async(context,revision)=>{
      if(revision!==this.revision||this.disposed)return;
      await coreFocus(context);
      await workspaces.refresh();
      if(revision===this.revision&&!this.disposed)this.showWorkbench();
    }});
    this.paint();
  }
  navigate(page:ConsolePage) {
    if(!this.main)return;
    this.revision++;this.route={kind:'console',page};this.workspaces?.setHostVisible(false);this.paint();this.onPage?.(page);
  }
  selectionChanged(){this.revision++;}
  showWorkbench() {
    if(this.main&&!this.projection?.window.activeWorkspace)return;
    this.revision++;this.route={kind:'workbench'};this.workspaces?.setHostVisible(true);this.paint();
  }
  private paint() {
    const managing=this.route.kind==='console';
    this.root.querySelector<HTMLElement>('#management-console')!.hidden=!managing;
    this.content.hidden=managing;
    const selected=this.route.kind==='console'?this.route.page:'connections';
    this.catalog.hidden=!managing||selected!=='connections';this.page.hidden=!managing||selected==='connections';
    this.root.querySelector('#catalog-tab')!.setAttribute('aria-selected',String(managing));
    for(const node of this.root.querySelectorAll<HTMLElement>('[data-console-page]'))node.setAttribute('aria-current',node.dataset.consolePage===selected?'page':'false');
    this.root.querySelector('#console-title')!.textContent=consolePages.find(p=>p[0]===selected)![1];
    this.root.querySelector<HTMLElement>('#return-workbench')!.hidden=!this.projection?.window.activeWorkspace;
    this.renderStatus();
  }
  renderStatus() {
    const w=this.projection?.workspaces.find(w=>w.context.workspaceId===this.projection?.window.activeWorkspace);
    const t=this.snapshot?.network?.servers.find(t=>t.serverId===w?.context.serverId);
    const rate=(n:number)=>n<1024?`${n.toFixed(0)} B/s`:n<1048576?`${(n/1024).toFixed(1)} KiB/s`:`${(n/1048576).toFixed(1)} MiB/s`;
    this.root.querySelector('#shell-status')!.innerHTML=this.route.kind==='workbench'&&w?`<span class="health-dot ${w.connected?'good':'warning'}"></span><span class="shell-endpoint" title="${e(w.endpoint)}">${e(w.endpoint)}</span><span>${w.connected?'已连接':'已断开'}</span>${w.connected?'':`<button class="sd-status-action" data-reconnect-server="${e(w.context.serverId)}">重新连接</button>`}<span class="grow"></span>${this.workspaces?.layoutSaveFailed?'<button class="sd-status-action" data-shell-retry-layout>重试保存布局</button>':''}<button class="sd-status-action" data-action="status">${this.networkStale?'网络计量失效':t?`↓ ${rate(t.receivedPerSecond)} ↑ ${rate(t.sentPerSecond)}`:'网络计量待更新'}</button>`:`<button class="sd-status-action" data-action="status"><span class="health-dot ${this.error?'warning':'good'}"></span>${this.error?'读取需关注':'SoDeck'}</button><span>${this.snapshot?.connectedServers??'—'} 个连接</span><span class="grow"></span><span>${this.snapshot?`v${e(this.snapshot.version)}`:'正在启动'}</span>`;
    this.root.querySelector('[data-shell-retry-layout]')?.addEventListener('click',()=>void this.workspaces?.retryLayoutSave());
  }
  async read() {
    const results=await Promise.allSettled([core.snapshot(),core.updates(),core.updateOverview(),core.network()]);
    if(this.disposed)return null;
    const [application,updates,overview,network]=results;
    if(application.status==='fulfilled'){application.value.network=this.snapshot?.network;this.snapshot=application.value;applyTheme(this.snapshot.preferences.theme);}
    this.networkStale=network.status==='rejected';
    if(network.status==='fulfilled'&&this.snapshot)this.snapshot.network=network.value;
    this.error=results.flatMap((r,i)=>r.status==='rejected'?[`${['应用状态','更新状态','更新检查','网络计量'][i]}读取失败：${r.reason}`]:[]).join('；');
    this.renderStatus();return {snapshot:this.snapshot,updates,overview};
  }
  private setBlocked(blocked:boolean) {for(const selector of ['.topbar','#management-console','#shell-status'])this.root.querySelector<HTMLElement>(selector)!.inert=blocked;}
  dispose(){this.disposed=true;}
}

import { invoke } from '@tauri-apps/api/core';
const coreFocus=(context:Context)=>invoke('connection',{command:{op:'focus',context}});
