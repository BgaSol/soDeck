import {SharedState} from './sdk/state.js';
const serviceTopic='network.tunnelService.v1',rulesTopic='network.tunnelRules.v1';
const escape=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export const address=value=>value?.host&&value.port?`${value.host.includes(':')?'['+value.host+']':value.host}:${value.port}`:'';
const loopback=host=>host==='::1'||/^127\./.test(host||'');
const targetEqual=(a,b)=>!!a&&!!b&&a.host===b.host&&a.port===b.port;
const active=run=>!!run&&(!run.canStart||!!run.queryError);
function runtimeMatches(row){const a=row.rule,b=row.run?.rule;return !b||a.type===b.type&&JSON.stringify(a.bind)===JSON.stringify(b.bind)&&targetEqual(a.target,b.target);}
export function candidates(records,target){
  const relevant=records.filter(row=>row.rule?.type==='tcpForward'&&(targetEqual(row.rule.target,target)&&loopback(row.rule.bind?.host)||active(row.run)&&targetEqual(row.run.rule?.target,target)&&loopback(row.run.rule?.bind?.host)));
  const blocked=relevant.find(row=>active(row.run)&&(row.run.ownedElsewhere||row.run.queryError||row.run.stopRequested||!runtimeMatches(row)||!['starting','running'].includes(row.run.snapshot?.state)));
  if(blocked)return {rows:[],blocked,reason:blocked.run.ownedElsewhere?'规则正在其他连接上下文运行':blocked.run.queryError?'隧道状态待确认':!runtimeMatches(blocked)?'配置已修改，请在网络隧道中处理':'原监听尚未确认清理'};
  const running=relevant.filter(row=>active(row.run)&&row.run.snapshot?.state==='running');
  const starting=relevant.filter(row=>active(row.run)&&row.run.snapshot?.state==='starting');
  return {rows:running.length?running:starting.length?starting:relevant.filter(row=>!active(row.run))};
}
export class PortController{
  constructor(root,send,copy){this.root=root;this.send=send;this.copy=copy;this.rows=[];this.records=[];this.selected=new Map();this.epoch=0;this.message='';this.inputEnabled=false;root.addEventListener('click',event=>{const button=event.target.closest('[data-port-action]');if(button&&!button.disabled&&this.inputEnabled)void this.act(button).catch(error=>this.notice(error.message));});root.addEventListener('change',event=>{if(event.target.matches('[data-port-choice]')){this.selected.set(event.target.dataset.portChoice,event.target.value);this.render();}});}
  enabled(value){this.inputEnabled=value;if(!value)this.unwatch();else if(this.show)this.watch();this.render();}
  mount(mount){this.dispose();this.mountData=mount;this.inputEnabled=false;this.show=false;this.rows=[];this.snapshot=null;this.root.hidden=true;this.epoch++;this.selected.clear();this.message='';}
  update(rows,snapshot,show){this.rows=rows||[];this.snapshot=snapshot;this.root.hidden=!show;this.show=show;if(show)this.watch();else this.unwatch();this.render();}
  watch(){if(this.client||!this.mountData||!this.inputEnabled)return;const epoch=this.epoch;this.client=new SharedState(this.mountData);
    this.subscription=this.client.watch({topics:[serviceTopic,rulesTopic]},projection=>{if(epoch!==this.epoch||!this.show)return;this.projection=projection;const records=projection?.records?.filter(r=>r.present&&r.producer.kind==='plugin'&&r.producer.pluginId==='sodeck.network-proxy'&&r.value?.version===1)||[];const providers=records.filter(r=>r.topic===serviceTopic&&r.key==='current');this.provider=providers.length===1?providers[0]:null;const producer=this.provider&&JSON.stringify(this.provider.producer);this.records=records.filter(r=>r.topic===rulesTopic&&JSON.stringify(r.producer)===producer).map(r=>r.value);this.render();});
    this.timer=setInterval(()=>{const ready=this.available();if(ready!==this.lastReady){this.lastReady=ready;this.render();}},1000);
  }
  available(){return !!(this.provider?.value.ready&&this.provider.value.publishState==='synced'&&this.projection?.scope.connected&&!this.projection.scope.frozen&&Date.now()-this.provider.value.observedAt<15000);}
  unwatch(){clearInterval(this.timer);this.client?.dispose();this.client=null;this.subscription=null;this.provider=null;this.records=[];this.projection=null;}
  dispose(){this.epoch++;this.unwatch();}
  notice(message){this.message=message||'';this.render();}
  render(){if(!this.show||!this.snapshot)return;const ready=this.available()&&this.inputEnabled,intent=this.snapshot.tunnel;const waiting=this.snapshot.blockingTunnel||['pending','unknown'].includes(intent?.status);const writable=this.snapshot.state==='ready'&&this.snapshot.status==='running'&&!this.snapshot.busy;
    const unavailable=!this.provider?'需要启用支持端口联动的网络隧道插件，重启后可用':'网络隧道状态尚未同步或已过期';
    const result=intent?.result;let feedback=this.message;
    if(!feedback&&waiting)feedback=this.snapshot.blockingTunnel?'另一个容器的隧道请求待确认，请查询原请求':intent.status==='unknown'?'创建结果待确认，请查询原请求':'正在核对端口并办理隧道请求…';
    if(!feedback&&intent?.status==='failed')feedback=intent.error?.message||'隧道请求失败';
    if(!feedback&&result?.outcome==='savedNotStarted')feedback='规则已保存，尚未启动：'+result.message;
    if(!feedback&&result?.outcome==='selectionRequired')feedback='存在多条匹配规则，请选择已有隧道';
    const content=this.rows.map(row=>{
      const match=row.target?candidates(this.records,row.target):{rows:[]};let choices=match.rows||[];let selected=choices.find(item=>item.rule.id===this.selected.get(row.key));if(!selected&&choices.length===1)selected=choices[0];
      const run=selected?.run,entry=run?.snapshot?.address;const running=run?.snapshot?.state==='running'&&!run.queryError&&!run.stopRequested&&!run.ownedElsewhere&&runtimeMatches(selected);
      const current=running&&Date.now()-(run.observedAt||0)<15000;const canCopy=ready&&current&&!!address(entry);const reason=row.reason||match.reason||(!ready?unavailable:!writable?'容器未运行或端口信息待刷新':'');
      let status=selected?run?.queryError?'状态待确认':run?.stopRequested?'正在停止':running?'隧道运行中':run?.snapshot?.state==='starting'?'正在启动':'已保存，未运行':'尚未创建';
      if(running){const check=selected.check;if(check?.checking)status+=' · 正在检查目标 TCP';else if(check&&check.runId===run.runId)status+=check.ok?' · TCP 可达':' · 目标暂不可达：'+(check.message||'检查未完成');else status+=' · 未检查';if(!current)status+=' · 观测已过期';}
      const target=row.binding?address({host:row.binding.hostIp||'0.0.0.0',port:row.binding.hostPort}):'未发布';
      const choice=choices.length>1?`<label class="port-choice">已有隧道<select data-port-choice="${escape(row.key)}" aria-label="选择 ${escape(row.containerPort)} 的已有隧道"><option value="">选择已有隧道</option>${choices.map(item=>`<option value="${escape(item.rule.id)}" ${selected?.rule.id===item.rule.id?'selected':''}>${escape(item.rule.name||item.rule.id)}${address(item.run?.snapshot?.address)?' · '+escape(address(item.run.snapshot.address)):''}</option>`).join('')}</select></label>`:'';
      const known=selected||match.blocked;let actions='';const common=`data-port-key="${escape(row.key)}"`;
      if(canCopy)actions+=`<button type="button" data-port-action="copy" ${common}>复制入口</button>`;
      else actions+=`<button type="button" data-port-action="create" ${common} ${reason||waiting||choices.length>1&&!selected||active(run)?'disabled':''}>${run?.snapshot?.state==='starting'?'正在启动':selected?'启动隧道':'创建隧道'}</button>`;
      if(known)actions+=`<button type="button" class="flat" data-port-action="reveal" ${common} ${waiting?'disabled':''}>查看隧道</button>`;
      const endpoint=canCopy?`<code class="port-entry">${escape(address(entry))}</code>`:'';
      return `<li class="port-row" data-dom-key="port:${escape(row.key)}"><div class="port-line"><code>${escape(target)} → <strong>${escape(row.containerPort)}</strong></code><span class="port-actions">${actions}</span></div><div class="port-state">${endpoint}${endpoint?' · ':''}${escape(reason||status)}${choice}</div></li>`;

    }).join('');
    SoDeckDOM.updateDOM(this.root,`${this.partial?'<p class="notice">端口绑定过多，仅保留前 100 条或 64 KiB 的完整条目。</p>':''}${!ready?`<p class="notice">${escape(unavailable)}</p>`:''}<p class="port-feedback" role="status" ${feedback?'':'hidden'}>${escape(feedback)}${waiting?'<button type="button" class="flat" data-port-action="query">查询状态</button>':''}</p><ul class="port-list">${content||'<li class="muted">没有可用于快捷创建的发布绑定。</li>'}</ul>`);
  }
  async act(button){const action=button.dataset.portAction,epoch=this.epoch;if(action==='query'){const result=await this.send({action:'tunnel.status'});if(epoch===this.epoch&&result){this.snapshot=result;this.render();}return;}
    const row=this.rows.find(row=>row.key===button.dataset.portKey);if(!row)return;const match=candidates(this.records,row.target);const choices=match.rows||[];const selected=choices.find(item=>item.rule.id===this.selected.get(row.key))||(choices.length===1?choices[0]:match.blocked);
    if(action==='copy'){if(!this.available())return;const copied=await this.copy(address(selected?.run?.snapshot?.address));if(copied!==false&&epoch===this.epoch)this.notice('入口已复制');return;}
    const snapshot=this.snapshot;let data={action:action==='reveal'?'tunnel.reveal':'tunnel.create',requestId:crypto.randomUUID(),id:snapshot.containerId,expectedContext:snapshot.context,engineId:snapshot.engine?.id,endpoint:snapshot.endpoint};
    if(action==='reveal'){if(!selected)return;data.revealRule=selected.rule.id;}else{if(!this.available()||!row.binding)return;data.binding=row.binding;data.selectedRule=selected?{id:selected.rule.id,revision:selected.rule.revision}:null;}
    this.message='';button.disabled=true;const result=await this.send(data);if(epoch===this.epoch&&result){this.snapshot=result;this.render();}
  }
}
