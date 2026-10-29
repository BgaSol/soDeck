import {test} from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import vm from 'node:vm';
const bundle=await build({entryPoints:[new URL('../src/worker.ts',import.meta.url).pathname.replace(/^\/(\w:)/,'$1')],bundle:true,write:false,format:'iife',platform:'neutral',define:{__MONITOR_VERSION__:'"0.1.0"'}});
const scope={workspaceId:'w',workspaceGeneration:'g'};
function fixture(){
  const records=[],timers=new Map(),leases=new Set(),execs=new Map();let now=0,sequence=0,samples=0,failCleanup=false;
  const frame=(token,name,body)=>`SODECK_MON_V1 ${token} ${name} BEGIN\n${body}\n\nSODECK_MON_V1 ${token} ${name} END 0\n`;
  const bridge={clock:{monotonicMs:()=>now},onNotification(){return()=>{};},async call(method,p){
    records.push({method,p,time:now});
    switch(method){
      case 'state.open':return {subscriptionId:'feed-'+(++sequence),feedEpoch:'epoch',sequence:'0',snapshot:{scope:p.target.scope,objectId:p.target.viewId,revision:'1',value:{viewId:p.target.viewId,scope:p.target.scope,visible:false,focused:false,revision:0}}};
      case 'state.ack':case 'state.close':return {accepted:true};
      case 'servers.get':return {revision:1,name:'Fixture'};
      case 'connections.observe':return {state:'active',revision:1,connection:{connectionId:'c',generation:1}};
      case 'connections.acquire':case 'connections.acquireExisting':{const leaseId='lease-'+(++sequence);leases.add(leaseId);return {leaseId,connectionId:'c',generation:1};}
      case 'connections.release':leases.delete(p.leaseId);return {released:true};
      case 'views.bindResources':return {bound:true,cursor:null};
      case 'connections.route.get':{const saved={targetServerId:'s',routeRevision:'1',stepCount:1,sshCount:1,proxyCount:0,attemptTimeoutMs:120000,reference:{kind:'saved',serverId:'s',routeRevision:'1'},targetName:'Fixture'};return {saved,running:[{connectionId:'c',generation:1,state:'active',route:{...saved,reference:{kind:'running',serverId:'s',connectionId:'c',generation:1,routeRevision:'1'}},configurationChanged:false,resources:{sessions:1,bridges:0},failureStep:null}],attempt:null,runningCount:1,currentCount:1,changedCount:0};}
      case 'remote.operations.forget':return {forgotten:true};
      case 'operations.cancel':case 'exec.cancel':return {canceled:true};
      case 'exec.close':if(failCleanup)throw Error('Transient close failure');execs.delete(p.execId);return {closed:true};
      case 'exec.start':{
        const script=Buffer.from(p.stdinBase64,'base64').toString(),token=/TOKEN='([^']+)'/.exec(script)[1];let text='';
        if(script.includes('begin os')){for(const [name,body]of Object.entries({os:'Linux',host:'fixture',kernel:'6.1',boot:'boot',online:'0-1',mounts:'1 0 8:0 / / rw - ext4 /dev/sda rw',interfaces:'eth0 2 1',devices:'sda 8:0 0'}))text+=frame(token,name,body);}
        else if(script.includes('begin capacity')){for(const [name,body]of Object.entries({before:'1 0 8:0 / / rw - ext4 /dev/sda rw',capacity:'STAT 4096 1000 400 300',after:'1 0 8:0 / / rw - ext4 /dev/sda rw'}))text+=frame(token,name,body);}
        else{samples++;for(const [name,body]of Object.entries({before:`${now/1000+100} 0`,cpu:`cpu ${100+samples*20} 0 0 ${100+samples*80} 0 0 0 0`,memory:'MemTotal: 1000 kB\nMemFree: 300 kB\nMemAvailable: 500 kB',load:'0.1 0.2 0.3 1/20 1',net:`eth0: ${samples*2000} 0 0 0 0 0 0 0 ${samples*4000} 0 0 0 0 0 0 0`,disk:`8 0 sda 1 0 ${samples*10} 0 1 0 ${samples*20} 0 0 0 0`,after:`${now/1000+100} 0`}))text+=frame(token,name,body);}
        const id='exec-'+(++sequence);execs.set(id,Buffer.from(text));return {execId:id};
      }
      case 'exec.read':{const data=execs.get(p.execId);return {frames:[{sequence:1,stream:'stdout',bytesBase64:data.toString('base64'),length:data.length}],lastSequence:1,acknowledged:0,state:'completed',exitCode:0,reason:null,truncated:false};}
      case 'exec.ack':return {acknowledged:p.sequence};
      default:throw Object.assign(Error(method),{code:'UNSUPPORTED_API'});
    }
  }};
  const context={sodeck:bridge,console,setTimeout(fn,ms){const id=++sequence;timers.set(id,{fn,at:now+ms});return id;},clearTimeout(id){timers.delete(id);}};
  vm.runInNewContext(bundle.outputFiles[0].text,context);const worker=context.sodeckPlugin;
  const flush=async()=>{for(let i=0;i<100;i++)await Promise.resolve();};
  return {worker,records,leases,execs,failCleanup(value){failCleanup=value;},async start(id='v'){await worker.prepare();await worker.activate();await worker.invoke('lifecycle.initialize',{...scope,serverId:'s',viewId:id});await flush();},async visible(value=true,id='v',revision=1){await worker.invoke('lifecycle.presentation',{viewId:id,visible:value,revision});await flush();},async advance(ms){const end=now+ms;for(let count=0;count<10000;count++){const next=[...timers].filter(([,t])=>t.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!next){now=end;await flush();return;}now=Math.max(now,next[1].at);timers.delete(next[0]);next[1].fn();await flush();}throw Error('timer spin');},snapshot(id='v'){return worker.invoke('monitor.snapshot',{viewId:id});}};
}
test('worker starts without dialing, samples on visibility, preserves pause and cleans only its lease',async()=>{
  const f=fixture();await f.start();assert.equal(f.leases.size,0);
  await f.visible();await f.advance(5500);const sample=await f.snapshot();assert.equal(sample.cpu.value.percent,20);assert.equal(sample.memory.value.percent,50);assert.equal(sample.connection,'connected');assert.equal(f.leases.size,1);
  const fast=()=>f.records.filter(r=>r.method==='exec.start'&&Buffer.from(r.p.stdinBase64,'base64').toString().includes('begin cpu')).length;
  await f.worker.invoke('monitor.setOptions',{viewId:'v',options:{paused:true}});await f.advance(1000);const count=fast();assert.equal(f.leases.size,0);
  await f.advance(10000);assert.equal(fast(),count);
  await f.worker.invoke('monitor.refresh',{viewId:'v',actionId:'once'});await f.advance(5000);assert.equal(fast(),count+1);assert.equal(f.leases.size,0);assert.equal((await f.snapshot()).options.paused,true);
  await f.worker.stop();assert.equal(f.execs.size,0);assert.equal(f.leases.size,0);
});
test('hidden views stop I/O, duplicate initialization does not duplicate a sampler, and requests are strict',async()=>{
  const f=fixture();await f.start();await f.start();await f.visible();await f.advance(5500);await f.visible(false,'v',2);await f.advance(1000);
  const starts=f.records.filter(r=>r.method==='exec.start').length;await f.advance(10000);assert.equal(f.records.filter(r=>r.method==='exec.start').length,starts);assert.equal(f.leases.size,0);
  await assert.rejects(f.worker.invoke('monitor.setOptions',{viewId:'v',options:{refreshSeconds:1}}),/周期/);
  await assert.rejects(f.worker.invoke('monitor.snapshot',{viewId:'v',serverId:'spoof'}),/未知参数/);
  await f.worker.stop();
});
test('64-controller limit and ninth visible target stay bounded and resume fairly',async()=>{
 const f=fixture();await f.start();for(let i=1;i<64;i++)await f.worker.invoke('lifecycle.initialize',{...scope,serverId:'s',viewId:'v'+i});
 const limited=await f.worker.invoke('lifecycle.initialize',{...scope,serverId:'s',viewId:'overflow'});assert.equal(limited.limited,true);
 for(let i=0;i<9;i++)await f.visible(true,i?'v'+i:'v');await f.advance(8000);
 const d=await f.worker.invoke('monitor.diagnostics',{viewId:'v'});assert.equal(d.controllers,64);assert.equal(d.scheduler.targets,8);assert.equal(d.scheduler.waiting,1);assert.equal((await f.snapshot('v8')).limited,true);
 await f.visible(false,'v',2);await f.advance(8000);assert.equal((await f.snapshot('v8')).limited,false);assert.notEqual((await f.snapshot('v8')).cpu.updatedAt,null);await f.worker.stop();assert.equal(f.leases.size,0);assert.equal(f.execs.size,0);
});
test('temporary exec cleanup failure retains the only lease and resumes after reclamation',async()=>{
 const f=fixture();await f.start();await f.visible();await f.advance(5500);const before=await f.snapshot(),lease=[...f.leases][0];
 f.failCleanup(true);await f.advance(5000);assert.equal((await f.snapshot()).cleanupPending,true);assert.deepEqual([...f.leases],[lease]);
 const held=await f.worker.invoke('monitor.diagnostics',{viewId:'v'});assert.ok(held.scheduler.cleanupSlots>0);assert.ok(held.scheduler.transientBytes<=4194304);
 f.failCleanup(false);await f.advance(15000);const after=await f.snapshot();assert.equal(after.cleanupPending,false);assert.equal(after.cpu.quality,'fresh');assert.ok(after.cpu.updatedAt>=before.cpu.updatedAt);
 assert.deepEqual([...f.leases],[lease]);assert.equal(f.records.filter(r=>r.method==='connections.acquireExisting').length,1);assert.equal(f.records.filter(r=>r.method==='connections.release').length,0);
 await f.worker.stop();assert.equal(f.execs.size,0);assert.equal(f.leases.size,0);
});
