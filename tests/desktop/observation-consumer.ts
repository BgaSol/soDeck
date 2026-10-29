import {callObservation,type ObservationTransport,type Event,type Page,type DescriptorItem,type diagnostics_snapshots_queryResult,type metrics_queryResult,type JobResult} from '@sodeck/sdk/observation';
// This consumer has no plugin-private commands and no database access.
export async function readEvidence(transport:ObservationTransport,filter:Record<string,unknown>){
  const events:Event[]=[];let cursor:string|undefined;
  do{const page=await callObservation<Page>(transport,'events.v2.query',{filter,limit:100,...(cursor?{cursor}:{})});events.push(...page.items);cursor=page.nextCursor??undefined;}while(cursor);
  const types:unknown[]=[];cursor=undefined;
  do{const page=await callObservation<{items:DescriptorItem[];nextCursor:string|null}>(transport,'events.types.list',{limit:100,...(cursor?{cursor}:{})});types.push(...page.items);cursor=page.nextCursor??undefined;}while(cursor);
  const snapshots=await callObservation<diagnostics_snapshots_queryResult>(transport,'diagnostics.snapshots.query',{}),metrics=await callObservation<metrics_queryResult>(transport,'metrics.query',{});
  const created=await callObservation<{jobId:string}>(transport,'evidence.create',{filter,allowPartial:true});
  return {events,types,snapshots,metrics,jobId:created.jobId};
}
export async function readPackage(transport:ObservationTransport,jobId:string){
  let content='',offset=0;
  for(;;){const page=await callObservation<JobResult>(transport,'evidence.get',{jobId,offset});content+=page.chunks.join('');if(page.nextOffset===null)return {job:page.job,content};offset=page.nextOffset;}
}
