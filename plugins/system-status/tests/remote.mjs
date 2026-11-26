// Explicit opt-in: actual core and plugin against a supplied SSH device; no server configuration changes.
import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import readline from 'node:readline';
import {spawn} from 'node:child_process';import {setTimeout as delay} from 'node:timers/promises';
import {fixtureBinary} from '../../../scripts/cargo-fixture.mjs';
const required=name=>{const value=process.env[name];assert(value,`Missing ${name}`);return value;};
const host=required('SODECK_STATUS_REMOTE_HOST'),username=required('SODECK_STATUS_REMOTE_USER'),key=required('SODECK_STATUS_REMOTE_KEY');
const plugin=path.resolve(required('SODECK_TEST_SYSTEM_STATUS_PLUGIN'));
const output=path.resolve(process.env.SODECK_STATUS_REMOTE_REPORT||'artifacts/system-status-remote/report.json');
const duration=Number(process.env.SODECK_STATUS_REMOTE_SECONDS||300);assert(duration>=30&&duration<=1800);
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-status-remote-'));
const child=spawn(fixtureBinary('catalog-fixture'),[directory],{stdio:['pipe','pipe','inherit'],windowsHide:true});
const pending=[];let failure;
readline.createInterface({input:child.stdout}).on('line',line=>{const reply=JSON.parse(line);if(reply.event)return;const p=pending.shift();assert(p,'Unexpected fixture response');reply.error?p.reject(new Error(JSON.stringify(reply.error))):p.resolve(reply.value);});
const exited=new Promise(resolve=>child.on('exit',(code,signal)=>{failure=new Error(`Core exited ${code}/${signal}`);for(const p of pending.splice(0))p.reject(failure);resolve({code,signal});}));
const call=(command,args={})=>new Promise((resolve,reject)=>{if(failure)return reject(failure);pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const ui=(operation,args={})=>call('component_ui',{window:'main',operation,args});
const wait=async(fn,label,timeout=60000)=>{const end=Date.now()+timeout;while(true){const value=await fn();if(value)return value;assert(Date.now()<end,'Timed out: '+label);await delay(200);}};
const report={passed:false,startedAt:new Date().toISOString(),plugin:JSON.parse(fs.readFileSync(path.join(plugin,'plugin.json'),'utf8')).version,samples:[],checks:[],skipped:['external disks','other macOS versions','physical native UI input']};
let surface,context;
const event=data=>ui('event',{surfaceId:surface,event:data});
const snapshot=()=>event({action:'snapshot'});
async function connect(serverId){await call('connection',{command:{op:'start',serverId}});return wait(async()=>{const all=await call('connection',{command:{op:'snapshot'}});const c=all.find(c=>c.serverId===serverId)||all[0];if(c?.question){assert(c.question.kind==='hostKey'||c.question.type==='hostKey',JSON.stringify(c.question));await call('connection',{command:{op:'answer',attemptId:c.attemptId,questionId:c.question.id,answer:{kind:'hostKey',accept:true}}});}assert(c?.state!=='failed',JSON.stringify(c?.error));return c?.state==='connected'&&c;},'SSH connection');}
try{
 await call('fixture_load_plugin',{directory:plugin});
 const catalog=await call('server_catalog');const saved=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'System status device verification',host,port:Number(process.env.SODECK_STATUS_REMOTE_PORT||22),username,authentication:{type:'privateKey',privateKeyPath:key,secret:{action:'clear'}},route:[]}}});
 const c=await connect(saved.selectedId);context=c.context;await call('connection',{command:{op:'focus',context}});
 let view=await wait(async()=>{const s=await ui('snapshot');return s.workspaces[context.workspaceId]?.views.find(v=>v.pluginId==='sodeck.system-status');},'background view');
 const attach=await ui('attach',{viewId:view.id});surface=attach.surfaceId;await ui('ready',{surfaceId:surface});
 const ready=await wait(async()=>{const s=await snapshot();return s.data.memory.state==='ok'&&s.data.cpu.state==='ok'&&s.disk.rates?.state==='ok'&&s.network.device?.addresses?.length&&s;},'complete device metrics');
 assert.equal(ready.family,'macOS');assert.equal(ready.network.device.name,'en0');assert.equal(ready.data.identity.architecture,'ARM64');assert(ready.data.memory.estimated);assert(Number(ready.data.memory.available)<=Number(ready.data.memory.total));
 const files=await event({action:'list',kind:'filesystems',group:'primary'});assert(files.items.some(r=>r.path==='/System/Volumes/Data'));assert(files.items.some(r=>r.path==='/'&&r.filesystemType==='apfs'&&r.readOnly===true));
 report.checks.push('identity, CPU, memory, IPv4/IPv6, default interface, disk I/O, APFS metadata');
 const began=Date.now();let lastAt=0;
 while(Date.now()-began<duration*1000){const s=await snapshot();report.lastSnapshot=s;assert(s.connected,"Device disconnected: "+JSON.stringify(s.groups));assert(!s.groups[1].observedAt||Date.now()-s.groups[1].observedAt<25000,"Dynamic sampling stalled: "+JSON.stringify(s));const at=s.groups.find(g=>g.name==='dynamic').observedAt;if(at!==lastAt){lastAt=at;assert.equal(s.data.memory.state,'ok',JSON.stringify(s.data.sourceErrors));assert.equal(s.data.cpu.state,'ok');assert.equal(s.disk.rates.state,'ok');assert.equal(s.network.rates.state,'ok');report.samples.push({at,cpu:s.data.cpu.value,memory:s.data.memory.value,available:s.data.memory.available,network:{first:s.network.rates.first,second:s.network.rates.second},disk:{first:s.disk.rates.first,second:s.disk.rates.second}});}await delay(1000);}
 const history=await event({action:'history',seriesKey:'memory'});assert(history.points.filter(p=>Number.isFinite(p.value)).length>=Math.min(5,duration/10));assert(history.points.length<=100);report.checks.push('continuous real metrics and memory history');
 const before=(await snapshot()).groups[1].observedAt;await ui('action',{context,action:{op:'collapse',side:'right'}});await delay(8000);assert((await snapshot()).groups[1].observedAt>before);report.checks.push('hidden panel continues sampling');
 await event({action:'pause'});await delay(1200);const paused=await snapshot();await delay(6000);assert.equal((await snapshot()).groups[1].observedAt,paused.groups[1].observedAt);await event({action:'refresh'});await wait(async()=>{const s=await snapshot();return s.paused&&s.groups[1].observedAt>paused.groups[1].observedAt;},'paused single refresh');await delay(10000);const refreshed=await snapshot();await delay(6000);assert.equal((await snapshot()).groups[1].observedAt,refreshed.groups[1].observedAt);await event({action:'resume'});await wait(async()=>{const s=await snapshot();return !s.paused&&s.groups[1].observedAt>refreshed.groups[1].observedAt;},'resume');report.checks.push('pause, exactly one refresh round, resume');
 const interfaces=await event({action:'list',kind:'interfaces'});const lo=interfaces.items.find(d=>d.name==='lo0');assert(lo);await event({action:'navigation',state:{interfaceId:lo.id}});assert.equal((await snapshot()).network.device.id,lo.id);await event({action:'navigation',state:{interfaceId:ready.network.device.id}});report.checks.push('manual interface selection');
 const oldContext=context;await call('connection',{command:{op:'disconnect',context}});await wait(async()=>!(await snapshot()).connected,'disconnect status');assert.equal((await snapshot()).data.memory.total,ready.data.memory.total);
 const reconnected=await connect(saved.selectedId);context=reconnected.context;assert.notDeepEqual(context,oldContext);await ui('detach',{surfaceId:surface});
 view=await wait(async()=>{const s=await ui('snapshot');return s.workspaces[context.workspaceId]?.views.find(v=>v.pluginId==='sodeck.system-status');},'reconnected view');
 const reattach=await ui('attach',{viewId:view.id});surface=reattach.surfaceId;await ui('ready',{surfaceId:surface});
 await wait(async()=>{const s=await snapshot();return s.connected&&s.data.memory.state==='ok'&&s.disk.rates?.state==='ok';},'reconnected real metrics');report.checks.push('disconnect keeps last values, new Context reconnect warms fresh counters');
 report.lastSnapshot=await snapshot();report.passed=true;
}catch(e){report.error=e.stack;throw e;}finally{
 child.stdin.end();const end=await Promise.race([exited,delay(10000).then(()=>null)]);if(!end){child.kill();await exited;}report.coreExit=end;report.finishedAt=new Date().toISOString();fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');assert.equal(path.dirname(directory),os.tmpdir());fs.rmSync(directory,{recursive:true,force:true});
}
console.log(`Remote status passed: ${report.samples.length} distinct dynamic samples; ${report.checks.join('; ')}`);
