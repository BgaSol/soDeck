import {describe,it,expect,vi,afterEach} from 'vitest';
import {createObservation,observedAction,DiagnosticEntities,observeHistory,callObservation,type ObservationTransport,type DiagnosticMetadata} from './observation';
afterEach(()=>vi.useRealTimers());
describe('observation producer',()=>{
  it('retries only diagnostics with identical item identity and bounded attempts',async()=>{
    vi.useFakeTimers();const sent:unknown[]=[];let attempts=0;
    const bridge:ObservationTransport={async call<T>(method:string,p?:unknown){sent.push(p);if(++attempts<3)throw {code:'RESULT_UNKNOWN'};return {storeEpoch:"store",deduplication:"retained-events",items:(p as {items:{clientEventId:string}[]}).items.map(i=>({clientEventId:i.clientEventId,status:'duplicate'}))}as T;}};
    const client=createObservation(bridge);const receipt=client.emit('plugin.example.done',{count:1});
    await vi.advanceTimersByTimeAsync(1000);expect((await receipt).status).toBe('duplicate');expect(attempts).toBe(3);expect(sent[0]).toEqual(sent[2]);expect(client.health.queuedBytes).toBe(0);
  });
  it('detaches unavailable actions and executes business exactly once',async()=>{
    const calls:string[]=[];const bridge:ObservationTransport={async call<T>(m:string){calls.push(m);if(m==='traces.begin')throw {code:'STORE_UNAVAILABLE'};return 7 as T;}};
    const client=await createObservation(bridge).beginAction('plugin.example.open');expect(await client.call('business.open')).toBe(7);expect(calls).toEqual(['traces.begin','business.open']);expect(client.metadata).toBeUndefined();
  });
  it('keeps invocation contexts immutable across concurrent business calls',async()=>{
    const contexts:string[]=[];const bridge:ObservationTransport={async call<T>(_m:string,_p?:unknown,meta?:DiagnosticMetadata){contexts.push(meta!.diagnostics.contextRef);return null as T;}};
    const client=createObservation(bridge),a=client.withInvocation({metadata:{diagnostics:{version:1,contextRef:'a'}}}),b=client.withInvocation({metadata:{diagnostics:{version:1,contextRef:'b'}}});
    await Promise.all([a.call('one'),b.call('two'),a.call('three')]);expect(contexts).toEqual(['a','b','a']);expect(Object.isFrozen(a.metadata?.diagnostics)).toBe(true);
  });
  it('caps queued bytes and private entity aliases without exposing private keys',async()=>{
    vi.useFakeTimers();const client=createObservation({call:()=>new Promise(()=>{})});
    const receipt=await client.emit('plugin.example.done',{secret:'x'.repeat(9000)});expect(receipt.status).toBe('rejected');expect(client.health.queuedBytes).toBe(0);
    let i=0;const entities=new DiagnosticEntities(()=>`ref-${++i}`);expect(entities.ref('/private/secret')).toBe('ref-1');for(let i=0;i<511;i++)entities.ref(`private-${i}`);expect(entities.ref('overflow')).toBeUndefined();entities.clear();expect(entities.ref('new')).toBe('ref-513');
  });
});
describe('history consumer',()=>{
  it('returns explicit unsupported evidence from a core without that API and performs no fallback work',async()=>{
    const calls:string[]=[];const bridge:ObservationTransport={async call(method){calls.push(method);throw {code:'METHOD_NOT_FOUND',message:'Unsupported core capability'};}};
    await expect(callObservation(bridge,'evidence.create',{filter:{},allowPartial:true})).rejects.toMatchObject({code:'METHOD_NOT_FOUND'});expect(calls).toEqual(['evidence.create']);
  });
  it('installs listener before activation and releases it if opening fails',async()=>{
    const order:string[]=[];const bridge:ObservationTransport={onNotification(){order.push('listen');return()=>{order.push('unlisten');};},async call(){order.push('open');throw {code:'STORE_UNAVAILABLE'};}};
    await expect(observeHistory(bridge,{filter:{}},()=>{}, {onGap:()=>false})).rejects.toMatchObject({code:'STORE_UNAVAILABLE'});expect(order).toEqual(['listen','open','unlisten']);
  });
});

describe('observation completion',()=>{
  it('uses the domain outcome of fulfilled results and does not call accepted work complete',async()=>{
    const outcomes:string[]=[];
    const bridge:ObservationTransport={async call<T>(method:string,p?:unknown){
      if(method==='traces.begin')return {traceId:'t',spanId:'s',parentSpanId:null,actionId:'a',contextRef:'ref'} as T;
      if(method==='traces.end'){outcomes.push((p as {outcome:string}).outcome);return {context:{traceId:'t',spanId:'s',parentSpanId:null,actionId:'a',contextRef:'ref'},outcome:(p as {outcome:string}).outcome,duplicate:false}as T;}
      throw Error(method);
    }};
    const client=createObservation(bridge);
    for(const result of [{state:'failed'},{accepted:true},{state:'canceled'},{state:'ready'}])await observedAction(client,'plugin.example.check',{},async()=>result);
    await Promise.resolve();expect(outcomes).toEqual(['failed','unknown','canceled','succeeded']);
  });
  it('bounds shutdown even when a diagnostic transport never responds',async()=>{
    vi.useFakeTimers();const client=createObservation({call:()=>new Promise(()=>{})});const receipt=client.emit('plugin.example.done');const stopped=client.stop();
    await vi.advanceTimersByTimeAsync(1001);expect(await stopped).toBe(false);expect((await receipt).status).toBe('rejected');expect(client.health.queuedBytes).toBe(0);
    expect((await client.emit('plugin.example.done')).status).toBe('rejected');
  });
});

