import {workerClient, type WorkerTransport} from '@sodeck/sdk/worker';
import {RpcBudget} from '@sodeck/sdk/bounded-exec';
import {workerClock, type Clock} from '@sodeck/sdk/runtime';
import {Controller} from './controller';
import {Scheduler} from './scheduler';
import {keys, string, object, options, invalid} from './validation';
import {monitorObservation} from './observation';
import type {Invocation} from '@sodeck/sdk/observation';
declare const __MONITOR_VERSION__: string;
const runtime=globalThis as typeof globalThis&{sodeck:WorkerTransport&{clock?:Clock};sodeckPlugin:unknown};
const transport=workerClient(runtime.sodeck),budget=new RpcBudget(transport);
const bridge={call:budget.call.bind(budget),onNotification:transport.onNotification.bind(transport)};
const clock=workerClock(runtime.sodeck),scheduler=new Scheduler(),controllers=new Map<string,Controller>();
const diagnostic=monitorObservation(bridge);
const retired=new Set<Controller>();
let unsubscribe:(()=>void)|undefined;
let healthTimer:ReturnType<typeof setTimeout>|undefined,stopping=false;
let instanceId='';
async function retire(c:Controller){retired.add(c);await c.dispose();if(await c.cleanup())retired.delete(c);}
async function health(){
  if(stopping)return;
  for(const c of controllers.values()){const s=c.health();diagnostic.state(c.viewId,s.state,s.activeJobs,s.cacheBytes,s.cleanupPending);}
  for(const c of retired)if(await c.cleanup())retired.delete(c);
  if(!stopping)healthTimer=setTimeout(()=>void health(),5000);
}
function cacheBudget() {
  let bytes=[...controllers.values(),...retired].reduce((sum,c)=>sum+c.model.bytes(),0);
  for(const c of [...controllers.values()].filter(c=>!c.visible).sort((a,b)=>a.lastVisible-b.lastVisible)){
    if(bytes<=33554432)break;const before=c.model.bytes();c.releaseHistory();bytes-=before-c.model.bytes();
  }
  return bytes<=33554432;
}
function controller(value:Record<string,unknown>) {
  const c=controllers.get(string(value.viewId,'视图'));if(!c||c.disposed)throw Object.assign(Error('资源监控视图已关闭'),{code:'STALE_WORKSPACE'});return c;
}
function initialize(p:Record<string,unknown>){
      const viewId=string(p.viewId),serverId=string(p.serverId),workspaceId=string(p.workspaceId),workspaceGeneration=string(p.workspaceGeneration);
      let c=controllers.get(viewId);
      if(c&&(c.scope.workspaceGeneration!==workspaceGeneration||c.serverId!==serverId))throw Object.assign(Error('视图身份不匹配'),{code:'STALE_WORKSPACE'});
      if(!c){
        if(controllers.size+retired.size>=64)return {limited:true,message:'监控视图容量已满，请关闭一个监控视图后重新打开'};
        c=new Controller(bridge,clock,scheduler,{workspaceId,workspaceGeneration},serverId,viewId,cacheBudget,(lane,ms,failed)=>diagnostic.collection(lane,ms,failed),instanceId);controllers.set(viewId,c);
        void diagnostic.bind(viewId,serverId,workspaceId).catch(()=>{});
        void c.attach().catch(()=>{});
      }
      return c.snapshot();
}
runtime.sodeckPlugin={
  prepare(context?:{instanceId?:string}){instanceId=context?.instanceId??'';healthTimer=setTimeout(()=>void health(),5000);unsubscribe=bridge.onNotification(event=>{if(event.type==='workspace.closed'){const generation=(event.payload as {workspaceGeneration?:string})?.workspaceGeneration;for(const [id,c] of controllers)if(c.scope.workspaceGeneration===generation){controllers.delete(id);diagnostic.remove(id);void retire(c);}}});},
  activate(){},
  async invoke(command:string,params:unknown,invocation?:Invocation) {
    const p=object(params);
    if(command==='lifecycle.initialize'){
      return initialize(p);
    }
    if(command==='lifecycle.prepareClose')return {allow:true};
    if(command==='lifecycle.presentation'){const c=controllers.get(String(p.viewId));if(c)c.presentation(p.visible===true,Number(p.revision));return {accepted:true};}
    if(command==='lifecycle.close'){const id=String(p.viewId),c=controllers.get(id);controllers.delete(id);diagnostic.remove(id);if(c)await retire(c);return {closed:true};}
    if(command==='lifecycle.cancel'){await bridge.call('operations.cancel',{operationId:p.operationId});return {canceled:true};}
    if(command==='command.execute'){
      const ctx=object(p.context);if(!['monitor.open','monitor.connect'].includes(String(p.commandId)))invalid('未知资源监控命令');
      diagnostic.action(p.commandId==='monitor.open'?'open':'connect',undefined,invocation);
      const opened=await bridge.call<{viewId:string}>('views.open',{viewType:'monitor.resources',workspaceId:string(ctx.workspaceId),operationId:string(ctx.operationId),title:'资源监控'});
      if(p.commandId==='monitor.connect'){if(!controllers.has(opened.viewId))initialize({...ctx,viewId:opened.viewId});const c=controllers.get(opened.viewId);if(!c)throw Object.assign(Error('监控视图容量已满'),{code:'CAPACITY'});await c.connect(string(ctx.operationId));}return opened;
    }
    const c=controller(p);
    if(command==='monitor.snapshot'){
      keys(p,['viewId','knownRevision']);if(p.knownRevision!==undefined&&!/^\d{1,32}$/.test(string(p.knownRevision)))invalid('修订无效');
      return c.snapshot(p.knownRevision as string|undefined);
    }
    if(command==='monitor.setOptions'){keys(p,['viewId','options']);const o=options(p.options);c.options(o);if(o.paused!==undefined)diagnostic.action('pause',c.viewId,invocation);}
    else if(command==='monitor.select'){
      keys(p,['viewId','kind','id','page','readCapacity','expanded']);if(!['network','disk','mount'].includes(String(p.kind)))invalid('选择类型无效');
      if(p.page!==undefined&&(!Number.isInteger(p.page)||Number(p.page)<0||Number(p.page)>15))invalid('分页无效');
      if(p.readCapacity!==undefined&&typeof p.readCapacity!=='boolean')invalid('容量选项无效');
      if(p.expanded!==undefined&&typeof p.expanded!=='boolean')invalid('展开状态无效');
      c.select(p.kind as 'network'|'disk'|'mount',p.id===null?null:string(p.id),p.page as number|undefined,p.readCapacity===true,p.expanded as boolean|undefined);diagnostic.action('select',c.viewId,invocation);
    }else if(command==='monitor.refresh'||command==='monitor.connect'){
      keys(p,['viewId','actionId']);const id=string(p.actionId);if(command==='monitor.refresh')c.refresh(id);else await c.connect(id);diagnostic.action(command==='monitor.refresh'?'refresh':'connect',c.viewId,invocation);
    }else if(command==='monitor.diagnostics'){keys(p,['viewId']);return {version:__MONITOR_VERSION__,controllers:controllers.size,rpc:budget.counts(),scheduler:scheduler.counts(),cacheBytes:[...controllers.values()].reduce((n,c)=>n+c.model.bytes(),0)};}
    else invalid('未知资源监控操作');
    return {accepted:true};
  },
  async stop(){stopping=true;clearTimeout(healthTimer);unsubscribe?.();await Promise.all([...controllers.values(),...retired].map(c=>c.dispose()));controllers.clear();retired.clear();await diagnostic.stop();},
};
