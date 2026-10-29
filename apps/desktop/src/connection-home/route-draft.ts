import type {AccessPath,RouteStep} from '@sodeck/sdk';
import type {Catalog} from './model';

export function insertStep(steps:RouteStep[],at:number,kind:RouteStep['kind']):RouteStep[]{
  const stepId=crypto.randomUUID();
  const step:RouteStep=kind==='ssh'?{stepId,kind,serverId:''}:{stepId,kind,proxyId:'',resolve:'proxy'};
  return [...steps.slice(0,at),step,...steps.slice(at)];
}
export function moveStep(steps:RouteStep[],id:string,delta:number):RouteStep[]{
  const next=[...steps],i=next.findIndex(s=>s.stepId===id),to=i+delta;
  if(i<0||to<0||to>=next.length)return next;
  [next[i],next[to]]=[next[to],next[i]];return next;
}
export function stepName(step:RouteStep,c:Catalog):string{
  return step.kind==='ssh'?(c.items.find(i=>i.server.id===step.serverId)?.server.name??'未选择 SSH'):(c.proxies.find(i=>i.proxy.id===step.proxyId)?.proxy.name??'未选择代理');
}
export function arrivalSource(steps:RouteStep[],index:number,c:Catalog):string{
  if(index===0)return '本机';
  const previous=steps[index-1];
  return `${previous.kind==='ssh'?'SSH 主机':'SOCKS5 代理'} ${stepName(previous,c)}`;
}
/** Display IDs and names never alter credential identity. */
export function pathIdentity(path:AccessPath):string{
  return JSON.stringify(path.steps.map(s=>s.kind==='ssh'?{kind:s.kind,serverId:s.serverId}:{kind:s.kind,proxyId:s.proxyId,resolve:s.resolve}));
}
export function pathSummary(path:AccessPath):string{
  const ssh=path.steps.filter(s=>s.kind==='ssh').length,proxy=path.steps.length-ssh;
  return path.steps.length?`${ssh} 个 SSH · ${proxy} 个 SOCKS5 中转`:'直接连接';
}
