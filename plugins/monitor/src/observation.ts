import {createObservation, ObservationSampler, type ObservationTransport, type ObservationClient, type Invocation} from '@sodeck/sdk/observation';
export function monitorObservation(bridge: ObservationTransport) {
  const client=createObservation(bridge), sampler=new ObservationSampler(client);
  const states=new Map<string,string>();
  const owners=new Map<string,ObservationClient>();
  let revision=0;
  return {
    async bind(id:string,serverId:string,workspaceId:string){owners.set(id,client);const targets=await client.targets([{kind:'server',id:serverId},{kind:'workspace',id:workspaceId},{kind:'view',id}]);if(owners.has(id))owners.set(id,client.withTargets(targets));},
    action(action:string,id?:string,invocation?:Invocation) {void (owners.get(id??'')??client).withInvocation(invocation).emit('plugin.sodeck.monitor.action',{action});},
    collection(lane:string,durationMs:number,failed:boolean){sampler.metric('monitor.collection.duration','monitor','histogram','ms',durationMs,{lane});if(failed)sampler.metric('monitor.collection.failed','monitor','counter','count',1,{lane});},
    state(id:string,state:string,activeJobs:number,cacheBytes:number,cleanupPending:boolean) {
      if(states.get(id)!==state){states.set(id,state);void (owners.get(id)??client).emit('plugin.sodeck.monitor.state.changed',{state});}
      void client.snapshot({snapshotType:'monitor.runtime.v1',typeVersion:1,scope:id,producerRevision:String(++revision),observedAt:Date.now(),fields:{state,activeJobs,cacheBytes,cleanupPending}});
      sampler.metric('monitor.cache.bytes',id,'gauge','bytes',cacheBytes);
    },
    remove(id:string){states.delete(id);owners.delete(id);},
    async stop(){sampler.stop();await client.stop();},
  };
}
