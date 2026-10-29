import {useCallback,useEffect,useRef,useState} from 'react';
import {core,errorMessage,onCoreNotification,observeState} from '@sodeck/sdk';
import {updatesClient,type Component,type Operation,type Plan,type Policy,type Status} from '@sodeck/sdk/updates';
import type {SettingsSaveParticipant} from './save-coordinator';
const client=updatesClient(core),bridge={call:core.call,onNotification:onCoreNotification};
const uncertain=(e:unknown)=>['TIMEOUT','RESULT_UNKNOWN','TRANSPORT_ERROR'].includes((e as {code?:string}).code??'');
export function useUpdates(){
  const [status,setStatus]=useState<Status>(),[rows,setRows]=useState<Component[]>([]),[plan,setPlan]=useState<Plan>(),[selected,setSelected]=useState<string[]>([]);
  const [draft,setDraft]=useState<boolean>(),[conflict,setConflict]=useState(false),[error,setError]=useState(''),[working,setWorking]=useState(false),[operation,setOperation]=useState<Operation>();
  const [saving,setSaving]=useState<{id:string;generation:string;value:boolean}>();
  const live=useRef(true),seq=useRef(0),base=useRef<Policy|undefined>(undefined),edit=useRef<boolean|undefined>(undefined),chosen=useRef<string[]>([]),pending=useRef(saving),action=useRef(operation),busy=useRef(false);
  const refresh=useCallback(async(discard=false)=>{
    const n=++seq.current;
    try {
      const next=await client.status();if(!live.current||n!==seq.current)return;
      const dirty=base.current&&edit.current!==undefined&&edit.current!==base.current.autoCheck;
      if(discard||!dirty){base.current=next.policy;edit.current=next.policy.autoCheck;setDraft(next.policy.autoCheck);setConflict(false);}
      else if(base.current?.generation!==next.policy.generation||base.current?.revision!==next.policy.revision)setConflict(true);
      setStatus(next);
      if(next.lastSuccess){const items:Component[]=[];let cursor:string|undefined;do{const page=await client.list({limit:100,...(cursor?{cursor}:{})});items.push(...page.items);cursor=page.nextCursor??undefined;}while(cursor&&live.current&&n===seq.current);
        if(!live.current||n!==seq.current)return;setRows(items);
        chosen.current=chosen.current.filter(id=>items.some(row=>row.componentId===id&&row.availableVersion&&!['ready','waitingForCore','installed'].includes(row.stage)));setSelected(chosen.current);
        const plan=await client.plan(chosen.current);if(live.current&&n===seq.current)setPlan(plan);
      }else{setRows([]);setPlan(undefined);}
      if(action.current){const result=await client.operation(action.current.operationId);if(live.current&&n===seq.current){action.current=result;setOperation(result);}}
    } catch(e){if(live.current&&n===seq.current)setError(errorMessage(e));}
  },[]);
  useEffect(()=>{live.current=true;const abort=new AbortController();void refresh();
    void observeState(bridge,{contract:'updates.state.v1',target:{app:'current'}},v=>v,()=>void refresh(),{signal:abort.signal,onError:e=>{if(live.current)setError(errorMessage(e));}}).catch(e=>{if(live.current)setError(errorMessage(e));});
    return()=>{live.current=false;seq.current++;abort.abort();};
  },[refresh]);
  const select=(id:string,value:boolean)=>{chosen.current=value?[...new Set([...chosen.current,id])]:chosen.current.filter(v=>v!==id);setSelected(chosen.current);void refresh();};
  const run=async(kind:'check'|'download'|'install'|'restart',only?:string)=>{
    if(busy.current)return;busy.current=true;setWorking(true);setError('');const operationId=crypto.randomUUID();
    try {
      const ids=only?[only]:chosen.current;
      const p=kind==='check'?undefined:await client.plan(kind==='restart'?[]:ids);
      if(p)setPlan(p);
      if(p?.blockedReasons.length)throw Error(p.blockedReasons.join('；'));
      const result=kind==='check'?await client.check(operationId):kind==='restart'?await client.restart(p!.planRevision,operationId):await client[kind]({operationId,planRevision:p!.planRevision,componentIds:ids});
      action.current=result;setOperation(result);
    }catch(e){if(uncertain(e)){const result=await client.operation(operationId).catch(()=>undefined);if(result){action.current=result;setOperation(result);}}setError(errorMessage(e));}
    finally{busy.current=false;if(live.current){setWorking(false);void refresh();}}
  };
  const reconcile=async()=>{
    const op=pending.current;if(!op)return true;const result=await client.policyOperation(op.generation,op.id);
    if(result.state==='saved'){pending.current=undefined;setSaving(undefined);edit.current=op.value;base.current=result.result;setDraft(op.value);await refresh();return true;}return false;
  };
  const participant:SettingsSaveParticipant={id:'core:updates',dirty:!!base.current&&draft!==undefined&&draft!==base.current.autoCheck,unresolvedOperations:saving?[saving.id]:[],
    async validate(){if(conflict)throw Error('更新设置已在其他位置改变，请先载入最新值');if(!base.current||edit.current===undefined)throw Error('更新设置不可用');await client.validate({generation:base.current.generation,expectedRevision:base.current.revision,autoCheck:edit.current});},
    async commit(){const policy=base.current!;const op={id:crypto.randomUUID(),generation:policy.generation,value:edit.current!};pending.current=op;setSaving(op);
      try{base.current=await client.commit({generation:op.generation,expectedRevision:policy.revision,autoCheck:op.value,operationId:op.id});pending.current=undefined;setSaving(undefined);await refresh();}
      catch(e){if(uncertain(e)){if(await reconcile())return;throw Error('更新设置保存结果尚未确定，请核对结果');}pending.current=undefined;setSaving(undefined);throw e;}
    },reconcile,async discard(){if(pending.current)throw Error('请先核对保存结果');await refresh(true);},
  };
  return {status,rows,plan,selected,draft,conflict,error,working,operation,saving,participant,refresh,select,run,
    edit:(value:boolean)=>{edit.current=value;setDraft(value);},
    reportError:(e:unknown)=>setError(errorMessage(e)),
    cancel:async()=>{if(action.current){action.current=await client.cancel(action.current.operationId);setOperation(action.current);void refresh();}},
    follow:async(pluginId:string,digest:string)=>{try{await client.follow(pluginId,digest,crypto.randomUUID());await refresh();}catch(e){setError(errorMessage(e));}},
  };
}
export type UpdatesController=ReturnType<typeof useUpdates>;
