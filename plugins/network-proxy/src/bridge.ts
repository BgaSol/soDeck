import type {StateBridge} from '@sodeck/sdk/state';
import type {WorkerTransport} from '@sodeck/sdk/worker';
import type {DiagnosticMetadata} from '@sodeck/sdk/observation';
type Job=()=>Promise<void>;
const targetLanes=new WeakMap<StateBridge,{active:number;queue:Job[]}>();
/** A core RPC may return a pending receipt immediately. Hold this slot until
 * the complete target operation settles, across every rule in the worker. */
export function targetOperation<T>(bridge:StateBridge,work:()=>Promise<T>):Promise<T> {
  let lane=targetLanes.get(bridge);if(!lane){lane={active:0,queue:[]};targetLanes.set(bridge,lane);}
  const current=lane;
  if(current.queue.length>=256)return Promise.reject(Object.assign(Error('目标连接等待队列已满'),{code:'CAPACITY'}));
  const drain=()=>{while(current.active<12&&current.queue.length){current.active++;void current.queue.shift()!().finally(()=>{current.active--;drain();});}};
  return new Promise<T>((resolve,reject)=>{current.queue.push(async()=>{try{resolve(await work());}catch(error){reject(error);}});drain();});
}
/** Keep below the host's 32-call ceiling across all rules. State ACK and cleanup
 * have independent lanes, so a burst of targets cannot starve either one. */
export function networkBridge(transport:WorkerTransport):WorkerTransport {
  const lanes=[{limit:12,max:256,active:0,queue:[] as Job[]},{limit:8,max:256,active:0,queue:[] as Job[]},{limit:4,max:64,active:0,queue:[] as Job[]}];
  const drain=(lane:typeof lanes[number])=>{while(lane.active<lane.limit&&lane.queue.length){const job=lane.queue.shift()!;lane.active++;void job().finally(()=>{lane.active--;drain(lane);});}};
  return {
    onNotification:listener=>transport.onNotification(listener),
    call<T>(method:string,params:unknown={},metadata?:DiagnosticMetadata) {
      const cleanup=method.endsWith('.close')||method.endsWith('.release')||method==='net.operations.cancel';
      const query=method==='state.ack'||method==='net.operations.get'||method==='net.operations.claim'||method==='net.groups.get';
      const lane=lanes[cleanup?2:query?1:0];
      if(lane.queue.length>=lane.max)return Promise.reject(Object.assign(Error('网络调用等待队列已满'),{code:'CAPACITY'}));
      return new Promise<T>((resolve,reject)=>{lane.queue.push(async()=>{try{resolve(await transport.call<T>(method,params,metadata));}catch(error){reject(error);}});drain(lane);});
    },
  };
}
