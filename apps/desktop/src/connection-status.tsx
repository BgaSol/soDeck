import {decodeDiagnosticList,diagnosticSummary,openConnectionDiagnostics,type DiagnosticList} from '@sodeck/sdk';
import {RoutePath} from './route-path';
import {routeLabel,type DisplayRoute} from './route-client';
export {routeLabel} from './route-client';
import {Button} from '@sodeck/ui';
import {createPortal} from 'react-dom';
import {useEffect,useState} from 'react';
import {core,decodeRouteStatus,errorMessage,observeState,onCoreNotification,type RouteStatus,type DependencyPage,type StateSubscription} from '@sodeck/sdk';
import {Dialog} from './dialog';

const connectionState=(state:string)=>({active:'已连接',closed:'已结束',disconnected:'已断开',failed:'连接失败'}[state]??state);
const stages:Record<string,string>={proxyDial:'正在连接网络代理',proxyNegotiate:'正在协商 SOCKS5',proxyAuthenticate:'正在认证网络代理',proxyConnect:'正在通过网络代理连接目标',connect:'正在连接',forward:'正在转发到目标',handshake:'正在握手',verify:'正在核验身份',authenticate:'正在认证',active:'已连接',failed:'连接未完成',canceled:'已取消'};
export function ConnectionStatus({workspaceId,generation,onModalChange,multihop=false}:{workspaceId:string;generation:string;multihop?:boolean;onModalChange(open:boolean):void}){
  const [diagnostics,setDiagnostics]=useState<DiagnosticList>();
  useEffect(()=>{let live=true,subscription:StateSubscription<DiagnosticList>|undefined;void observeState({call:core.call,onNotification:onCoreNotification},{contract:'connections.diagnostics.v1',target:{workspaceId,workspaceGeneration:generation}},decodeDiagnosticList,v=>{if(live)setDiagnostics(v);}).then(s=>{if(live)subscription=s;else void s.dispose();}).catch(e=>{if(live)setError(errorMessage(e));});return()=>{live=false;void subscription?.dispose();};},[workspaceId,generation]);
  const [value,setValue]=useState<RouteStatus>(),[error,setError]=useState(''),[details,setDetails]=useState(false);
  useEffect(()=>{onModalChange(details);return()=>onModalChange(false);},[details,onModalChange]);
  useEffect(()=>{
    let live=true,feed:StateSubscription<RouteStatus>|undefined;setValue(undefined);setError('');
    void observeState<RouteStatus>({call:core.call,onNotification:onCoreNotification},{contract:'connections.route.v2',target:{workspaceId,workspaceGeneration:generation}},decodeRouteStatus,v=>{if(live){setValue(v);setError('');}},{onError:e=>{if(live)setError(errorMessage(e));}}).then(f=>{if(live)feed=f;else void f.dispose();}).catch(e=>{if(live)setError(errorMessage(e));});
    return()=>{live=false;void feed?.dispose();};
  },[workspaceId,generation,multihop]);
  if(!value)return error?<span title={error}>连接状态暂不可用</span>:null;
  const attempt=value.attempt?.route.routeRevision===value.saved.routeRevision?value.attempt:null;
  const current=value.currentCount;
  const changed=value.changedCount>0;
  const node=attempt?.current;
  const faultDetails=attempt?.error?.details;
  const proxyStage=attempt?.stage.startsWith('proxy')||(faultDetails&&typeof faultDetails==='object'&&'ownerKind' in faultDetails&&faultDetails.ownerKind==='socks5');
  const latest=diagnostics?.attempts.find(a=>a.purpose==='session');
  const status=latest?.state==='failed'?`${current?'已有连接仍可用 · 本次新建失败：':''}${diagnosticSummary(latest)}`:current?(value.saved.proxyCount>0?'目标已连接 · 经由 SOCKS5 网络代理':'已连接'):attempt&&attempt.stage!=='active'?`${proxyStage?'网络代理':node?.role==='jump'?'跳板':'目标'} · ${stages[attempt.stage]??attempt.stage}`:'尚未连接';
  return <><Button variant="quiet" size="compact" className="connection-status" onClick={()=>setDetails(true)} title={routeLabel(value.saved)}>{value.saved.stepCount>1?`经由 ${value.saved.stepCount-1} 个中转 · `:''}{status}{changed?' · 配置已修改':''}</Button>
    {details&&createPortal(<Dialog title="连接路径" onClose={()=>setDetails(false)}><div className="confirm-body"><RoutePath route={value.saved}/>{diagnostics?.attempts.map(a=><article key={a.reference.attemptId}><strong>{a.source} · {diagnosticSummary(a)}</strong><Button variant="quiet" onClick={()=>{setDetails(false);void openConnectionDiagnostics(core,a.reference).catch(e=>setError(errorMessage(e)));}}>查看连接详情</Button></article>)}<p role="status">{status}</p>{!latest&&attempt?.error&&<p className="inline-error">{attempt.error.message}</p>}{changed&&<p>连接配置已修改，新连接生效。现有会话继续使用原路径。</p>}{value.running.map(r=><article key={r.connectionId} className="route-running"><strong>{r.configurationChanged?'原连接':'当前连接'} · {connectionState(r.state)}</strong>{r.configurationChanged&&<RoutePath route={r.route}/>}<details><summary>诊断信息</summary><small>{r.connectionId} · 代次 {r.generation}</small></details></article>)}{'runningCount' in value&&value.runningCount>value.running.length&&<p>共 {value.runningCount} 条运行路径，完整列表可在运行资源查看。</p>}{error&&<p className="inline-error">{error}</p>}</div></Dialog>,document.body)}
  </>;
}

