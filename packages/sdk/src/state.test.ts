import {describe,it,expect,vi} from 'vitest';
import {observeState,validateState,type Notification,type StateBridge} from './state';
const scope={workspaceId:'w',workspaceGeneration:'g'};
const tick=()=>new Promise(resolve=>setTimeout(resolve,0));
function fixture(){
  const listeners=new Set<(n:Notification)=>void>();let opens=0;
  const calls:{method:string;params:any}[]=[];
  const snapshot=(revision:string,value:number)=>({scope,objectId:'w',revision,value});
  const bridge:StateBridge={async call<T>(method:string,params:any){
    calls.push({method,params});
    if(method==='state.open')return {subscriptionId:'s'+(++opens),feedEpoch:'epoch',sequence:'0',snapshot:snapshot('1',1)} as T;
    return {accepted:true} as T;
  },onNotification(listener){listeners.add(listener);return()=>{listeners.delete(listener);};}};
  return {bridge,calls,listeners,snapshot,emit:(type:string,payload:unknown)=>{for(const listener of listeners)listener({type,payload});}};
}
describe('latest state feed',()=>{
  it('pins app generation across updates and recovery without accepting another app object',async()=>{
    const f=fixture(),errors:unknown[]=[],original=f.bridge.call;
    f.bridge.call=async<T>(m:string,p:unknown)=>m==='state.open'?{subscriptionId:'app',feedEpoch:'epoch',sequence:'0',snapshot:{scope:{appGeneration:'generation-1'},objectId:'theme',revision:'1',value:1}} as T:original<T>(m,p);
    const sub=await observeState(f.bridge,{contract:'app.theme.v1',target:{app:'current'}},Number,()=>{},{onError:e=>errors.push(e)});
    f.emit('state.update',{subscriptionId:'app',feedEpoch:'epoch',sequence:'1',snapshot:{scope:{appGeneration:'generation-2'},objectId:'theme',revision:'2',value:99}});
    await tick();expect(sub.current).toBe(1);expect(errors.length).toBe(1);await sub.dispose();
  });
  it('installs before ack, rejects old epochs, deduplicates and disposes',async()=>{
    const f=fixture(),values:number[]=[];
    const sub=await observeState(f.bridge,{contract:'workspace.directory.v1',target:scope},v=>Number(v),v=>{values.push(v);});
    await tick();expect(values).toEqual([1]);
    const payload={subscriptionId:'s1',feedEpoch:'epoch',sequence:'1',snapshot:f.snapshot('2',2)};
    f.emit('state.update',{...payload,feedEpoch:'old'});expect(sub.current).toBe(1);
    f.emit('state.update',payload);expect(sub.current).toBe(2);
    f.emit('state.update',payload);await tick();expect(values).toEqual([1,2]);
    await sub.dispose();await sub.dispose();expect(f.listeners.size).toBe(0);
    expect(f.calls.filter(c=>c.method==='state.close')).toHaveLength(1);
  });
  it('resyncs a sequence gap without applying stale payloads',async()=>{
    const f=fixture();const sub=await observeState(f.bridge,{contract:'workspace.directory.v1',target:scope},Number,()=>{});
    f.emit('state.update',{subscriptionId:'s1',feedEpoch:'epoch',sequence:'9',snapshot:f.snapshot('9',9)});
    await tick();expect(sub.current).toBe(1);
    expect(f.calls.filter(c=>c.method==='state.open')).toHaveLength(2);
    f.emit('state.update',{subscriptionId:'s1',feedEpoch:'epoch',sequence:'1',snapshot:f.snapshot('2',99)});
    expect(sub.current).toBe(1);await sub.dispose();
  });
  it('isolates listener errors and aborts while open is pending',async()=>{
    const f=fixture(),errors:unknown[]=[];
    const sub=await observeState(f.bridge,{contract:'workspace.directory.v1',target:scope},Number,()=>{throw Error('listener');},{onError:e=>errors.push(e)});
    await tick();expect(errors).toHaveLength(1);await sub.dispose();
    const controller=new AbortController();let resolve!: (value:any)=>void;
    const delayed:StateBridge={...f.bridge,call:<T>(method:string,p:unknown)=>method==='state.open'?new Promise<T>(r=>{resolve=r;}):f.bridge.call<T>(method,p)};
    const pending=observeState(delayed,{contract:'workspace.directory.v1',target:scope},Number,()=>{throw Error('must not deliver');},{signal:controller.signal});
    controller.abort();resolve({subscriptionId:'late',feedEpoch:'epoch',sequence:'0',snapshot:f.snapshot('1',1)});
    await pending;expect(f.listeners.size).toBe(0);expect(f.calls.some(c=>c.method==='state.close'&&c.params.subscriptionId==='late')).toBe(true);
  });
  it('rejects arbitrary topics and extra identity fields',()=>{
    expect(validateState('x-methods','state.open',{contract:'plugin.other.any',target:scope})).toBeTruthy();
    expect(validateState('x-methods','state.open',{contract:'workspace.directory.v1',target:scope,pluginId:'other'})).toBeTruthy();
  });
  it('recovers a transient open failure and cancels recovery waits on dispose',async()=>{
    vi.useFakeTimers();
    try{
      const f=fixture(),original=f.bridge.call;let opens=0;
      f.bridge.call=async<T>(method:string,p:unknown)=>{if(method==='state.open'&&++opens===2)throw Object.assign(Error('transient'),{code:'TIMEOUT'});return original<T>(method,p);};
      const sub=await observeState(f.bridge,{contract:'workspace.directory.v1',target:scope},Number,()=>{});
      f.emit('state.resyncRequired',{subscriptionId:'s1',feedEpoch:'epoch',reason:'gap'});
      await vi.advanceTimersByTimeAsync(100);expect(opens).toBe(3);expect(sub.current).toBe(1);
      f.bridge.call=async<T>(method:string,p:unknown)=>{if(method==='state.open'){opens++;throw Object.assign(Error('capacity'),{code:'CAPACITY'});}return original<T>(method,p);};
      f.emit('state.resyncRequired',{subscriptionId:'s2',feedEpoch:'epoch',reason:'gap'});
      await vi.advanceTimersByTimeAsync(0);const before=opens;await sub.dispose();await vi.advanceTimersByTimeAsync(5000);
      expect(opens).toBe(before);expect(f.listeners.size).toBe(0);
    }finally{vi.useRealTimers();}
  });
});
