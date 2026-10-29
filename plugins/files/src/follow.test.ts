import {describe,it,expect,vi} from 'vitest';
const feeds=vi.hoisted(()=>new Map<string,(v:any)=>void>());
const verification=vi.hoisted(()=>({run:vi.fn(async()=>{})}));
vi.mock('@sodeck/sdk/linux-path-view',()=>({verifyPathView:verification.run}));
vi.mock('./follow-contract',()=>({directory:(v:unknown)=>v,presentation:(v:unknown)=>v,observeState:async(_b:unknown,r:any,_d:unknown,fn:(v:any)=>void)=>{
  feeds.set(r.contract,fn);return {dispose:async()=>{if(feeds.get(r.contract)===fn)feeds.delete(r.contract);}};
}}));
import {FollowController} from './follow';
import type {Explorer} from './state';
import type {StateBridge} from '@sodeck/sdk/state';
import type {ObservationClient} from './observation';
const turn=()=>new Promise(r=>setTimeout(r,0));
const path=(text:string)=>({bytesBase64:btoa(text)});
function value(revision:string,target='/tmp',source='A'){return {scope:{workspaceId:'w',workspaceGeneration:'g'},source:{id:source,generation:'g',title:source,viewId:source},selectionRevision:'1',pathRevision:revision,status:'ready',physicalPath:path(target),evidence:{pid:'1'},logicalPath:path(target),observedAt:1};}
async function fixture(connection?:{connectionId:string;generation:number}){
  feeds.clear();verification.run.mockReset();verification.run.mockResolvedValue(undefined);
  const events:{type:string;fields:any;options:any;action?:any}[]=[];
  const diagnostic={async beginAction(_type:string,action:any){return {call:async()=>connection,async emit(type:string,fields:any,options:any){events.push({type,fields,options,action});},async end(){}};},async emit(type:string,fields:any,options:any){events.push({type,fields,options});}} as unknown as ObservationClient;
  const commits:string[]=[];const remote={lease:{connectionId:'files-connection',generation:1},cancel:async()=>{},fork:()=>({lease:{connectionId:'files-connection',generation:1},cancel:async()=>{}})};
  const state={scope:{workspaceId:'w',workspaceGeneration:'g'},viewId:'f',visible:false,remote,observationContext(){return diagnostic;},changed(){},async presentation(v:boolean){this.visible=v;},async locateFollow(p:any,_r:unknown,valid:()=>boolean){if(valid())commits.push(atob(p.bytesBase64));}};
  const bridge={call:async(method:string)=>method==='capabilities.list'?{capabilities:connection?['workspace.directory.connection.v1']:[]}:connection};
  const controller=new FollowController(bridge as StateBridge);await controller.attach(state as unknown as Explorer);
  await controller.setVisible(true,1);return {controller,commits,state,events,emit:(v:any)=>feeds.get('workspace.directory.v1')?.(v)};
}
describe('follow arbitration',()=>{
  it('never probes a path from a different connection, and resumes on a matching source',async()=>{
    const connection={connectionId:'old-route',generation:1};const f=await fixture(connection);
    f.emit(value('1'));await turn();expect(f.commits).toEqual([]);expect(verification.run).not.toHaveBeenCalled();
    expect(f.controller.snapshot().message).toBe('连接不同，暂不跟随');
    connection.connectionId='files-connection';f.emit(value('2'));await turn();expect(f.commits).toEqual(['/tmp']);await f.controller.dispose();
  });
  it('does not pull back manual navigation on identical prompts; selection changes do follow',async()=>{
    const f=await fixture();f.emit(value('1'));await turn();expect(f.commits).toEqual(['/tmp']);
    f.controller.manual();f.emit({...value('1'),observedAt:2});await turn();expect(f.commits).toHaveLength(1);
    f.emit({...value('1','/etc','B'),selectionRevision:'2'});await turn();expect(f.commits).toEqual(['/tmp','/etc']);await f.controller.dispose();
  });
  it('discards delayed old results and lets a newer one-shot retain its intent',async()=>{
    const f=await fixture();let release!:()=>void;verification.run.mockImplementationOnce(()=>new Promise<void>(r=>release=r));
    f.emit(value('1'));await turn();f.controller.manual();f.emit(value('2','/etc'));f.controller.locateOnce();release();await turn();await turn();
    expect(f.commits).toEqual(['/etc']);
    const completed=f.events.filter(e=>e.type.endsWith('locate.completed'));
    expect(completed.map(e=>({revision:e.fields.directoryRevision,result:e.fields.result,outcome:e.options.outcome,source:e.action.sourceSessionRef}))).toEqual([
      {revision:'1',result:'superseded',outcome:'canceled',source:'A'},
      {revision:'2',result:'applied',outcome:'succeeded',source:'A'},
    ]);
    expect(JSON.stringify(f.events)).not.toContain('/etc');await f.controller.dispose();
  });
  it('does no hidden I/O, resumes once, honors draft input and cancel until explicit action',async()=>{
    const f=await fixture();await f.controller.setVisible(false,2);f.emit(value('1'));await turn();expect(verification.run).not.toHaveBeenCalled();
    await f.controller.setVisible(true,3);f.controller.interactionState(true);f.emit(value('2'));await turn();expect(f.commits).toEqual([]);
    f.controller.interactionState(false,true);await turn();expect(f.commits).toEqual([]);
    f.emit(value('3','/etc'));await turn();expect(f.commits).toEqual(['/etc']);
    await f.controller.cancel();f.emit(value('4'));await turn();expect(f.commits).toHaveLength(1);
    f.controller.locateOnce();await turn();expect(f.commits).toEqual(['/etc','/tmp']);expect(f.controller.paused).toBe(true);await f.controller.dispose();
  });
  it('retains follow-off across detach and never changes it for a one-shot',async()=>{
    const f=await fixture();await f.controller.setEnabled(false);f.emit(value('1'));await turn();expect(f.commits).toEqual([]);
    f.controller.locateOnce();await turn();expect(f.commits).toEqual(['/tmp']);expect(f.controller.enabled).toBe(false);
    await f.controller.detach();expect(f.controller.enabled).toBe(false);expect(feeds.size).toBe(0);
  });
});
