import {describe,it,expect,vi} from 'vitest';
import type {StateBridge} from '@sodeck/sdk/state';
import type {NetworkStreams} from '@sodeck/sdk/network-stream';
import {utf8} from '@sodeck/sdk/bytes';
import {Rules,type Rule} from './rules';
import {Manager} from './manager';
const mock=vi.hoisted(()=>({runs:[] as Array<{finish:()=>void;stopped:boolean}>,manual:false}));
vi.mock('./run',()=>({Run:class {
  status='starting';stopped=false;finish=()=>{};
  constructor(readonly rule:Rule){mock.runs.push(this);}
  async start(){if(mock.manual)await new Promise<void>(r=>{this.finish=r;});if(this.stopped)throw Object.assign(Error('canceled'),{code:'CANCELED'});this.status='ready';}
  async stop(){this.stopped=true;this.status='stopped';}
  snapshot(){return {rule:this.rule,status:this.status};}
}}));
async function setup(count=1){
  mock.manual=false;mock.runs=[];
  const rules=new Rules({call:async<T>()=>null as T});await rules.load();
  for(let i=0;i<count;i++)await rules.save('server',{name:'长名称'.repeat(40),type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false},0);
  return new Manager(rules,{workspaceId:'w',workspaceGeneration:'g'},'server','v',{call:async()=>({ok:true})} as unknown as StateBridge,{} as NetworkStreams);
}
describe('run ownership and bounded UI pages',()=>{
  it('starts a fresh run after stop without reusing or losing a pending start',async()=>{
    const m=await setup(),id=m.rules.all('server')[0].id;mock.manual=true;
    const first=m.start(id,'first');const rejected=expect(first).rejects.toMatchObject({code:'CANCELED'});
    await m.stop(id);const second=m.start(id,'second');await vi.waitFor(()=>expect(mock.runs.length).toBe(2));
    mock.runs[0].finish();await rejected;
    const duplicate=m.start(id,'duplicate');mock.runs[1].finish();await second;await duplicate;
    expect(mock.runs).toHaveLength(2);expect(m.runs.get(id)?.status).toBe('ready');await m.dispose();
  });
  it('paginates large running snapshots without skipping rules or exceeding 32 KiB',async()=>{
    const m=await setup(45);for(const rule of m.rules.all('server'))m.runs.set(rule.id,{snapshot:()=>({rule,error:{message:'故障'.repeat(256)}})} as never);
    const ids:string[]=[];let offset=0;
    do{const page=m.snapshot(offset,m.rules.all('server')[0].id);expect(utf8(JSON.stringify(page)).length).toBeLessThanOrEqual(32768);expect(page.items.length).toBeGreaterThan(0);ids.push(...page.items.map(i=>i.rule.id));offset=page.nextStart;}while(offset<45);
    expect(ids).toEqual(m.rules.all('server').map(r=>r.id));
  });
});
