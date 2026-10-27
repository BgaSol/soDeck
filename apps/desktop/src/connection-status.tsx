import {RoutePath} from './route-path';
import {routeLabel,type DisplayRoute} from './route-client';
export {routeLabel} from './route-client';
import {Button} from '@sodeck/ui';
import {createPortal} from 'react-dom';
import {useEffect,useState} from 'react';
import {core,decodeRouteStatus,decodeRouteStatusV2,errorMessage,observeState,onCoreNotification,type RouteStatus,type RouteStatusV2,type DependencyPage,type StateSubscription} from '@sodeck/sdk';
import {Dialog} from './dialog';

const connectionState=(state:string)=>({active:'已连接',closed:'已结束',disconnected:'已断开',failed:'连接失败'}[state]??state);
const stages:Record<string,string>={connect:'正在连接',forward:'正在转发到目标',handshake:'正在握手',verify:'正在核验身份',authenticate:'正在认证',active:'已连接',failed:'连接未完成',canceled:'已取消'};
export function ConnectionStatus({workspaceId,generation,onModalChange,multihop=false}:{workspaceId:string;generation:string;multihop?:boolean;onModalChange(open:boolean):void}){
  const [value,setValue]=useState<RouteStatus|RouteStatusV2>(),[error,setError]=useState(''),[details,setDetails]=useState(false);
  useEffect(()=>{onModalChange(details);return()=>onModalChange(false);},[details,onModalChange]);
  useEffect(()=>{
    let live=true,feed:StateSubscription<RouteStatus|RouteStatusV2>|undefined;setValue(undefined);setError('');
    void observeState<RouteStatus|RouteStatusV2>({call:core.call,onNotification:onCoreNotification},{contract:multihop?'connections.route.v2':'connections.route.v1',target:{workspaceId,workspaceGeneration:generation}},multihop?decodeRouteStatusV2:decodeRouteStatus,v=>{if(live){setValue(v);setError('');}},{onError:e=>{if(live)setError(errorMessage(e));}}).then(f=>{if(live)feed=f;else void f.dispose();}).catch(e=>{if(live)setError(errorMessage(e));});
    return()=>{live=false;void feed?.dispose();};
  },[workspaceId,generation,multihop]);
  if(!value)return error?<span title={error}>连接状态暂不可用</span>:null;
  const attempt=value.attempt?.route.routeRevision===value.saved.routeRevision?value.attempt:null;
  const current='currentCount' in value?value.currentCount:value.running.filter(r=>!r.configurationChanged&&r.state==='active').length;
  const changed='changedCount' in value?value.changedCount>0:value.running.some(r=>r.configurationChanged);
  const node=attempt?('current' in attempt?attempt.current:attempt.route.nodes[attempt.hopIndex]):undefined;
  const status=current?'已连接':attempt&&attempt.stage!=='active'?`${node?.role==='jump'?'跳板':'目标'} · ${stages[attempt.stage]??attempt.stage}`:'尚未连接';
  return <><Button variant="quiet" size="compact" className="connection-status" onClick={()=>setDetails(true)} title={routeLabel(value.saved)}>{('nodes' in value.saved?value.saved.nodes.length:value.saved.nodeCount)>1?`经由 ${('nodes' in value.saved?value.saved.nodes.length:value.saved.nodeCount)-1} 台跳板 · `:''}{status}{changed?' · 配置已修改':''}</Button>
    {details&&createPortal(<Dialog title="连接路径" onClose={()=>setDetails(false)}><div className="confirm-body"><RoutePath route={value.saved}/><p role="status">{status}</p>{attempt?.error&&<p className="inline-error">{attempt.error.message}</p>}{changed&&<p>连接配置已修改，新连接生效。现有会话继续使用原路径。</p>}{value.running.map(r=><article key={r.connectionId} className="route-running"><strong>{r.configurationChanged?'原连接':'当前连接'} · {connectionState(r.state)}</strong>{r.configurationChanged&&<RoutePath route={r.route}/>}<details><summary>诊断信息</summary><small>{r.connectionId} · 代次 {r.generation}</small></details></article>)}{'runningCount' in value&&value.runningCount>value.running.length&&<p>共 {value.runningCount} 条运行路径，完整列表可在运行资源查看。</p>}{error&&<p className="inline-error">{error}</p>}</div></Dialog>,document.body)}
  </>;
}

export function DeleteServerDialog({server,onClose,onDeleted}:{server:{id:string;name:string};onClose():void;onDeleted():void}){
  const [impact,setImpact]=useState<{saved:{id:string;name:string;direct?:boolean}[];running:{connectionId:string;route:DisplayRoute}[]}>(),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{let live=true;void core.call<{capabilities:string[]}>('capabilities.list').then(async c=>{
    if(c?.capabilities?.includes('servers.routes.v2')){
      const saved:{id:string;name:string;direct:boolean}[]=[],running:{connectionId:string;route:DisplayRoute}[]=[];let cursor:string|null=null;
      do{const page:DependencyPage=await core.call<DependencyPage>('servers.routes.v2.dependencies',{serverId:server.id,cursor});for(const d of page.dependencies){if(d.kind==='saved')saved.push({...d.server,direct:d.direct});else running.push(d.connection);}cursor=page.nextCursor;}while(cursor);
      return {saved,running};
    }
    return c?.capabilities?.includes('servers.routes.v1')?core.call<NonNullable<typeof impact>>('servers.routes.dependencies',{serverId:server.id}):{saved:[],running:[]};
  }).then(r=>{if(live)setImpact(r);}).catch(e=>{if(live)setError(errorMessage(e));});return()=>{live=false;};},[server.id]);
  const blocked=Boolean(impact?.saved.length);
  return <Dialog title="删除服务器" subtitle={server.name} onClose={()=>{if(!busy)onClose();}}><div className="confirm-body">
    {blocked?<><p>此服务器被以下目标用作跳板，请先修改这些目标的连接方式。</p><ul>{impact?.saved.map(s=><li key={s.id}>{s.name}{s.direct===false?" · 间接依赖":""}</li>)}</ul></>:<p>删除连接档案，并结束以下路径及此服务器当前运行的会话。</p>}
    {!!impact?.running.length&&<ul>{impact.running.map(r=><li key={r.connectionId}><RoutePath route={r.route}/></li>)}</ul>}
    <p className="form-help">远程服务器上的文件不会被删除。</p>{error&&<p className="inline-error" role="alert">{error}</p>}
    <footer><Button disabled={busy} onClick={onClose}>取消</Button><Button variant="danger" className="danger" disabled={!impact||blocked||busy} onClick={async()=>{setBusy(true);try{await core.call('servers.delete',{serverId:server.id,stopSessions:true});onDeleted();}catch(e){setError(errorMessage(e));}finally{setBusy(false);}}}>结束会话并删除</Button></footer>
  </div></Dialog>;
}
