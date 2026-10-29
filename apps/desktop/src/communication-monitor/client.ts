import {useEffect,useState} from 'react';
import {core,decodeTrafficSummary,errorMessage,observeState,onCoreNotification,type CommunicationSummary} from '@sodeck/sdk';
import type {SummaryRequest} from '@sodeck/sdk/traffic';

export const trafficState=(state:CommunicationSummary['state'])=>({collecting:'正在采集',active:'已连接',disconnected:'已断开',unavailable:'统计不可用'}[state]);
/** Domain identity changes create a new feed; late snapshots never replace the selection. */
export function useTrafficSummary(request:SummaryRequest|undefined,serverId?:string) {
  const [value,setValue]=useState<CommunicationSummary>(),[error,setError]=useState(''),[attempt,retry]=useState(0);
  const key=JSON.stringify(request??{});
  useEffect(()=>{
    const controller=new AbortController();setValue(undefined);setError('');
    void (async()=>{
      let target:SummaryRequest=JSON.parse(key);
      if(serverId){const initial=await core.call<CommunicationSummary>('communication.summary',{});target={target:{kind:'server',configEpoch:initial.configEpoch,serverId},layer:'channel'};}
      if(controller.signal.aborted)return;
      await observeState({call:core.call,onNotification:onCoreNotification},{contract:'communication.summary.v1',target},decodeTrafficSummary,
        next=>{if(!controller.signal.aborted){setValue(next);setError('');}},
        {signal:controller.signal,onError:e=>{if(!controller.signal.aborted)setError(errorMessage(e));}});
    })().catch(e=>{if(!controller.signal.aborted)setError(errorMessage(e));});
    return()=>controller.abort();
  },[key,serverId,attempt]);
  return {value,error,retry:()=>retry(v=>v+1)};
}
