import {useEffect,useState} from 'react';
import {core,errorMessage,onCoreNotification} from '@sodeck/sdk';
import {validateTraffic,type Policy} from '@sodeck/sdk/traffic';

export function useCommunicationPolicy() {
  const [policy,setPolicy]=useState<Policy>(),[error,setError]=useState('');
  useEffect(()=>{
    let active=true;
    const accept=(value:unknown)=>{
      if(!active)return;
      const invalid=validateTraffic('x-results','communication.policy.get',value);
      if(invalid){setError(invalid);return;}
      const next=value as Policy;
      setPolicy(old=>old&&BigInt(old.revision)>BigInt(next.revision)?old:next);
    };
    const off=onCoreNotification(event=>{if(event.type==='communication.policy.changed')accept(event.payload);});
    void core.call('communication.policy.get',{}).then(accept).catch(e=>{if(active)setError(errorMessage(e));});
    return()=>{active=false;off();};
  },[]);
  return {policy,setPolicy,error};
}
