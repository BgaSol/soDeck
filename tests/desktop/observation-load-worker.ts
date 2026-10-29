import {createObservation,observeHistory} from '@sodeck/sdk/observation';
import {workerClient,type WorkerTransport} from '@sodeck/sdk/worker';
declare const __LOAD_PLUGIN__:string;
const runtime=globalThis as typeof globalThis&{sodeck:WorkerTransport;sodeckPlugin:unknown};
const timing={notifications:0,notificationMs:0,notificationMaxMs:0,calls:{} as Record<string,{count:number;ms:number;maxMs:number}>};
const measured:WorkerTransport={
  get observationCreditBytes(){return runtime.sodeck.observationCreditBytes;},
  async call<T>(method:string,params?:unknown,metadata?:Parameters<WorkerTransport['call']>[2]):Promise<T>{const began=Date.now();try{return await runtime.sodeck.call<T>(method,params,metadata);}finally{const ms=Date.now()-began,row=timing.calls[method]??={count:0,ms:0,maxMs:0};row.count++;row.ms+=ms;row.maxMs=Math.max(row.maxMs,ms);}},
  onNotification(listener){return runtime.sodeck.onNotification(event=>{const began=Date.now();try{listener(event);}finally{const ms=Date.now()-began;timing.notifications++;timing.notificationMs+=ms;timing.notificationMaxMs=Math.max(timing.notificationMaxMs,ms);}});},
};
const bridge=workerClient(measured),client=createObservation(bridge);
const subscriptions:Array<Awaited<ReturnType<typeof observeHistory>>>=[];
let result:{done:boolean;produced:number;accepted:number;rejected:number;received:number;gaps:number;histogram:number[];closeMs:number[];errors:Record<string,number>}|undefined;
let stopAt=0,starting=false;
async function start(at:number,duration:number,enabled:boolean){
  if(starting)throw Error('Load already running');starting=true;
  result={done:false,produced:0,accepted:0,rejected:0,received:0,gaps:0,histogram:Array(501).fill(0),closeMs:[],errors:{}};
  if(enabled)for(let n=0;n<8;n++){const slow=n===7;subscriptions.push(await observeHistory(bridge,{filter:{kind:'domain'},from:'now'},async events=>{
    for(const event of events){if(!event.source.pluginId?.startsWith('fixture.observation.'))continue;result!.received++;if(!slow){const bin=Math.min(500,Math.max(0,Math.ceil((Date.now()-event.committedAt)/10)));result!.histogram[bin]++;}}
    if(slow)await new Promise(r=>setTimeout(r,5000));
  },{onGap:()=>{result!.gaps++;return true;},onError:error=>{const code=(error as {code?:string}).code??'FEED_ERROR';result!.errors[code]=(result!.errors[code]??0)+1;}}));}
  stopAt=at+duration;
  await new Promise(r=>setTimeout(r,Math.max(0,at-Date.now())));
  let next=at;
  if(enabled)while(Date.now()<stopAt){
    const seq=result.produced++;void client.emit(`plugin.${__LOAD_PLUGIN__}.sample`,{sample:seq}).then(receipt=>{if(receipt.status==='persisted'||receipt.status==='duplicate')result!.accepted++;else{result!.rejected++;const code=receipt.error?.code??receipt.status;result!.errors[code]=(result!.errors[code]??0)+1;}});
    next+=80;await new Promise(r=>setTimeout(r,Math.max(0,next-Date.now())));
  }else await new Promise(r=>setTimeout(r,Math.max(0,stopAt-Date.now())));
  await client.flush(2000);await new Promise(r=>setTimeout(r,1200));
  for(const feed of subscriptions){const began=Date.now();await feed.dispose();result.closeMs.push(Date.now()-began);}
  subscriptions.length=0;result.done=true;starting=false;
}
runtime.sodeckPlugin={prepare(){},activate(){},invoke(command:string,p:Record<string,unknown>){
  if(command!=='command.execute')throw Error('Unsupported fixture command');
  if(p.commandId==='load.results')return {...result,health:client.health,timing};
  if(p.commandId==='load.start'){const args=p.args as {at:number;duration:number;enabled:boolean};void start(args.at,args.duration,args.enabled).catch(error=>{if(result){result.rejected++;result.errors[(error as {code?:string}).code??"FIXTURE_ERROR"]=(result.errors[(error as {code?:string}).code??"FIXTURE_ERROR"]??0)+1;result.done=true;}starting=false;});return {accepted:true};}
  throw Error('Unsupported fixture command');
},async stop(){stopAt=0;await client.stop();}};
