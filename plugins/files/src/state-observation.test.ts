import {describe,it,expect,vi} from 'vitest';
const observation=vi.hoisted(()=>({emit:vi.fn(async()=>{})}));
const collect=vi.hoisted(()=>vi.fn());
vi.mock('./observation',async()=>({...await vi.importActual<typeof import('./observation')>('./observation'),observation,sampler:undefined}));
vi.mock('./linux-collector',()=>({mountCapacity:collect,mounts:vi.fn()}));
import {Explorer} from './state';
import {root} from './remote-path';
import type {Bridge} from './remote';
import {createObservation,type ObservationTransport} from '@sodeck/sdk/observation';

function fixture(){
  observation.emit.mockClear();collect.mockReset();
  const e=new Explorer({call:async()=>({})} as Bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',()=>0);
  e.visible=true;e.remote.lease={leaseId:'l',connectionId:'c',generation:1};
  e.mountSnapshot={mounts:[],raw:new Uint8Array(),observedAt:0,pid:'1',start:'1'};
  const key=JSON.stringify(['g',1,0,root.bytesBase64]);
  return {e,key,read:async()=>{e.capacities.visible([key]);await vi.waitFor(()=>expect(e.capacities.diagnostics().active).toBe(0));},states:()=>observation.emit.mock.calls.map(c=>(c as unknown as [string,{state:string}])[1].state)};
}
describe('capacity observation uses only current completed reads',()=>{
  it('reports a disconnected refresh as failed with the reference captured while its connection was alive',async()=>{
    const events:any[]=[];let ended=false,resolutions=0;
    const transport:ObservationTransport={async call<T>(method:string,params:any){
      let value:unknown;
      if(method==='observability.targets.resolve'){if(params.kind==='resource'){resolutions++;if(ended)throw {code:'NOT_FOUND'};}value={kind:params.kind,ref:`ref-${params.kind}`,...(params.generation?{generation:params.generation}:{})};}
      else if(method==='traces.begin')throw {code:'STORE_UNAVAILABLE'};
      else if(method==='events.v2.appendBatch'){events.push(...params.items);value={storeEpoch:'e',deduplication:'retained-events',items:params.items.map((i:any)=>({clientEventId:i.clientEventId,status:'persisted'}))};}
      else if(method==='servers.get')value={revision:1};
      else if(method==='connections.acquire')value={leaseId:'l',connectionId:'c',generation:1};
      else if(method==='capabilities.list')value={capabilities:['connections.route']};
      else if(method==='connections.route.get')value={running:[]};
      else value={};return value as T;
    }};
    const client=createObservation(transport),e=new Explorer(transport as Bridge,{workspaceId:'w',workspaceGeneration:'g'},'s','v',()=>0);
    await e.connect('connect',client);expect(resolutions).toBe(1);ended=true;e.visible=true;
    await e.refresh(undefined,client);await vi.waitFor(()=>expect(events.some(e=>e.type.endsWith('directory.operation.completed'))).toBe(true));
    const completed=events.find(e=>e.type.endsWith('directory.operation.completed'));expect(completed).toMatchObject({outcome:'failed',errorCode:'CONNECTION_NOT_READY'});expect(completed.targets).toContainEqual({kind:'resource',ref:'ref-resource',generation:'1'});
    expect(resolutions).toBe(1);expect(e.state).toBe('disconnected');await e.dispose();await client.stop();
  });
  it('deduplicates unsupported state and reports recovery only after a successful read',async()=>{
    const f=fixture();collect.mockRejectedValue({code:'CAPACITY_UNSUPPORTED',message:'private command body'});
    await f.read();expect(f.states()).toEqual(['unsupported']);expect(f.e.state).toBe('waitingConnection');
    f.e.capacities.clear();await f.read();expect(f.states()).toEqual(['unsupported']);
    let resolve!:(v:unknown)=>void;collect.mockImplementation(()=>new Promise(r=>resolve=r));f.e.capacities.clear();f.e.capacities.visible([f.key]);
    expect(f.states()).toEqual(['unsupported']);resolve({total:'100',used:'25',available:'75'});
    await vi.waitFor(()=>expect(f.states()).toEqual(['unsupported','available']));
    expect(JSON.stringify(observation.emit.mock.calls)).not.toContain('private command body');await f.e.dispose();
  });
  it.each(['cancel','hide','replace'] as const)('does not report late available after %s',async kind=>{
    const f=fixture();let resolve!:(v:unknown)=>void;collect.mockImplementation(()=>new Promise(r=>resolve=r));f.e.capacities.visible([f.key]);
    if(kind==='cancel')f.e.capacities.visible([]);
    if(kind==='hide')await f.e.presentation(false,1);
    if(kind==='replace')f.e.remote.lease={leaseId:'new',connectionId:'other',generation:2};
    resolve({total:'100',used:'25',available:'75'});await vi.waitFor(()=>expect(f.e.capacities.diagnostics().active).toBe(0));
    expect(f.states()).toEqual([]);await f.e.dispose();
  });
});
