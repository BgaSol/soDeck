import {observationSchema} from './observation.generated';
import {validate,type Shape} from './schema';
import type {Context,Item,Receipt,Begin,End,Snapshot,Metric,FeedOpen,FeedOpened,FeedBatch,FeedGap,Event} from './observation.generated';
export type * from './observation.generated';
export interface DiagnosticMetadata {diagnostics:{version:1;contextRef:string}}
export interface Invocation {metadata?:DiagnosticMetadata}
export interface ObservationTransport {
  readonly observationCreditBytes?:number;
  call<T=unknown>(method:string,params?:unknown,metadata?:DiagnosticMetadata):Promise<T>;
  onNotification?(listener:(event:{type:string;payload?:unknown})=>void):()=>void;
}
const schema=observationSchema as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>;'x-events':Record<string,string>};
export function validateObservation(kind:'x-methods'|'x-results'|'x-events',method:string,value:unknown):string|undefined {
  const name=schema[kind][method];return name?validate(schema,schema.$defs[name],value,method):undefined;
}
const fault=(code:string)=>Object.assign(new Error(code),{code});
export async function callObservation<T>(transport:ObservationTransport,method:string,params:unknown,metadata?:DiagnosticMetadata):Promise<T>{
  if(!schema['x-methods'][method])throw fault('UNSUPPORTED_API');
  if(validateObservation('x-methods',method,params))throw fault('INVALID_PARAMS');
  const value=await transport.call<T>(method,params,metadata);
  if(validateObservation('x-results',method,value))throw fault('PROTOCOL_ERROR');return value;
}
export interface ObservationRuntime {
  id():string; now():number;
  later(callback:()=>void,ms:number):unknown; cancel(timer:unknown):void;
}
let sequence=0;
let endpointQueuedBytes=0;
const runtime:ObservationRuntime={id:()=>`obs-${Date.now().toString(36)}-${(++sequence).toString(36)}-${Math.random().toString(36).slice(2)}`,now:()=>Date.now(),later:(fn,ms)=>setTimeout(fn,ms),cancel:t=>clearTimeout(t as ReturnType<typeof setTimeout>)};
// Count UTF-8 without TextEncoder, which is absent in QuickJS.
function size(value:unknown):number{const s=JSON.stringify(value);let n=0;for(const c of s){const code=c.codePointAt(0)!;n+=code<128?1:code<2048?2:code<65536?3:4;}return n;}
type Queued={method:string;params:unknown;bytes:number;resolve:(value:unknown)=>void};
export interface ProducerHealth {dropped:number;rejected:number;filtered:number;detached:number;queuedBytes:number}
/** One queue/credit owner shared by all immutable clients in a producer. */
class Producer {
  readonly health:ProducerHealth={dropped:0,rejected:0,filtered:0,detached:0,queuedBytes:0};
  queue:Queued[]=[];timer:unknown;running=false;closed=false;
  idle=new Set<()=>void>();
  cancelRequest:(()=>void)|undefined;
  constructor(readonly bridge:ObservationTransport,readonly clock:ObservationRuntime){}
  enqueue(method:string,params:unknown,urgent=false):Promise<unknown>{
    let bytes:number;try{bytes=size(params);params=JSON.parse(JSON.stringify(params));}catch{return Promise.resolve(this.reject('INVALID_EVENT'));}
    if(this.closed||bytes>(method==='diagnostics.snapshots.report'?32768:8192)){this.health.dropped++;return Promise.resolve(this.reject('CAPACITY'));}
    // Reserve the retained object and its transport serialization together.
    bytes=bytes*2+256;
    if(method==='diagnostics.snapshots.report'){
      const next=params as Snapshot,index=this.queue.findIndex(item=>item.method===method&&(item.params as Snapshot).scope===next.scope&&(item.params as Snapshot).snapshotType===next.snapshotType);
      if(index>=0){const old=this.queue[index];if(Number((old.params as Snapshot).producerRevision)>=Number(next.producerRevision))return Promise.resolve({status:'ignored'});
        this.queue.splice(index,1);this.health.queuedBytes-=old.bytes;endpointQueuedBytes-=old.bytes;old.resolve({status:'coalesced'});}
    }
    if(endpointQueuedBytes+bytes>(this.bridge.observationCreditBytes??256*1024)){this.health.dropped++;return Promise.resolve(this.reject('CAPACITY'));}
    this.health.queuedBytes+=bytes;endpointQueuedBytes+=bytes;
    const promise=new Promise(resolve=>this.queue.push({method,params,bytes,resolve}));
    if(this.timer===undefined)this.timer=this.clock.later(()=>{this.timer=undefined;void this.pump();},250);
    if(urgent)void this.pump();
    return promise;
  }
  reject(code:string){this.health.rejected++;return {status:'rejected',error:{code,message:code}};}
  async pump():Promise<void>{
    if(this.running||!this.queue.length)return;this.running=true;
    if(this.timer!==undefined){this.clock.cancel(this.timer);this.timer=undefined;}
    try{while(this.queue.length){
      const batch=[this.queue.shift()!],method=batch[0].method;let bytes=batch[0].bytes;
      if(method!=='diagnostics.snapshots.report')while(this.queue[0]?.method===method&&batch.length<32&&bytes+this.queue[0].bytes<128*1024){const item=this.queue.shift()!;batch.push(item);bytes+=item.bytes;}
      const params=method==='events.v2.appendBatch'?{batchId:this.clock.id(),items:batch.map(i=>i.params)}:method==='metrics.report'?{items:batch.map(i=>i.params)}:batch[0].params;
      const receipts:unknown[]=new Array(batch.length);let pending=batch.map((_,index)=>index);
      for(let attempt=0;attempt<4&&pending.length;attempt++){
        let result:unknown;try{
          const request=pending.length===batch.length?params:method==='events.v2.appendBatch'?{...params as object,items:pending.map(index=>batch[index].params)}:{items:pending.map(index=>batch[index].params)};
          result=await this.request(method,request);
        }catch(error){const code=(error as {code?:string})?.code??'UNAVAILABLE';result={status:'rejected',error:{code,message:code}};}
        const returned=(result as {items?:unknown[]})?.items;const retry:number[]=[];
        pending.forEach((index,position)=>{const receipt=returned?.[position]??result;receipts[index]=receipt;
          const value=receipt as {status?:string;error?:{code?:string}};
          if(attempt<3&&!this.closed&&value.status==='rejected'&&['CAPACITY','STORE_BUSY','STORE_UNAVAILABLE','TIMEOUT','RESULT_UNKNOWN'].includes(value.error?.code??''))retry.push(index);
        });pending=retry;
        if(pending.length)await new Promise<void>(resolve=>this.clock.later(resolve,[100,300,1000][attempt]));
      }
      batch.forEach((item,index)=>{const receipt=receipts[index];const status=(receipt as {status?:string})?.status;
        if(status==='filtered')this.health.filtered++;else if(status==='rejected')this.health.rejected++;
        item.resolve(receipt);this.health.queuedBytes-=item.bytes;endpointQueuedBytes-=item.bytes;
      });
    }}finally{this.running=false;if(!this.queue.length){for(const resolve of this.idle)resolve();this.idle.clear();}}
  }
  control<T>(method:string,params:unknown,metadata?:DiagnosticMetadata):Promise<T>{
    return new Promise((resolve,reject)=>{const timer=this.clock.later(()=>reject(fault('TIMEOUT')),1000);
      void callObservation<T>(this.bridge,method,params,metadata).then(resolve,reject).finally(()=>this.clock.cancel(timer));
    });
  }
  private request(method:string,params:unknown):Promise<unknown>{
    return new Promise((resolve,reject)=>{
      let done=false;
      const finish=(error:unknown,value?:unknown)=>{if(done)return;done=true;this.clock.cancel(timer);this.cancelRequest=undefined;error?reject(error):resolve(value);};
      const timer=this.clock.later(()=>finish(fault('TIMEOUT')),2000);
      this.cancelRequest=()=>finish(fault('STOPPED'));
      void callObservation(this.bridge,method,params).then(value=>finish(undefined,value),error=>finish(error));
    });
  }
  async flush(ms=1000):Promise<boolean>{
    void this.pump();if(!this.running&&!this.queue.length)return true;
    return new Promise<boolean>(resolve=>{let timer:unknown;const done=()=>{this.idle.delete(done);this.clock.cancel(timer);resolve(true);};this.idle.add(done);timer=this.clock.later(()=>{this.idle.delete(done);resolve(false);},ms);});
  }
  async stop():Promise<boolean>{this.closed=true;const complete=await this.flush();if(this.timer!==undefined)this.clock.cancel(this.timer);this.cancelRequest?.();
    for(const item of this.queue.splice(0)){this.health.dropped++;this.health.queuedBytes-=item.bytes;endpointQueuedBytes-=item.bytes;item.resolve(this.reject('STOPPED'));}return complete;
  }
}
export class ObservationClient {
  readonly metadata:DiagnosticMetadata|undefined;
  constructor(private readonly producer:Producer,readonly context:Readonly<Context>|undefined=undefined,metadata?:DiagnosticMetadata,private readonly references:readonly import('./observation.generated').Target[]=[]){
    this.metadata=metadata?Object.freeze({diagnostics:Object.freeze({...metadata.diagnostics})}):context?Object.freeze({diagnostics:Object.freeze({version:1 as const,contextRef:context.contextRef})}):undefined;
  }
  async targets(values:{kind:'server'|'workspace'|'view'|'resource';id:string;generation?:string}[]):Promise<import('./observation.generated').Target[]>{
    const result=await Promise.all(values.slice(0,8).map(value=>this.producer.control<import('./observation.generated').Target>('observability.targets.resolve',value).catch(()=>undefined)));
    return result.filter((value):value is import('./observation.generated').Target=>value!==undefined);
  }
  withTargets(targets:import('./observation.generated').Target[]):ObservationClient{return new ObservationClient(this.producer,this.context,this.metadata,Object.freeze(targets.slice(0,8).map(value=>Object.freeze({...value}))));}
  withResourceContext(client:ObservationClient|undefined):ObservationClient{return client?this.withTargets([...client.references]):this;}
  get health():Readonly<ProducerHealth>{return {...this.producer.health};}
  withInvocation(invocation?:Invocation):ObservationClient{return new ObservationClient(this.producer,undefined,invocation?.metadata,this.references);}
  call<T=unknown>(method:string,params:unknown={}):Promise<T>{return this.producer.bridge.call<T>(method,params,this.metadata);}
  emit(type:string,fields:Record<string,unknown>={},options:Partial<Omit<Item,'type'|'fields'>>={}):Promise<Receipt>{
    const item:Item={clientEventId:this.producer.clock.id(),occurredAt:this.producer.clock.now(),typeVersion:1,...(this.references.length?{targets:[...this.references]}:{}),...(this.metadata?{contextRef:this.metadata.diagnostics.contextRef}:{}),...options,type,fields};
    return this.producer.enqueue('events.v2.appendBatch',item,['error','fatal'].includes(item.level??'')||item.outcome!==undefined).then(value=>({...value as Receipt,clientEventId:item.clientEventId}));
  }
  logger(type:string,typeVersion=1){
    const write=(level:import('./observation.generated').Level)=>(fields:Record<string,unknown>={},options:Pick<Partial<Item>,'errorCode'|'outcome'>={})=>this.emit(type,fields,{...options,level,typeVersion});
    return {trace:write('trace'),debug:write('debug'),info:write('info'),warn:write('warn'),error:write('error'),fatal:write('fatal')};
  }
  snapshot(value:Snapshot):Promise<unknown>{return this.producer.enqueue('diagnostics.snapshots.report',value);}
  metric(value:Metric):Promise<unknown>{return this.producer.enqueue('metrics.report',value);}
  async beginAction(actionType:string,fields:Record<string,unknown>={},options:Partial<Omit<Begin,'actionType'|'fields'>>={}):Promise<ObservationClient>{
    try{const context=await this.producer.control<Context>('traces.begin',{clientOperationId:this.producer.clock.id(),trigger:'plugin',...(this.references.length?{targets:[...this.references]}:{}),...options,actionType,fields},this.metadata);return new ObservationClient(this.producer,Object.freeze(context),undefined,this.references);}
    catch{this.producer.health.detached++;return new ObservationClient(this.producer,undefined,undefined,this.references);}
  }
  async stage(stageKey:string,fields:Record<string,unknown>={}):Promise<ObservationClient>{
    if(!this.metadata)return new ObservationClient(this.producer,undefined,undefined,this.references);
    try{const context=await this.producer.control<Context>('traces.stage',{contextRef:this.metadata.diagnostics.contextRef,stageKey,phase:'start',fields});return new ObservationClient(this.producer,Object.freeze(context),undefined,this.references);}
    catch{this.producer.health.detached++;return new ObservationClient(this.producer,undefined,undefined,this.references);}
  }
  async end(outcome:End['outcome'],fields:Record<string,unknown>={},errorCode?:string):Promise<void>{
    if(!this.metadata)return;
    // Queued facts must resolve their live context before its end is committed.
    await this.producer.flush(1000);
    try{await this.producer.control('traces.end',{contextRef:this.metadata.diagnostics.contextRef,outcome,fields,...(errorCode?{errorCode}:{})});}catch{this.producer.health.detached++;}
  }
  flush(ms=1000){return this.producer.flush(ms);}
  stop(){return this.producer.stop();}
  noteDropped(count=1){this.producer.health.dropped+=count;}
}
export function createObservation(bridge:ObservationTransport,clock:ObservationRuntime=runtime):ObservationClient{return new ObservationClient(new Producer(bridge,clock));}
export async function observedAction<T>(client:ObservationClient,type:string,fields:Record<string,unknown>,operation:(client:ObservationClient)=>Promise<T>):Promise<T>{
  const action=await client.beginAction(type,fields,{trigger:'user'});
  try{const result=await operation(action);const value=result&&typeof result==='object'?result as {state?:string;status?:string;accepted?:boolean}:undefined;
    const state=value?.state??value?.status;
    const outcome:End['outcome']=state==='failed'?'failed':state==='canceled'?'canceled':state==='interrupted'?'interrupted':value?.accepted===true||['waitingAuth','needs-input','needs-attention','pending','running'].includes(state??'')?'unknown':'succeeded';
    void action.end(outcome);return result;}
  catch(error){const code=(error as {code?:unknown})?.code;void action.end(code==='CANCELED'?'canceled':'failed');throw error;}
}

