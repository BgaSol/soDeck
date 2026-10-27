import {schema as wire} from './state-feed.generated';
import type {StateRequest,Snapshot,OpenResult,StateUpdate,StateControl,Presentation} from './state-feed.generated';
import {validate,type Shape} from './schema';

export interface Notification {type:string;payload?:unknown}
export interface StateBridge {
  call<T=unknown>(method:string,params?:unknown):Promise<T>;
  onNotification(listener:(event:Notification)=>void):()=>void;
}
export interface StateSubscription<T> {
  readonly current:T|undefined;
  readonly revision:string|undefined;
  dispose():Promise<void>;
}
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>;'x-events':Record<string,string>};
export function validateState(kind:'x-methods'|'x-results'|'x-events',name:string,value:unknown) {
  const definition=schema[kind][name];
  return definition?validate(schema,schema.$defs[definition],value,name):undefined;
}
export function presentation(value:unknown):Presentation {
  const error=validate(schema,schema.$defs.Presentation,value,'view.presentation.v1');
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as Presentation;
}

/** Latest state only. Domain work must use its own cancellation/generation. */
export async function observeState<T>(bridge:StateBridge,request:StateRequest,decode:(value:unknown)=>T,
  listener:(value:T,snapshot:Snapshot)=>void,options:{signal?:AbortSignal;onError?:(error:unknown)=>void}={}):Promise<StateSubscription<T>> {
  let active:OpenResult|undefined,current:T|undefined,revision:string|undefined;
  let sequence=0n,disposed=false,recovering:Promise<void>|undefined,appGeneration:string|undefined;
  let cancelWait:(()=>void)|undefined;
  const report=(error:unknown)=>{try{options.onError?.(error);}catch{/* listener failures never escape the wire receiver */}};
  const notify=(value:T,snapshot:Snapshot)=>{Promise.resolve().then(()=>{if(!disposed&&revision===snapshot.revision)listener(value,snapshot);}).catch(report);};
  const call=async<R>(method:string,params:unknown):Promise<R>=>{
    const invalid=validateState('x-methods',method,params);if(invalid)throw new Error(invalid);
    const result=await bridge.call<R>(method,params);
    const invalidResult=validateState('x-results',method,result);if(invalidResult)throw new Error(invalidResult);
    return result;
  };
  const close=async(entry:OpenResult|undefined)=>{if(entry)await call('state.close',{subscriptionId:entry.subscriptionId,feedEpoch:entry.feedEpoch});};
  const install=(snapshot:Snapshot)=>{
    const target=request.target as {scope?:unknown;workspaceId?:string;viewId?:string;group?:{id:string}};
    const expectedScope=target.scope??target;
    const valid=request.contract==='app.theme.v1'||request.contract==='servers.catalog.v1'
      ? 'appGeneration' in snapshot.scope && snapshot.objectId===(request.contract==='servers.catalog.v1'?'catalog':'theme') && (!appGeneration||snapshot.scope.appGeneration===appGeneration)
      : 'workspaceId' in snapshot.scope && snapshot.scope.workspaceId===(expectedScope as {workspaceId:string}).workspaceId && snapshot.scope.workspaceGeneration===(expectedScope as {workspaceGeneration:string}).workspaceGeneration && snapshot.objectId===(target.group?.id??target.viewId??target.workspaceId);
    if(!valid)throw Object.assign(new Error('State snapshot belongs to another object'),{code:'PROTOCOL_ERROR'});
    const value=decode(snapshot.value);
    if('appGeneration' in snapshot.scope)appGeneration=snapshot.scope.appGeneration;
    current=value;revision=snapshot.revision;
    return value;
  };
  const open=async()=>{
    const entry=await call<OpenResult>('state.open',request);
    if(disposed){await close(entry);return;}
    active=entry;sequence=0n;
    const value=install(entry.snapshot);
    await call('state.ack',{subscriptionId:entry.subscriptionId,feedEpoch:entry.feedEpoch,sequence:'0'});
    if(!disposed&&active===entry&&sequence===0n)notify(value,entry.snapshot);
  };
  const recover=()=>{
    if(disposed||recovering)return;
    const old=active;active=undefined;current=undefined;revision=undefined;
    recovering=(async()=>{
      await close(old).catch(report);
      for(let attempt=0;!disposed;attempt++){
        try{await open();return;}catch(error){
          report(error);const failed=active;active=undefined;current=undefined;revision=undefined;await close(failed).catch(report);
          if(disposed)return;
          if(attempt===3||!['TIMEOUT','RESULT_UNKNOWN','CAPACITY','TRANSPORT_ERROR'].includes((error as {code?:string}).code??'')){await handle.dispose();return;}
          await new Promise<void>(resolve=>{const timer=setTimeout(()=>{cancelWait=undefined;resolve();},[100,300,1000][attempt]);cancelWait=()=>{clearTimeout(timer);cancelWait=undefined;resolve();};});
        }
      }
    })().catch(report).finally(()=>{recovering=undefined;});
  };
  const unsubscribe=bridge.onNotification(event=>{
    if(disposed||!active||!event.type.startsWith('state.'))return;
    const payload=event.payload as StateControl|StateUpdate|undefined;
    if(payload?.subscriptionId!==active.subscriptionId||payload.feedEpoch!==active.feedEpoch)return;
    const invalid=validateState('x-events',event.type,payload);
    if(invalid){report(new Error(invalid));recover();return;}
    if(event.type==='state.closed'){report(Object.assign(new Error('State object ended'),{code:'STATE_CLOSED',reason:(payload as StateControl).reason}));void handle.dispose().catch(report);return;}
    if(event.type==='state.resyncRequired'){recover();return;}
    if(event.type!=='state.update')return;
    const update=payload as StateUpdate,n=BigInt(update.sequence);
    if(n>sequence+1n){recover();return;}
    const entry=active;
    try {
      if(n<=sequence){void call('state.ack',{subscriptionId:entry.subscriptionId,feedEpoch:entry.feedEpoch,sequence:update.sequence}).catch(report);return;}
      if(revision!==undefined&&BigInt(update.snapshot.revision)<BigInt(revision)){recover();return;}
      const value=install(update.snapshot);sequence=n;
      // Confirm installation, not completion of arbitrary async business work.
      void call('state.ack',{subscriptionId:entry.subscriptionId,feedEpoch:entry.feedEpoch,sequence:update.sequence}).then(()=>{if(active===entry&&!disposed&&sequence===n)notify(value,update.snapshot);}).catch(error=>{report(error);recover();});
    }catch(error){report(error);recover();}
  });
  const abort=()=>{void handle.dispose().catch(report);};
  const handle:StateSubscription<T>={get current(){return current;},get revision(){return revision;},async dispose(){
    if(disposed)return;disposed=true;cancelWait?.();unsubscribe();options.signal?.removeEventListener('abort',abort);
    const old=active;active=undefined;current=undefined;await close(old);
  }};
  options.signal?.addEventListener('abort',abort,{once:true});
  if(options.signal?.aborted){await handle.dispose();return handle;}
  try{await open();return handle;}catch(error){await handle.dispose().catch(report);throw error;}
}