describe('bounded diagnostic control and retries',()=>{
  it('continues a business action after a hung diagnostic begin',async()=>{
    vi.useFakeTimers();let business=0;const client=createObservation({call:<T>(method:string)=>method==='traces.begin'?new Promise<T>(()=>{}):Promise.resolve(++business as T)});
    const running=observedAction(client,'plugin.example.open',{},action=>action.call('business.open'));
    await vi.advanceTimersByTimeAsync(1001);expect(await running).toBe(1);expect(client.health.detached).toBe(1);
  });
  it('retries only capacity receipts without replaying already committed items',async()=>{
    vi.useFakeTimers();const batches:string[][]=[];
    const client=createObservation({async call<T>(_method:string,params?:unknown){const ids=(params as {items:{clientEventId:string}[]}).items.map(i=>i.clientEventId);batches.push(ids);return {storeEpoch:'e',deduplication:'retained-events',items:ids.map((id,index)=>({clientEventId:id,status:batches.length===1&&index===1?'rejected':'persisted',...(batches.length===1&&index===1?{error:{code:'CAPACITY',message:'busy'}}:{})}))}as T;}});
    const first=client.emit('plugin.example.done'),second=client.emit('plugin.example.done');await vi.advanceTimersByTimeAsync(1000);
    expect((await first).status).toBe('persisted');expect((await second).status).toBe('persisted');expect(batches[1]).toEqual([batches[0][1]]);expect(client.health.queuedBytes).toBe(0);
  });
  it('sends terminal outcomes immediately and retains only latest pending snapshot',async()=>{
    vi.useFakeTimers();const methods:string[]=[];const client=createObservation({async call<T>(method:string,params?:unknown){methods.push(method);if(method==='events.v2.appendBatch')return {storeEpoch:'e',deduplication:'retained-events',items:(params as {items:{clientEventId:string}[]}).items.map(i=>({clientEventId:i.clientEventId,status:'persisted'}))}as T;return {storedSnapshotId:'s',receivedAt:1,status:'stored'} as T;}});
    await client.emit('plugin.example.done',{}, {outcome:'failed'});expect(methods).toEqual(['events.v2.appendBatch']);
    const old=client.snapshot({snapshotType:'state',typeVersion:1,scope:'one',producerRevision:'1',observedAt:1,fields:{}});const next=client.snapshot({snapshotType:'state',typeVersion:1,scope:'one',producerRevision:'2',observedAt:2,fields:{}});
    expect(await old).toEqual({status:'coalesced'});await vi.advanceTimersByTimeAsync(251);expect(await next).toMatchObject({status:'stored'});expect(methods).toHaveLength(2);await client.stop();
  });
});

describe('logger and private alias byte bounds',()=>{
  it('uses all six levels with the declared type version',async()=>{
    vi.useFakeTimers();const sent:{level:string;typeVersion:number}[]=[];
    const client=createObservation({async call<T>(_method:string,p?:unknown){const items=(p as {items:{clientEventId:string;level:string;typeVersion:number}[]}).items;sent.push(...items);return {items:items.map(i=>({clientEventId:i.clientEventId,status:'persisted'}))}as T;}});
    const logger=client.logger('plugin.example.log',3),receipts=['trace','debug','info','warn','error','fatal'].map(level=>logger[level as keyof typeof logger]());
    await vi.advanceTimersByTimeAsync(1000);await Promise.all(receipts);expect(sent.map(i=>i.level)).toEqual(['trace','debug','info','warn','error','fatal']);expect(sent.every(i=>i.typeVersion===3)).toBe(true);await client.stop();
  });
  it('refuses huge private keys before retaining them and releases charged memory',()=>{
    const entities=new DiagnosticEntities(()=> 'ref');expect(entities.ref('x'.repeat(300000))).toBeUndefined();expect(entities.ref('x'.repeat(250000))).toBe('ref');expect(entities.ref('y'.repeat(250000))).toBeUndefined();entities.release('x'.repeat(250000));expect(entities.ref('y'.repeat(250000))).toBe('ref');
  });
});

describe('history dispatch isolation',()=>{
  it('validates only its own batch and never acknowledges after disposal',async()=>{
    const listeners=new Set<(event:{type:string;payload:unknown})=>void>(),methods:string[]=[];let sequence=0,release!:()=>void;
    const bridge:ObservationTransport={onNotification(listener){listeners.add(listener);return()=>{listeners.delete(listener);};},async call<T>(method:string){methods.push(method);if(method==='events.v2.open')return {subscriptionId:String(++sequence),feedEpoch:'epoch',highWatermark:'0',checkpoint:'cp',effectiveFilter:{}} as T;if(method==='events.v2.close')return {closed:true}as T;return {acknowledged:true,checkpoint:'cp'}as T;}};
    let gaps=0;const first=await observeHistory(bridge,{filter:{}},()=>{}, {onGap:async()=>{gaps++;await new Promise<void>(r=>{release=r;});return true;}}),second=await observeHistory(bridge,{filter:{}},()=>{}, {onGap:()=>{throw Error('unrelated feed');}});
    for(const listener of listeners)listener({type:'events.v2.gap',payload:{subscriptionId:'1',feedEpoch:'epoch',reason:7,checkpoint:'cp'}});expect(gaps).toBe(0);
    for(const listener of listeners)listener({type:'events.v2.gap',payload:{subscriptionId:'1',feedEpoch:'epoch',reason:'history-pruned',checkpoint:'cp'}});expect(gaps).toBe(1);await first.dispose();release();await Promise.resolve();await Promise.resolve();expect(methods.filter(m=>m==='events.v2.ack')).toHaveLength(2);await second.dispose();expect(listeners.size).toBe(0);
  });
});
