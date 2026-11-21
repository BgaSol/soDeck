import {escape} from './tree.js';
import {PortController} from './ports.js';
const status={running:'运行中',paused:'已暂停',exited:'已停止',created:'未启动',restarting:'重启中',dead:'异常'};
const collections={ports:'端口',mounts:'挂载',networks:'网络',aliases:'别名',subnets:'子网',references:'引用容器'};
export class ResourceInline {
  constructor(root,callbacks){
    this.root=root;this.callbacks=callbacks;this.epoch=0;this.pages=new Map();this.offsets=new Map();
    root.innerHTML='<div class="inline-actions toolbar"></div><div class="inline-summary"></div><div class="inline-collections"></div><details class="inline-more"><summary>更多信息</summary><div class="inline-identity"></div><div class="inline-stats"></div></details><p class="inline-result" role="status" hidden></p>';
    this.portRoot=document.createElement('section');this.portRoot.id='port-tunnels';this.portRoot.setAttribute('aria-label','容器端口隧道');
    this.ports=new PortController(this.portRoot,data=>callbacks.send({...data,scopeEpoch:this.snapshot.scopeEpoch}),text=>callbacks.copy(text));
    this.region=new SoDeckDOM.DomRegion(root);
    root.addEventListener('click',e=>{const b=e.target.closest('[data-inline-action]');if(!b||b.disabled)return;const action=b.dataset.inlineAction;if(action==='page'){this.offsets.set(b.dataset.collection,Number(b.dataset.offset));void this.refresh();}else if(action==='locate'){void callbacks.locate(JSON.parse(b.dataset.resource));}else callbacks.action(action,this.node,b);});
    root.addEventListener('keydown',e=>{if(e.key==='Escape')callbacks.focus();});
  }
  mount(node,snapshot,mount){this.epoch++;this.node=node;this.snapshot=snapshot;this.pages.clear();this.offsets.clear();this.observation=null;this.revision=null;this.ports.mount(mount);}
  enabled(value,visible){this.ready=value;this.visible=visible;this.ports.enabled(value&&visible);if(!visible)this.ports.update([],this.portSnapshot(),false);}
  portSnapshot(){const receipt=this.snapshot?.tunnel,own=receipt?.containerId===this.node?.resource.id;return {...this.snapshot,state:this.observation?.relation?.phase==='ready'?this.snapshot.state:'stale',containerId:this.node?.resource.id,status:this.observation?.info?.status||this.node?.status,blockingTunnel:!own&&['pending','unknown'].includes(receipt?.status),tunnel:own?receipt:null};}
  async refresh(){
    if(this.reading||!this.node||!this.ready||!this.visible)return;this.reading=true;const epoch=this.epoch,resource=this.node.resource;
    try{
      const observation=await this.callbacks.request({action:'browser.object',resource,scopeEpoch:this.snapshot.scopeEpoch});
      if(epoch!==this.epoch||!observation||observation.scopeEpoch!==this.snapshot.scopeEpoch||JSON.stringify(observation.resource)!==JSON.stringify(resource))return;
      if(this.revision!==observation.observationRevision||this.observation?.relation?.phase!==observation.relation?.phase){this.pages.clear();this.offsets.clear();this.revision=observation.observationRevision;}
      if(this.observation?.relation?.count!==observation.relation?.count)this.pages.delete('references');
      this.observation=observation;
      if(observation.info){
        const names=Object.keys(observation.info.counts||{});if(resource.kind!=='container')names.push('references');
        for(const name of names){
          const offset=this.offsets.get(name)||0;
          if(this.pages.get(name)?.observationRevision===this.revision&&this.pages.get(name)?.offset===offset)continue;
          const result=await this.callbacks.request({action:'browser.collection',resource,scopeEpoch:this.snapshot.scopeEpoch,observationRevision:this.revision,collection:name,offset});
          if(epoch!==this.epoch)return;
          if(result?.observationRevision===this.revision&&result.scopeEpoch===this.snapshot.scopeEpoch)this.pages.set(name,{...result,offset});
        }
      }
      this.render();
    }catch(error){if(epoch===this.epoch)this.notice(error.message);}finally{this.reading=false;}
  }
  update(node,snapshot){this.node=node;this.snapshot=snapshot;this.render();void this.refresh();}
  notice(text){const el=this.root.querySelector('.inline-result');el.textContent=text||'';el.hidden=!text;}
  render(){
    if(!this.node)return;const n=this.node,o=this.observation,info=o?.info,tools=this.root.querySelector('.inline-actions');
    this.region.update(tools,this.callbacks.actions(n).map(a=>`<button type="button" data-dom-key="action:${a.action}" data-inline-action="${a.action}" ${a.disabled?'disabled':''} class="${a.danger?'danger':''}">${escape(a.label)}</button>`).join(''),false);
    const field=(label,value,mono=false)=>value==null||value===''?'':`<div class="inline-field"><span class="inline-label">${label}</span><span class="inline-value ${mono?'mono':''}">${escape(value)}</span></div>`;
    let summary='';
    if(info?.kind==='container')summary=(info.imageId?`<div class="inline-field"><span class="inline-label">镜像</span><button type="button" class="inline-reference" data-inline-action="locate" data-resource="${escape(JSON.stringify({kind:'image',id:info.imageId}))}">${escape(info.image)}</button></div>`:field('镜像',info.image))+(['exited','dead'].includes(info.status)?field('状态',`${status[info.status]||info.status}${info.exitCode==null?'':' · 退出码 '+info.exitCode}`):'')+(info.health==='unhealthy'?'<p class="danger">健康检查未通过</p>':'');
    if(info?.kind==='image')summary=field('大小',info.size==null?'未读取':(info.size/1024/1024).toFixed(1)+' MiB')+field('平台',info.platform);
    if(info?.kind==='network'||info?.kind==='volume')summary=field('驱动',info.driver)+field('范围',info.scope)+field('位置',info.mountpoint,true);
    if(!info)summary='<p class="muted">'+escape(o?.error||'正在读取对象信息…')+'</p>';
    if(o?.relation?.phase==='loading'&&info)summary+='<p class="muted">引用信息更新中…</p>';
    if(o?.error&&info)summary+='<p class="danger">'+escape(o.error)+'</p>';
    if(info?.partial)summary+='<p class="muted">部分信息超出上限，已截断。</p>';
    this.region.update(this.root.querySelector('.inline-summary'),summary,false);
    const owner=this.root.querySelector('.inline-collections');
    for(const [name,page]of this.pages){
      let section=[...owner.children].find(el=>el.dataset.collection===name);
      if(!section){section=document.createElement('section');section.dataset.collection=name;section.innerHTML=`<div class="inline-collection-title">${collections[name]}</div><div class="inline-items"></div><div class="inline-pager"></div>`;owner.append(section);if(name==='ports')section.querySelector('.inline-items').append(this.portRoot);}
      if(name==='ports'){this.ports.partial=!!info?.partial;this.ports.update(page.rows,this.portSnapshot(),this.visible);}
      else {
        const content=(page.rows||[]).map(row=>{
          const reference=row.resource;
          const label=name==='mounts'?`${row.source} → ${row.destination}${row.readOnly?' · 只读':''}`:name==='networks'?`${row.name}${row.address?' · '+row.address:''}${row.ipv6?' · '+row.ipv6:''}`:name==='subnets'?`${row.subnet}${row.gateway?' · '+row.gateway:''}`:name==='references'?row.label:String(row);
          return reference?`<button type="button" class="inline-reference" data-inline-action="locate" data-resource="${escape(JSON.stringify(reference))}">${escape(label)}</button>`:`<div class="inline-item ${['mounts','subnets'].includes(name)?'mono':''}">${escape(label)}</div>`;
        }).join('')||'<p class="muted">'+(name==='references'&&o?.relation?.complete!==true?'引用尚未完整读取':'无')+'</p>';
        this.region.update(section.querySelector('.inline-items'),content,false);
      }
      const offset=page.offset||0;
      this.region.update(section.querySelector('.inline-pager'),`${offset?`<button type="button" data-inline-action="page" data-collection="${name}" data-offset="${Math.max(0,offset-5)}">上一页</button>`:''}${page.nextOffset!=null?`<span>${offset+1}–${offset+page.rows.length} / ${page.total}</span><button type="button" data-inline-action="page" data-collection="${name}" data-offset="${page.nextOffset}">更多</button>`:''}`,false);
    }
    this.region.update(this.root.querySelector('.inline-identity'),field('ID',n.resource.id,true)+field('启动',info?.started)+field('退出',info?.finished)+field('重启策略',info?.restart)+`<button type="button" data-inline-action="copy-id">复制${n.resource.kind==='volume'?'名称':'完整 ID'}</button>${n.resource.kind==='container'?'<button type="button" data-inline-action="stats">读取资源快照</button>':''}`,false);
    const stats=o?.stats||{};
    this.region.update(this.root.querySelector('.inline-stats'),Object.entries({CPUPerc:'CPU',MemUsage:'内存 / 限制',NetIO:'网络接收 / 发送',BlockIO:'块读取 / 写入',PIDs:'进程数',unavailable:'状态'}).map(([key,label])=>field(label,stats[key])).join(''),false);
    const op=this.snapshot.operation,terminal=this.snapshot.terminal;
    const result=op&&(op.containerId===n.resource.id||op.resource?.id===n.resource.id)?`${this.callbacks.phase(op.state)}：${op.message||''}`:terminal?.containerId===n.resource.id?({pending:'终端请求待确认',completed:'终端页面已创建',failed:'终端请求失败',unknown:'终端结果未确认'}[terminal.status]||'')+(terminal.error?.message?'：'+terminal.error.message:''):'';
    this.notice(result);
  }
  dispose(){this.epoch++;this.ports.dispose();this.region.dispose();}
}