export const durationBuckets=[10,50,100,250,500,1000,3000,10000,30000,120000];
/** Only completed windows are submitted. No remote polling or physical byte counters. */
export class ObservationSampler {
  private snapshots=new Map<string,{type:string;scope:string;fields:Record<string,unknown>;revision:number;sent:number;dirty:boolean}>();
  private metrics=new Map<string,{sample:Metric;kind:'counter'|'gauge'|'histogram'}>();
  private timer:unknown;private closed=false;
  constructor(private readonly client:ObservationClient,private readonly clock:ObservationRuntime=runtime){}
  snapshot(type:string,scope:string,fields:Record<string,unknown>){
    if(this.closed)return;const key=JSON.stringify([type,scope]),old=this.snapshots.get(key);
    if(!old&&this.snapshots.size>=16){this.client.noteDropped();return;}
    if(old&&JSON.stringify(old.fields)===JSON.stringify(fields))return;
    this.snapshots.set(key,{type,scope,fields,revision:(old?.revision??0)+1,sent:old?.sent??0,dirty:true});this.tick();
  }
  metric(type:string,scope:string,kind:'counter'|'gauge'|'histogram',unit:string,value:number,labels:Record<string,string>={}){
    if(this.closed||!Number.isFinite(value))return;
    const from=Math.floor(this.clock.now()/10000)*10000,key=JSON.stringify([type,scope,labels,from]);let entry=this.metrics.get(key);
    if(!entry){if(this.metrics.size>=128){this.client.noteDropped();return;}entry={kind,sample:{metricType:type,typeVersion:1,scope,windowId:String(from),fromUtc:from,toUtc:from+10000,sampleId:this.clock.id(),unit,labels,...(kind==='histogram'?{buckets:durationBuckets.map(()=>0).concat(0),count:0}:{value:0})}};this.metrics.set(key,entry);}
    if(kind==='histogram'){let index=durationBuckets.findIndex(limit=>value<=limit);if(index<0)index=durationBuckets.length;entry.sample.buckets![index]++;entry.sample.count!++;}
    else entry.sample.value=kind==='counter'?entry.sample.value!+value:value;
    this.tick();
  }
  private tick(){
    if(this.closed)return;const now=this.clock.now();
    for(const entry of this.snapshots.values())if((entry.dirty&&now-entry.sent>=5000)||now-entry.sent>=30000){if(!entry.dirty)entry.revision++;entry.sent=now;entry.dirty=false;void this.client.snapshot({snapshotType:entry.type,typeVersion:1,scope:entry.scope,producerRevision:String(entry.revision),observedAt:now,fields:entry.fields});}
    for(const [key,entry]of this.metrics)if(entry.sample.toUtc<=now){this.metrics.delete(key);void this.client.metric(entry.sample);}
    if(this.timer===undefined&&(this.metrics.size||this.snapshots.size)){this.timer=this.clock.later(()=>{this.timer=undefined;this.tick();},1000);(this.timer as {unref?:()=>void}|undefined)?.unref?.();}
  }
  release(scope:string){for(const [key,entry]of this.snapshots)if(entry.scope===scope)this.snapshots.delete(key);}
  stop(){this.tick();this.closed=true;if(this.timer!==undefined)this.clock.cancel(this.timer);this.snapshots.clear();this.client.noteDropped(this.metrics.size);this.metrics.clear();}
}

