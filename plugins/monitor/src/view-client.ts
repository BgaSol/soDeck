import {useEffect,useRef,useState} from 'react';
import {core,context,observeState,presentation,onCoreNotification,connectionCommandTimeout} from '@sodeck/sdk';
import {diagnosticReference,errorMessage,type DiagnosticRef} from '@sodeck/sdk';
import {snapshot as decode} from './validation';
import type {Snapshot} from './protocol';
const ctx=context();
export function useMonitor() {
  const [snapshot,setSnapshot]=useState<Snapshot>(),[error,setError]=useState(''),[elapsed,setElapsed]=useState(0);
  const [errorRef,setErrorRef]=useState<DiagnosticRef>();
  const visible=useRef(false),mounted=useRef(false),pending=useRef(false),received=useRef(0),revision=useRef<string>(undefined),timer=useRef<ReturnType<typeof setTimeout>>(undefined);
  const poll=useRef(async()=>{});
  const invoke=<T,>(command:string,args:Record<string,unknown>={})=>core.call<T>('plugin.invoke',{command,args:{...args,viewId:ctx.viewId}});
  poll.current=async()=>{
    if(!mounted.current||!visible.current||pending.current)return;
    pending.current=true;clearTimeout(timer.current);
    try{
      const result=decode(await invoke('monitor.snapshot',{...(revision.current?{knownRevision:revision.current}:{})}),ctx.viewId!,ctx.workspaceGeneration!);
      if(!mounted.current)return;
      if(!('unchanged'in result)){revision.current=result.revision;received.current=performance.now();setElapsed(0);setSnapshot(result);}
      else setElapsed(Math.max(0,performance.now()-received.current));
      setError('');
    }catch(error){if(mounted.current)setError(String((error as Error).message));}
    finally{pending.current=false;if(mounted.current&&visible.current)timer.current=setTimeout(()=>void poll.current(),1000);}
  };
  useEffect(()=>{
    mounted.current=true;let disposed=false,subscription:Awaited<ReturnType<typeof observeState>>|undefined;
    const off=onCoreNotification(event=>{
      if(event.type==='view.prepareMove'){
        visible.current=false;clearTimeout(timer.current);
        const scrollTop=document.querySelector('.monitor-content')?.scrollTop??0;
        const expanded=Array.from(document.querySelectorAll('main details')).map((e,index)=>({index,open:(e as HTMLDetailsElement).open}));
        void core.call('view.snapshotChunk',{operationId:(event.payload as {operationId:string}).operationId,index:0,total:1,data:JSON.stringify({version:1,scrollTop,expanded})}).catch(error=>setError(String(error)));
      }
      if(event.type==='view.moveCancelled'){visible.current=true;void poll.current();}
    });
    void (async()=>{
      const initial=await core.call<unknown>('view.initialize');
      if(initial&&typeof initial==='object'&&'limited'in initial&&initial.limited===true&&!('protocolVersion'in initial)){
        setError('监控视图容量已满，请关闭一个监控视图后重新打开');await core.call('view.ready');return;
      }
      const result=decode(initial,ctx.viewId!,ctx.workspaceGeneration!);
      if(disposed)return;
      if(!('unchanged'in result)){setSnapshot(result);revision.current=result.revision;received.current=performance.now();}
      await core.call('view.ready');if(disposed)return;
      const value=await observeState({call:(method,params)=>core.call(method,params),onNotification:onCoreNotification},{contract:'view.presentation.v1',target:{scope:{workspaceId:ctx.workspaceId!,workspaceGeneration:ctx.workspaceGeneration!},viewId:ctx.viewId!}},presentation,value=>{
        visible.current=value.visible;if(value.visible)void poll.current();else clearTimeout(timer.current);
      },{onError:error=>{setError(String((error as Error).message));visible.current=false;clearTimeout(timer.current);}});
      if(disposed){await value.dispose();return;}subscription=value;
      requestAnimationFrame(()=>{
        const saved=ctx.snapshot as {version?:number;scrollTop?:number;expanded?:{index:number;open:boolean}[]}|undefined;
        if(saved?.version!==1||disposed)return;
        const details=document.querySelectorAll('main details');for(const item of saved.expanded??[])if(details[item.index])(details[item.index] as HTMLDetailsElement).open=item.open===true;
        const area=document.querySelector('.monitor-content');if(area&&Number.isFinite(saved.scrollTop))area.scrollTop=Math.max(0,saved.scrollTop!);
      });
    })().catch(error=>{if(!disposed)setError(String((error as Error).message));});
    return()=>{disposed=true;mounted.current=false;off();clearTimeout(timer.current);void subscription?.dispose();};
  },[]);
  const action=async(command:string,args:Record<string,unknown>={})=>{
    try{
      if(command==='monitor.connect')await core.call('commands.execute',{pluginId:'sodeck.monitor',commandId:'monitor.connect',workspaceId:ctx.workspaceId,viewId:ctx.viewId,operationId:args.actionId},await connectionCommandTimeout(core,ctx.serverId,ctx.workspaceId));
      else await invoke(command,args);
      revision.current=undefined;await poll.current();
    }
    catch(error){setError(errorMessage(error));setErrorRef(diagnosticReference(error));}
  };
  return {snapshot,error,errorRef,elapsed,action};
}