export function DeleteServerDialog({server,onClose,onDeleted}:{server:{id:string;name:string};onClose():void;onDeleted():void}){
  const [impact,setImpact]=useState<{saved:{id:string;name:string;direct?:boolean}[];running:{connectionId:string;route:DisplayRoute}[]}>(),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{let live=true;void core.call<{capabilities:string[]}>('capabilities.list').then(async c=>{
    if(c?.capabilities?.includes('servers.routes.v2')){
      const saved:{id:string;name:string;direct:boolean}[]=[],running:{connectionId:string;route:DisplayRoute}[]=[];let cursor:string|null=null;
      do{const page:DependencyPage=await core.call<DependencyPage>('servers.routes.dependencies',{serverId:server.id,cursor});for(const d of page.dependencies){if(d.kind==='saved')saved.push({...d.server,direct:d.direct});else running.push(d.connection);}cursor=page.nextCursor;}while(cursor);
      return {saved,running};
    }
    return {saved:[],running:[]};
  }).then(r=>{if(live)setImpact(r);}).catch(e=>{if(live)setError(errorMessage(e));});return()=>{live=false;};},[server.id]);
  const blocked=Boolean(impact?.saved.length);
  return <Dialog title="删除服务器" subtitle={server.name} onClose={()=>{if(!busy)onClose();}}><div className="confirm-body">
    {blocked?<><p>此服务器被以下目标用作跳板，请先修改这些目标的连接方式。</p><ul>{impact?.saved.map(s=><li key={s.id}>{s.name}{s.direct===false?" · 间接依赖":""}</li>)}</ul></>:<p>删除连接档案，并结束以下路径及此服务器当前运行的会话。</p>}
    {!!impact?.running.length&&<ul>{impact.running.map(r=><li key={r.connectionId}><RoutePath route={r.route}/></li>)}</ul>}
    <p className="form-help">远程服务器上的文件不会被删除。</p>{error&&<p className="inline-error" role="alert">{error}</p>}
    <footer><Button disabled={busy} onClick={onClose}>取消</Button><Button variant="danger" className="danger" disabled={!impact||blocked||busy} onClick={async()=>{setBusy(true);try{await core.call('servers.delete',{serverId:server.id,stopSessions:true});onDeleted();}catch(e){setError(errorMessage(e));}finally{setBusy(false);}}}>结束会话并删除</Button></footer>
  </div></Dialog>;
}
