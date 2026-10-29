import {describe,it,expect,vi} from 'vitest';
import type {ObservationClient} from './observation';
import type {StateBridge} from '@sodeck/sdk/state';
import type {NetworkStreams} from '@sodeck/sdk/network-stream';
import type {Rule} from './rules';
const recorded=vi.hoisted(()=>({events:[] as {type:string;fields:any;options:any;action?:any}[]}));
vi.mock('./observation',async()=>{
  const client=(action?:any):any=>({withResourceContext(){return this;},async beginAction(_type:string,fields:any){return client(fields);},async emit(type:string,fields:any,options:any){recorded.events.push({type,fields,options,action});},async end(){}});
  return {...await vi.importActual<typeof import('./observation')>('./observation'),observation:client(),sampler:undefined,reportRun:()=>{}};
});
import {observation} from './observation';
import {Run} from './run';
const rule:Rule={id:'rule',revision:1,name:'secret rule name',serverId:'s',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false};
function run(revision=1){return new Run({...rule,revision},{workspaceId:'w',workspaceGeneration:'g'},'v',{call:async()=>({})} as unknown as StateBridge,{} as NetworkStreams);}
describe('real Run observation lifecycle with controlled transport results',()=>{
  it('keeps each pool attempt reference while target-service errors remain separate',async()=>{
    const r=run(),a={bootId:'b',attemptId:'one',attemptGeneration:1},b={...a,attemptId:'two',attemptGeneration:2};
    r.log('route',{code:'PROXY_DISCONNECTED',details:{diagnosticRef:a}});r.log('route',{code:'AUTH_FAILED',details:{diagnosticRef:b}});r.log('target',{code:'TARGET_REFUSED'});
    expect(r.diagnostics.map(d=>d.diagnosticRef)).toEqual([a,b,undefined]);await r.stop();
  });
  it('keeps a late failed check on its original run and rule revision after replacement',async()=>{
    recorded.events=[];const old=run();old.status='ready';let reject!:(e:unknown)=>void;
    vi.spyOn(old.net,'execute').mockImplementation(async method=>{if(method==='net.listeners.connectSelf')return new Promise((_r,j)=>reject=j);return {} as never;});
    const pending=old.test({host:'SECRET_TARGET',port:80},'http',observation as ObservationClient);
    await vi.waitFor(()=>expect(reject).toBeTypeOf('function'));await old.stop();const current=run(2);current.status='ready';
    reject({code:'CANCELED',message:'SECRET_HTTP_HEADER'});const result=await pending;
    expect(result).toMatchObject({runId:old.id,revision:1,state:'failed',code:'CANCELED'});expect(old.check).toBeUndefined();expect(current.check).toBeUndefined();expect(current.status).toBe('ready');
    const completed=recorded.events.filter(e=>e.type.endsWith('check.completed'));expect(completed).toHaveLength(1);
    expect(completed[0]).toMatchObject({fields:{runId:old.id,ruleId:'rule'},options:{outcome:'failed'},action:{runId:old.id,ruleRevision:1}});
    expect(JSON.stringify(recorded.events)).not.toMatch(/SECRET_|secret rule name/);await current.stop();
  });
  it('reports each error once across catch layers while keeping independent cleanup failures and bounded display history',async()=>{
    recorded.events=[];const r=run();
    for(let i=0;i<100;i++){const error={code:'BAD_REQUEST',message:`SECRET_BODY_${i}`};r.log('target',error,{host:'SECRET_HOST',port:80});r.log('client',error);r.log('accept',error);}
    r.log('cleanup',{code:'IO_ERROR',message:'SECRET_CLEANUP'});
    expect(recorded.events.filter(e=>e.type.endsWith('proxy.target.failed'))).toHaveLength(100);
    expect(recorded.events.filter(e=>e.type.endsWith('run.cleanup.failed'))).toHaveLength(1);expect(recorded.events).toHaveLength(101);expect(r.diagnostics).toHaveLength(200);
    expect(JSON.stringify(recorded.events)).not.toContain('SECRET');await r.stop();
  });
});
