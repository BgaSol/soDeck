import {Button,Disclosure} from '@sodeck/ui';
import {useEffect,useRef,useState} from 'react';
import {core,errorMessage,type NodePage,type RouteSummary,type SegmentPage} from '@sodeck/sdk';
import {routeLabel,type DisplayRoute} from './route-client';

export function RoutePath({route,compact=false}:{route:DisplayRoute;compact?:boolean}) {
  const [open,setOpen]=useState(false),[nodes,setNodes]=useState<RouteSummary['nodes']>([]);
  const [cursor,setCursor]=useState<string|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const identity=JSON.stringify(route);
  const activeIdentity=useRef(identity);activeIdentity.current=identity;
  useEffect(()=>{setOpen(false);setNodes([]);setCursor(null);setError('');},[identity]);
  useEffect(()=>{
    if(!open||'nodes' in route)return;
    let live=true;setBusy(true);setError('');
    void core.call<NodePage>('connections.route.v2.nodes',{reference:route.reference}).then(page=>{if(live){setNodes(page.nodes);setCursor(page.nextCursor);}}).catch(e=>{if(live)setError(errorMessage(e));}).finally(()=>{if(live)setBusy(false);});
    return()=>{live=false;};
  },[open,identity]);
  if('nodes' in route)return compact?<Disclosure title="查看完整路径"><ol className="route-node-list">{route.nodes.map(n=><li key={n.serverId}><strong>{n.name}</strong><span>{n.username}@{n.host}:{n.port}</span></li>)}</ol></Disclosure>:<p className="route-preview">{routeLabel(route)}</p>;
  const more=async()=>{
    if(!cursor||busy)return;setBusy(true);setError('');
    try{const page=await core.call<NodePage>('connections.route.v2.nodes',{reference:route.reference,cursor});if(activeIdentity.current!==identity)return;if(page.offset!==nodes.length)throw new Error('路径页次不一致，请重新展开');setNodes(v=>[...v,...page.nodes]);setCursor(page.nextCursor);}catch(e){if(activeIdentity.current===identity)setError(errorMessage(e));}finally{if(activeIdentity.current===identity)setBusy(false);}
  };
  return <section className={compact?'route-inline':'route-preview'}>{!compact&&<strong>{routeLabel(route)}</strong>}<Button variant="quiet" size="compact" onClick={()=>setOpen(v=>!v)} aria-expanded={open}>{open?'收起路径':'查看完整路径'}</Button>
    {open&&<><ol className="route-node-list">{nodes.map((n,i)=><li key={n.serverId}><strong>{i+1}/{route.nodeCount} · {n.role==='target'?'目标':'跳板'} · {n.name}</strong><span>{n.username}@{n.host}:{n.port}</span></li>)}</ol>{busy&&<p role="status">正在读取路径</p>}{error&&<p className="inline-error" role="alert">{error}</p>}{cursor&&<Button disabled={busy} onClick={()=>void more()}>继续显示路径</Button>}</>}
  </section>;
}

export function RouteBridges({connectionId,generation}:{connectionId:string;generation:number}) {
  const [data,setData]=useState<SegmentPage>(),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const load=async(cursor?:string|null)=>{
    if(busy)return;setBusy(true);setError('');
    try{const next=await core.call<SegmentPage>('connections.route.v2.segments',{connectionId,generation,...(cursor?{cursor}:{})});setData(previous=>cursor&&previous?{...next,segments:[...previous.segments,...next.segments]}:next);}catch(e){setError(errorMessage(e));}finally{setBusy(false);}
  };
  return <details onToggle={e=>{if(e.currentTarget.open&&!data)void load();}}><summary>每段中转状态</summary>
    {data&&<ol className="route-node-list">{data.segments.map(s=><li key={s.hopIndex}><strong>第 {s.hopIndex} 段 · {s.metrics.state==='active'?'运行中':s.metrics.state==='failed'?'已断开':'已关闭'}</strong><span>发送 {s.metrics.upstreamBytes} B · 接收 {s.metrics.downstreamBytes} B · 缓冲峰值 {s.metrics.peakBufferedBytes} B</span></li>)}</ol>}
    {error&&<p className="inline-error" role="alert">{error}</p>}<Button disabled={busy} onClick={()=>void load()}>{busy?'正在读取':'刷新中转状态'}</Button>{data?.nextCursor&&<Button disabled={busy} onClick={()=>void load(data.nextCursor)}>查看更多中转</Button>}
  </details>;
}
