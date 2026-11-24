import { escape, icon } from './tree.js';
import { PortController } from './ports.js';
import { ResourceDetailsData } from './resource-details-data.js';

const status = { running:'运行中', paused:'已暂停', exited:'已停止', created:'未启动', restarting:'重启中', dead:'异常' };
const health = { healthy:'健康', unhealthy:'健康检查未通过', starting:'健康检查中' };
const labels = { ports:'端口', mounts:'挂载', networks:'网络', aliases:'别名', subnets:'子网', references:'引用容器' };
const field = (label, value, mono=false) => value == null || value === '' ? '' : `<div class="details-field"><span class="details-label">${label}</span><span class="details-value ${mono?'mono':''}">${escape(value)}</span></div>`;
const reference = (label, resource) => `<button type="button" class="details-reference" data-inline-action="locate" data-resource="${escape(JSON.stringify(resource))}">${escape(label)}</button>`;
const collection = name => `<section class="details-collection" data-collection="${name}"><div class="details-collection-title">${labels[name]}</div><p class="details-error" role="status" hidden></p><div class="details-items"></div><div class="details-pager"></div></section>`;

export class ResourceDetails {
  constructor(root, callbacks) {
    this.root = root;
    this.callbacks = callbacks;
    this.data = new ResourceDetailsData(callbacks.request, () => this.render());
    root.innerHTML = `<div class="details-actions"><div data-action-group="read"></div><div data-action-group="maintain"></div></div>
      <div class="details-tabs" role="tablist" aria-label="容器详情">${['overview','ports','mounts','networks'].map(tab => `<button type="button" role="tab" id="docker-details-${tab}-tab" aria-controls="docker-details-${tab}" aria-selected="${tab==='overview'}" tabindex="${tab==='overview'?0:-1}" data-details-tab="${tab}"><span>${tab==='overview'?'概览':labels[tab]}</span>${tab==='overview'?'':' <span class="details-tab-count">—</span>'}</button>`).join('')}</div>
      <p class="details-observation" role="status" hidden></p>
      <div class="details-panels">
        <section id="docker-details-overview" role="tabpanel" aria-labelledby="docker-details-overview-tab"><div class="details-summary"></div><details class="details-more"><summary>更多信息</summary><div class="details-identity"></div><div class="details-stats"></div></details></section>
        ${['ports','mounts','networks'].map(name=>`<section id="docker-details-${name}" role="tabpanel" aria-labelledby="docker-details-${name}-tab" hidden>${collection(name)}</section>`).join('')}
        <div class="details-other"></div>
      </div>
      <div class="details-feedback"><p class="details-result" role="status" hidden></p><button type="button" data-inline-action="query-tunnel" hidden>查询原请求</button></div><div class="details-confirmation"></div>`;
    this.region = new SoDeckDOM.DomRegion(root);
    this.portRoot = document.createElement('section');
    this.portRoot.id = 'port-tunnels';
    this.portRoot.setAttribute('aria-label', '容器端口隧道');
    root.querySelector('[data-collection=ports] .details-items').append(this.portRoot);
    this.ports = new PortController(this.portRoot, data => callbacks.send({...data, scopeEpoch:this.snapshot.scopeEpoch}), callbacks.copy);
    root.addEventListener('click', event => {
      const button = event.target.closest('button');
      if (!button || button.disabled || !this.ready) return;
      if (button.dataset.detailsTab) { this.selectTab(button.dataset.detailsTab); return; }
      if (button.dataset.detailsPage) {
        if (this.data.navigate(button.dataset.collection, button.dataset.detailsPage)) this.changedNavigation();
        return;
      }
      const action = button.dataset.inlineAction;
      if (action === 'locate') void callbacks.locate(JSON.parse(button.dataset.resource));
      else if (action === 'query-tunnel') void callbacks.send({action:'tunnel.status'});
      else if (action) void callbacks.action(action, this.node, button);
    });
    root.addEventListener('keydown', event => {
      const tab = event.target.closest('[data-details-tab]');
      if (tab && ['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) {
        event.preventDefault();event.stopPropagation();
        const tabs = [...root.querySelectorAll('[data-details-tab]')], index = tabs.indexOf(tab);
        const next = event.key==='Home'?0:event.key==='End'?tabs.length-1:(index+(event.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;
        tabs[next].focus({preventScroll:true});this.selectTab(tabs[next].dataset.detailsTab);
      }
      if (event.key==='Escape' && !event.target.closest('#browser-panel')) callbacks.focus();
    });
  }
  mount(node, snapshot, mount, navigation) {
    this.node = node;this.snapshot = snapshot;
    this.container = node.resource.kind === 'container';
    this.data.mount(node, snapshot, navigation);this.ports.mount(mount);
    this.root.querySelector('.details-tabs').hidden = !this.container;
    for (const panel of this.root.querySelectorAll('[role=tabpanel]')) {
      if (!this.container) { panel.removeAttribute('role');panel.removeAttribute('aria-labelledby'); }
    }
  }
  enabled(value, visible) {
    this.ready = value;this.visible = visible;
    this.data.enabled(value && visible);
    this.syncPorts();
  }
  update(node, snapshot) {
    this.node = node;this.snapshot = snapshot;this.data.update(node, snapshot);
    this.render();void this.data.refresh();
  }
  navigation() { return this.data.navigation(); }
  attachConfirmation(panel) { this.root.querySelector('.details-confirmation').append(panel); }
  selectTab(tab) { if (this.ready && this.data.select(tab)) this.changedNavigation(); }
  changedNavigation() {
    this.render();this.callbacks.navigation(this.navigation());void this.data.refresh();
  }
  portSnapshot() {
    const receipt=this.snapshot.tunnel, own=receipt?.containerId===this.node.resource.id;
    return {...this.snapshot, state:this.data.observation?.relation?.phase==='ready'?this.snapshot.state:'stale', containerId:this.node.resource.id,
      status:this.data.observation?.info?.status||this.node.status, blockingTunnel:!own&&['pending','unknown'].includes(receipt?.status), tunnel:own?receipt:null};
  }
  syncPorts() {
    if (!this.node) return;
    const active=!!(this.ready && this.visible && this.container && this.data.tab==='ports');
    this.ports.enabled(active);
    this.ports.update(this.data.page('ports')?.rows||[],this.portSnapshot(),active);
  }
  render() {
    if (!this.node) return;
    const observation=this.data.observation, info=observation?.info, n=this.node;
    const actions=this.callbacks.actions(n);
    for (const group of ['read','maintain']) this.region.update(this.root.querySelector(`[data-action-group=${group}]`),actions.filter(action=>['logs','terminal'].includes(action.action)===(group==='read')).map(action=>`<button type="button" data-dom-key="action:${action.action}" data-inline-action="${action.action}" ${action.disabled?'disabled':''} class="${action.danger?'danger':''}">${['logs','terminal'].includes(action.action)?icon(action.action):''}${escape(action.label)}</button>`).join(''),false);
    for (const tab of this.root.querySelectorAll('[data-details-tab]')) {
      const selected=tab.dataset.detailsTab===this.data.tab;
      tab.setAttribute('aria-selected',String(selected));tab.tabIndex=selected?0:-1;tab.disabled=!this.ready;
      const count=tab.querySelector('.details-tab-count');
      if (count) count.textContent=info?.counts?.[tab.dataset.detailsTab]??'—';
      this.root.querySelector('#'+tab.getAttribute('aria-controls')).hidden=this.container&&!selected || !this.container&&tab.dataset.detailsTab!=='overview';
    }
    const notice=this.root.querySelector('.details-observation');
    notice.textContent=[this.data.error,observation?.error,!info?'正在读取对象信息…':observation?.relation?.phase!=='ready'?'对象信息待更新':'',info?.partial?'部分信息已截断，数字为已保留条目。':''].filter(Boolean).join(' · ');
    notice.hidden=!notice.textContent;
    let summary='';
    if (info?.kind==='container') summary=`<div class="details-field"><span class="details-label">镜像</span><div class="details-value">${info.imageId?reference(info.image,{kind:'image',id:info.imageId}):escape(info.image)}</div></div>`+
      field('状态',`${status[info.status]||info.status}${info.exitCode!=null&&['exited','dead'].includes(info.status)?' · 退出码 '+info.exitCode:''}`)+field('健康',health[info.health]||'未配置健康检查')+field('启动',info.started)+field('退出',info.finished)+field('重启策略',info.restart);
    if (info?.kind==='image') summary=field('大小',info.size==null?'未读取':(info.size/1024/1024).toFixed(1)+' MiB')+field('平台',info.platform);
    if (['network','volume'].includes(info?.kind)) summary=field('驱动',info.driver)+field('范围',info.scope)+field('位置',info.mountpoint,true);
    this.region.update(this.root.querySelector('.details-summary'),summary,false);
    this.region.update(this.root.querySelector('.details-identity'),field('ID',n.resource.id,true)+`<button type="button" data-inline-action="copy-id" ${!this.ready?'disabled':''}>复制${n.resource.kind==='volume'?'名称':'完整 ID'}</button>${this.container?`<button type="button" data-inline-action="stats" ${!this.ready||this.snapshot.state!=='ready'?'disabled':''}>读取资源快照</button>`:''}`,false);
    const stats=observation?.stats||{};
    this.region.update(this.root.querySelector('.details-stats'),Object.entries({CPUPerc:'CPU',MemUsage:'内存 / 限制',NetIO:'网络接收 / 发送',BlockIO:'块读取 / 写入',PIDs:'进程数',unavailable:'状态'}).map(([key,label])=>field(label,stats[key])).join(''),false);
    if (this.container) {
      if (this.data.tab!=='overview') this.renderCollection(this.data.tab,this.root.querySelector(`[data-collection=${this.data.tab}]`));
    } else {
      const owner=this.root.querySelector('.details-other');
      for (const name of this.data.collections()) {
        let section=owner.querySelector(`[data-collection=${name}]`);
        if (!section) {const template=document.createElement('template');template.innerHTML=collection(name);section=template.content.firstElementChild;owner.append(section);}
        this.renderCollection(name,section);
      }
    }
    this.syncPorts();this.renderFeedback();
  }
  renderCollection(name, section) {
    const page=this.data.page(name), error=this.data.errors.get(name), notice=section.querySelector('.details-error');
    notice.textContent=error||'';notice.hidden=!error;
    if (name!=='ports') {
      const rows=(page?.rows||[]).map((row,index)=>{
        const key=escape(row.key||row.resource?.id||[name,page.offset,index].join(':'));
        let content;
        if (name==='mounts') content=`<div class="details-mount"><div>${row.resource?reference(row.source,row.resource):`<span class="mono">${escape(row.source)}</span>`}</div><div class="mono details-destination">→ ${escape(row.destination)}${row.readOnly?' · 只读':''}</div></div>`;
        else if (name==='networks') content=`<div class="details-network">${row.resource?reference(row.name,row.resource):escape(row.name)}<span class="mono">${escape([row.address,row.ipv6].filter(Boolean).join(' · '))}</span></div>`;
        else {const label=name==='subnets'?`${row.subnet}${row.gateway?' · '+row.gateway:''}`:name==='references'?row.label:String(row);content=row.resource?reference(label,row.resource):`<span class="${name==='subnets'?'mono':''}">${escape(label)}</span>`;}
        return `<div class="details-item" data-dom-key="${key}">${content}</div>`;
      }).join('');
      this.region.update(section.querySelector('.details-items'),rows||`<p class="muted">${!page?'正在读取…':name==='references'&&this.data.observation?.relation?.complete!==true?'引用尚未完整读取':'无'}</p>`,false);
    }
    const offset=page?.offset||0;
    this.region.update(section.querySelector('.details-pager'),page?`${offset?`<button type="button" data-details-page="previous" data-collection="${name}">上一页</button>`:''}${page.nextOffset!=null||offset?`<span>${offset+1}–${offset+page.rows.length} / ${page.total}</span>`:''}${page.nextOffset!=null?`<button type="button" data-details-page="next" data-collection="${name}">更多</button>`:''}`:'',false);
  }
  renderFeedback() {
    const n=this.node, feedback=[], op=this.snapshot.operation, terminal=this.snapshot.terminal, tunnel=this.snapshot.tunnel;
    if (op&&(op.containerId===n.resource.id||op.resource?.id===n.resource.id)) feedback.push(`${this.callbacks.phase(op.state)}：${op.message||''}`);
    if (terminal?.containerId===n.resource.id) feedback.push(({pending:'终端请求待确认',completed:'终端页面已创建',failed:'终端请求失败',unknown:'终端结果未确认'}[terminal.status]||'')+(terminal.error?.message?'：'+terminal.error.message:''));
    const own=tunnel?.containerId===n.resource.id;
    if (own) feedback.push(({pending:'隧道请求办理中',completed:'隧道请求已办理',failed:'隧道请求失败',unknown:'隧道结果未确认'}[tunnel.status]||'')+(tunnel.error?.message?'：'+tunnel.error.message:''));
    const result=this.root.querySelector('.details-result');result.textContent=feedback.filter(Boolean).join('\n');result.hidden=!result.textContent;
    const query=this.root.querySelector('[data-inline-action=query-tunnel]');query.hidden=!own||!['pending','unknown'].includes(tunnel.status);query.disabled=!this.ready;
  }
  dispose() { this.node=null;this.data.dispose();this.ports.dispose();this.region.dispose(); }
}
