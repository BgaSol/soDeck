import {describe,it,expect} from 'vitest';
import {callQuality,decodeQualityState,qualityObjectId,validateQuality} from './quality';
import {observeState,validateState,type StateBridge} from './state';
describe('connection quality boundary',()=>{
  it('rejects spoofed identity, impossible measurements and unsupported contracts',async()=>{
    expect(validateQuality('x-methods','quality.check.start',{serverId:'s',configEpoch:'e',operationId:'o',connectionId:'spoof'})).toBeTruthy();
    expect(validateQuality('x-methods','quality.unknown',{})).toBeTruthy();
    await expect(callQuality(async()=>({enabled:true}),'quality.policy.get',{serverId:'s',configEpoch:'e'})).rejects.toMatchObject({code:'PROTOCOL_ERROR'});
    expect(()=>decodeQualityState({configEpoch:'e',serverId:null,revision:'18446744073709551616'})).toThrow();
    expect(validateState('x-methods','state.open',{contract:'communication.quality.v1',target:{configEpoch:'e',serverId:'s'}})).toBeUndefined();
  });
  it('binds its app subscription to configuration and target before accepting a snapshot',async()=>{
    const target={serverId:'s',configEpoch:'e'},objectId=await qualityObjectId(target);
    const value={...target,revision:'1'};
    const bridge:StateBridge={async call<T>(method:string){return (method==='state.open'?{subscriptionId:'quality',feedEpoch:'feed',sequence:'0',snapshot:{scope:{appGeneration:'app'},objectId,revision:'1',value}}:{accepted:true}) as T;},onNotification:()=>()=>{}};
    const sub=await observeState(bridge,{contract:'communication.quality.v1',target},decodeQualityState,()=>{});expect(sub.current).toEqual(value);await sub.dispose();
    await expect(observeState(bridge,{contract:'communication.quality.v1',target:{...target,configEpoch:'new'}},decodeQualityState,()=>{})).rejects.toThrow('another object');
  });
});
