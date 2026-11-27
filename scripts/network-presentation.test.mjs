import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const context=vm.createContext({});vm.runInContext(fs.readFileSync('plugins/network-proxy/ui/presentation.js','utf8'),context);
const P=context.NetworkPresentation;
const rule={id:'rule',revision:1,name:'<untrusted>',type:'tcpForward',bind:{host:'127.0.0.1',port:0},target:{host:'new.example',port:80}};
const ui=row=>({catalog:{rules:[row],total:1,offset:0,checks:{}},selected:'rule',draft:null,busy:new Set(),targets:new Map(),copied:new Map(),feedback:new Map()});
test('state and action eligibility follow cleanup facts, not label or optimistic ACK',()=>{
  for(const [snapshot,extra,wanted,start]of [
    [{state:'starting'},{},'starting',false],
    [{state:'running'},{},'running',false],
    [{state:'running'},{stopRequested:true},'stopping',false],
    [{state:'closed',cleanupComplete:true},{stopRequested:true,canStart:false},'stopping',false],
    [{state:'running'},{queryError:'lost'},'unknown',false],
    [{state:'closed',cleanupComplete:true},{stopRequested:true,canStart:true},'closed',true],
  ]){const row={rule,run:{snapshot,...extra}};assert.equal(P.state(row),wanted);assert.equal(P.canStart(row),start);}
  assert(P.canStart({rule}));
});
test('inline rows use native sibling controls, escaped content and the fixed run target',()=>{
  const row={rule,name:rule.name,run:{rule:{...rule,target:{host:'old.example',port:3306}},snapshot:{state:'running',address:{host:'::1',port:13306}}}};
  const html=P.manager(ui(row));
  assert.match(html,/old\.example:3306/);assert(!html.includes('new.example:80'));
  assert.match(html,/\[::1\]:13306/);assert(!html.includes('<untrusted>'));assert.match(html,/&lt;untrusted&gt;/);
  assert.match(html,/data-dom-key="run:rule"/);assert(!html.includes('role="button"'));
  assert.match(html,/data-action="stop"/);assert.match(html,/data-action="edit"/);assert.match(html,/data-action="delete"[^>]*disabled/);
  assert.equal(P.address({host:'2001:db8::1',port:80}),'[2001:db8::1]:80');
});
test('unconfirmed state retains last entry but does not expose a fresh copy or check action',()=>{
  const html=P.manager(ui({rule,name:'rule',run:{queryError:'query failed',snapshot:{state:'running',address:{host:'127.0.0.1',port:1080},sent:100}}}));
  assert.match(html,/状态待确认/);assert.match(html,/已过期/);assert.match(html,/data-action="copy"[^>]*disabled/);assert.match(html,/data-action="check"[^>]*disabled/);
});
test('committed save with lost reply and failed reconciliation stays frozen until observed without replay',async()=>{
  const events=new Map(),globalEvents=new Map(),flushes=[];let timer,painted,failSnapshot=false,backendRevision='1',saveCount=0,draftCount=0;
  const original={id:'r',serverId:'s',revision:1,name:'initial',type:'socks5',bind:{host:'127.0.0.1',port:0}};
  let backendDraft=structuredClone(original);
  const form={reportValidity:()=>true,values:{name:'initial',bindHost:'127.0.0.1',auto:'on'}};
  const root={contains:()=>false,querySelector:s=>s==='form'?form:null,addEventListener:(key,handler)=>events.set(key,handler)};
  const error={hidden:true};
  const snapshot=()=>({catalog:{rules:[{rule:original,name:'initial'}]},draft:backendDraft,draftRevision:backendRevision,navigation:{selected:'r'},navigationRevision:'1',dirty:!!backendDraft,writePending:false});
  const c={
    document:{querySelector:s=>s==='#app'?root:error,activeElement:{},documentElement:{style:{setProperty(){}}}},
    SoDeckMotion:{observeMotion(){}},SoDeckDOM:{DomRegion:class{update(){}dispose(){}}},
    NetworkPresentation:{ready:()=>false,manager:ui=>{painted={draft:structuredClone(ui.draft),saving:ui.saving,saveUncertain:ui.saveUncertain};return ''; }},
    FormData:class{constructor(f){this.v=f.values;}get(key){return this.v[key];}has(key){return Object.hasOwn(this.v,key);}},
    sodeck:{prepared(){},flushed:(...args)=>flushes.push(args),request:async(_,data)=>{
      if(data.action==='draft'){draftCount++;assert(backendDraft);assert.equal(data.draftRevision,backendRevision);backendDraft=data.rule;backendRevision=String(+backendRevision+1);return snapshot();}
      if(data.action==='save'){saveCount++;backendDraft=null;backendRevision=String(+backendRevision+1);failSnapshot=true;throw new Error('save reply lost');}
      if(data.action==='snapshot'){if(failSnapshot){failSnapshot=false;throw new Error('lookup reply lost');}return snapshot();}
      throw new Error('unexpected action '+data.action);
    }},
    addEventListener:(key,handler)=>globalEvents.set(key,handler),setInterval:fn=>(timer=fn,1),clearInterval(){},
    CSS:{escape:s=>s},BigInt,structuredClone,Date,clearTimeout,setTimeout,
  };
  vm.runInNewContext(fs.readFileSync('plugins/network-proxy/ui/view.js','utf8'),c);
  const drain=()=>new Promise(resolve=>setImmediate(resolve));
  globalEvents.get('sodeck:mount')({detail:{data:snapshot(),context:{serverId:'s'},visible:true,readOnly:false}});
  events.get('submit')({preventDefault(){}});await drain();await drain();
  assert(painted.saving&&painted.saveUncertain);assert.equal(backendDraft,null);
  form.values.name='continued edit';events.get('input')({target:{closest:()=>form}});await drain();assert.equal(draftCount,1);
  await timer();assert.equal(painted.draft,null);assert(!painted.saving&&!painted.saveUncertain);
  assert.equal(saveCount,1);assert.equal(draftCount,1);assert(error.hidden);
  globalEvents.get('sodeck:flush')({detail:{id:'after-recovery'}});await drain();
  assert.deepEqual(flushes,[['after-recovery']],'Recovered save does not leave a rejected queue blocking close or transfer');
});