/** Private paths are keys only; random aliases are the sole exported values. */
export class DiagnosticEntities {
  private readonly values=new Map<string,string>();private bytes=0;
  constructor(private readonly id:()=>string=runtime.id){}
  ref(privateKey:string):string|undefined{let ref=this.values.get(privateKey);if(ref)return ref;if(this.values.size>=512||this.bytes+privateKey.length*2+512>512*1024)return undefined;ref=this.id();this.values.set(privateKey,ref);this.bytes+=privateKey.length*2+ref.length*2+128;return ref;}
  release(privateKey:string){const value=this.values.get(privateKey);if(value)this.bytes-=privateKey.length*2+value.length*2+128;this.values.delete(privateKey);}
  clear(){this.values.clear();this.bytes=0;}
}

export async function observeHistory(bridge:ObservationTransport,request:FeedOpen,onEvents:(events:Event[])=>void|Promise<void>,options:{onGap:(gap:FeedGap)=>boolean|Promise<boolean>;onError?:(error:unknown)=>void;onClosed?:(reason:string)=>void}){
  if(!bridge.onNotification)throw fault('NOTIFICATIONS_REQUIRED');
  let opened:FeedOpened|undefined,closed=false,busy=false,checkpoint=request.checkpoint;const seen=new Set<string>();
  const unlisten=bridge.onNotification(event=>{
    if(!opened||closed||busy||!['events.v2.batch','events.v2.gap','events.v2.closed'].includes(event.type))return;
    const payload=event.payload as FeedBatch&FeedGap;
    if(!payload||typeof payload!=='object'||payload.subscriptionId!==opened.subscriptionId||payload.feedEpoch!==opened.feedEpoch)return;
    if(validateObservation('x-events',event.type,payload))return;
    if(event.type==='events.v2.closed'){closed=true;unlisten();options.onClosed?.(payload.reason);return;}
    busy=true;void(async()=>{try{
      if(event.type==='events.v2.gap'){if(!await options.onGap(payload))return;}
      else{const events=payload.items.filter(item=>!seen.has(`${item.storeEpoch}:${item.eventId}`));if(events.length)await onEvents(events);
        for(const item of events){seen.add(`${item.storeEpoch}:${item.eventId}`);if(seen.size>512)seen.delete(seen.values().next().value!);}}
      if(closed)return;
      const receipt=await callObservation<{checkpoint:string}>(bridge,'events.v2.ack',{subscriptionId:payload.subscriptionId,feedEpoch:payload.feedEpoch,...(event.type==='events.v2.gap'?{acceptGap:true}:{batchId:payload.batchId})});checkpoint=receipt.checkpoint;
    }catch(error){options.onError?.(error);}finally{busy=false;}})();
  });
  try{opened=await callObservation<FeedOpened>(bridge,'events.v2.open',request);checkpoint=opened.checkpoint;
    await callObservation(bridge,'events.v2.ack',{subscriptionId:opened.subscriptionId,feedEpoch:opened.feedEpoch});
  }catch(error){unlisten();if(opened)await callObservation(bridge,'events.v2.close',{subscriptionId:opened.subscriptionId,feedEpoch:opened.feedEpoch}).catch(()=>{});throw error;}
  return {get checkpoint(){return checkpoint;},async dispose(){if(closed)return;closed=true;unlisten();await callObservation(bridge,'events.v2.close',{subscriptionId:opened!.subscriptionId,feedEpoch:opened!.feedEpoch});}};
}
