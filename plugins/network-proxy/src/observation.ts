import {createObservation,ObservationSampler,type ObservationClient,type ObservationTransport} from '@sodeck/sdk/observation';
export let observation:ObservationClient|undefined;
export let sampler:ObservationSampler|undefined;
export function configureObservation(bridge:ObservationTransport){observation=createObservation(bridge);sampler=new ObservationSampler(observation);}
export function errorCode(error:unknown):string{const code=(error as {code?:unknown})?.code;return typeof code==='string'&&/^[A-Z][A-Z0-9_]{0,63}$/.test(code)?code:'UNKNOWN';}
export type RunDetails={ruleId:string;ruleRevision:number;portMode:'automatic'|'fixed';localPort?:number;routeRevision?:string;activeChannelCount:number;retryIndex:number;checkState?:string;recentErrorCode?:string;resourceRefs:string[]};
const runs=new Map<string,{view:string;state:string;kind:string;details?:RunDetails}>();
export function reportRun(id:string,view:string,state:string,kind:string,details?:RunDetails){
  if(!observation)return;if(!runs.has(id)&&runs.size>=128)return;runs.set(id,{view,state,kind,details});
  publishRuns(view);
}
function publishRuns(view:string){
  const rows=[...runs].filter(([,r])=>r.view===view);const stateCounts:Record<string,number>={};for(const [,r]of rows)stateCounts[r.state]=(stateCounts[r.state]??0)+1;sampler?.snapshot('network.runs.v1',view,{total:rows.length,truncated:rows.length>16,stateCounts,runs:rows.slice(0,16).map(([runId,r])=>({runId,state:r.state,kind:r.kind,...r.details}))});
  sampler?.metric('client.active',view,'gauge','count',rows.reduce((n,[,r])=>n+(r.details?.activeChannelCount??0),0));
  for(const status of ['starting','ready','reconnecting','needs-attention','stopping','stopped','failed'])sampler?.metric('run.active',view,'gauge','count',rows.filter(([,r])=>r.state===status).length,{state:status});
}
export function releaseRuns(view:string){for(const [key,r]of runs)if(r.view===view)runs.delete(key);sampler?.release(view);}
export type {ObservationClient};

export function releaseRun(id:string){const old=runs.get(id);runs.delete(id);if(old)publishRuns(old.view);}
