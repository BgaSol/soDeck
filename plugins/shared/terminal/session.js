import {SharedState} from '../state.js';

export const sameContext = (a,b) => !!a&&!!b&&['workspaceId','serverId','connectionId','generation'].every(key=>a[key]===b[key]);
const newer = (a,b) => !b || BigInt(a.revision)>BigInt(b.revision);

export function createSessionAdapter() {
  let mount,descriptor,runtime,client,watch,disposed=false,epoch=0,recovering=false,retryTimer,retries=0;
  const listeners=new Set();
  const emit=()=>{for(const listener of listeners)listener({descriptor,runtime});};
  const unresolved=()=>descriptor?.preparation==='preparing'||descriptor?.ptyId&&(!runtime||['opening','closing'].includes(runtime.state));
  function receive(snapshot) {
    const next=snapshot?.session;
    if(next?.schemaVersion===1&&next.viewId===mount.viewId&&(!descriptor||sameContext(next.context,descriptor.context))&&newer(next,descriptor)) {
      if(descriptor?.ptyId&&next.ptyId!==descriptor.ptyId)throw new Error('终端目标已改变，不能替换已有会话');
      descriptor=next;
    }
    const native=snapshot?.runtime;
    if(native&&native.id===descriptor?.ptyId&&native.viewId===mount.viewId&&sameContext(native.context,descriptor.context)&&newer(native,runtime))runtime=native;
    emit();
  }
  async function recover() {
    if(recovering||disposed)return;recovering=true;const ticket=epoch;
    try {const result=await sodeck.request('event',{type:'terminal.status'});if(ticket===epoch&&!disposed)receive(result.data||result);}
    finally {recovering=false;if(ticket===epoch&&unresolved())scheduleRecovery();}
  }
  function scheduleRecovery(){if(disposed||retryTimer||retries>=12)return;retryTimer=setTimeout(()=>{retryTimer=undefined;retries++;void recover().catch(()=>{});},Math.min(5000,500*2**Math.min(retries,4)));}
  return {
    current:()=>({descriptor,runtime}),
    subscribe(listener){listeners.add(listener);return()=>listeners.delete(listener);},
    mount(value) {
      client?.dispose();clearTimeout(retryTimer);retryTimer=undefined;retries=0;mount=value;disposed=false;epoch++;descriptor=undefined;runtime=undefined;
      receive({session:value.data.session,runtime:value.data.runtime});
      if(!descriptor)throw new Error('终端会话描述缺失或版本不支持');
      client=new SharedState(value);
      if(!client.available)return;
      watch=client.watch({topics:['terminal.session.v1','terminal.runtime.v1']},projection=>{
        if(!projection){runtime=null;emit();scheduleRecovery();return;}
        const records=projection.records;
        const session=records.find(r=>r.present&&r.topic==='terminal.session.v1'&&r.key==='current'&&r.producer.viewId===mount.viewId)?.value;
        const native=records.find(r=>r.present&&r.topic==='terminal.runtime.v1'&&r.key===(session?.ptyId||descriptor?.ptyId)&&r.producer.kind==='core')?.value;
        receive({session,runtime:native});
        if(!session||unresolved()||descriptor.ptyId&&!native)scheduleRecovery();
      });
      if(descriptor.preparation==='preparing')scheduleRecovery();
    },
    data:params=>sodeck.request('data',params),
    async invokeAction(id) {
      if(!descriptor?.actions.some(action=>action.id===id&&action.enabled))throw new Error('当前终端操作不可用');
      const result=await sodeck.request('event',{type:'terminal.action',actionId:id});
      const plan=result.data||result;
      if(plan.viewType)await sodeck.request('openView',{viewType:plan.viewType,params:plan.params});
    },
    dispose(){disposed=true;epoch++;clearTimeout(retryTimer);watch?.dispose();client?.dispose();listeners.clear();},
  };
}
