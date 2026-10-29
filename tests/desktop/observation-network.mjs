import assert from 'node:assert/strict';
import net from 'node:net';
import {readFile,writeFile,readdir,mkdir} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {networkHarness} from './network-harness.mjs';
import {rpc,until,delay} from './harness.mjs';
const output=resolve('.qa/t016/network');await mkdir(output,{recursive:true});
const h=await networkHarness('t016-observation',9621);let passed=false;
const markers=['URL_T016_790b','HEADER_T016_f7a1','CREDENTIAL_T016_612d','BODY_T016_cc81'];
async function events(type){let cursor,items=[];const filter={...(type?{type}:{}),fromUtc:Date.now()-600000,toUtc:Date.now()+1000};do{const p=await rpc(h.app.page,'events.v2.query',{filter,limit:200,...(cursor?{cursor}:{})});items.push(...p.items);cursor=p.nextCursor;}while(cursor);return items;}
async function request(port,text){return new Promise((resolve,reject)=>{const socket=net.connect(port,'127.0.0.1',()=>socket.end(text));let output='';socket.on('data',b=>output+=b);socket.setTimeout(10000,()=>socket.destroy(Error('client timeout')));socket.on('error',reject);socket.on('close',()=>resolve(output));});}
async function scan(path){for(const entry of await readdir(path,{withFileTypes:true})){const target=join(path,entry.name);if(entry.isDirectory())await scan(target);else{const bytes=await readFile(target);for(const marker of markers)assert.ok(!bytes.includes(Buffer.from(marker)),`prohibited marker in ${entry.name}`);}}}
try{
 await h.start();await h.create();await h.trust();await h.open();
 const policy=await rpc(h.app.page,'observability.v2.policy.get');await rpc(h.app.page,'observability.v2.policy.set',{expectedRevision:policy.revision,patch:{logLevel:'debug',debugUntil:Date.now()+300000}});
 const proxy=await h.save(),fixed=await h.save({type:'port-forward',targetHost:'http.internal',targetPort:80});await h.run(proxy);await h.run(fixed);
 const before=(await events('core.connection.ended')).length,oldProxy=await h.state(proxy),oldFixed=await h.state(fixed);
 const transport=(await rpc(h.app.page,'net.resources.list')).items.find(i=>i.kind==='transport');assert.ok(transport);
 const stopped=await rpc(h.app.page,'net.resources.stop',{resource:transport.handle,operationId:crypto.randomUUID()});assert.equal(stopped.result.affectedGroups.length,2);
 await until(async()=>(await h.state(proxy)).status==='needs-attention'&&(await h.state(fixed)).status==='needs-attention');
 const ended=await until(async()=>{const e=await events('core.connection.ended');return e.length>before&&e.slice(0,e.length-before);});
 assert.equal(new Set(ended.map(e=>e.attributes.resourceRef)).size,ended.length);assert.ok(ended.every(e=>e.source.instanceId==='connection-broker'&&e.provenance==='core-observed'));
 for(const run of [oldProxy,oldFixed]){const states=(await events('plugin.sodeck.network-proxy.run.state.changed')).filter(e=>e.attributes.runId===run.id&&e.attributes.to==='needs-attention');assert.equal(states.length,1);assert.ok(states[0].targets.some(t=>t.kind==='resource'));}
 await h.run(proxy,false,'network.reconnect');await h.run(fixed,false,'network.reconnect');assert.equal(await h.curl(oldProxy.address.port),'REMOTE-http.internal');
 h.note('Shared transport stop has one core lifecycle end per physical connection and one impact state per Run; manual recovery retains both run identities');
 const first=(await events('plugin.sodeck.network-proxy.proxy.protocol.failed')).length;
 for(let i=0;i<32;i++){await request(oldProxy.address.port,'BAD\n\n');if(i%8===7)assert.equal(await h.curl(oldProxy.address.port),'REMOTE-http.internal');}
 await until(async()=>(await events('plugin.sodeck.network-proxy.proxy.protocol.failed')).length===first+32);
 const meterBefore=await rpc(h.app.page,'communication.summary');
 const response=await request(oldProxy.address.port,`POST http://http.internal/${markers[0]} HTTP/1.1\r\nHost: http.internal\r\nAuthorization: Bearer ${markers[2]}\r\nX-Test: ${markers[1]}\r\nContent-Length: ${markers[3].length}\r\nConnection: close\r\n\r\n${markers[3]}`);assert.ok(response.includes('REMOTE-http.internal'));
 const meterAfter=await until(async()=>{const v=await rpc(h.app.page,'communication.summary');return BigInt(v.sentBytes)>BigInt(meterBefore.sentBytes)&&BigInt(v.receivedBytes)>BigInt(meterBefore.receivedBytes)&&v;});
 h.note('32 bad HTTP clients each produce one failure while interleaved normal traffic succeeds; core communication counters increase for actual HTTP traffic');
 const automatic=await h.save({autoReconnect:true});await h.run(automatic);const autoRun=await h.state(automatic);
 await h.control('block',0);await until(async()=>(await events('plugin.sodeck.network-proxy.run.reconnect.completed')).some(e=>e.attributes.runId===autoRun.id&&e.attributes.trigger==='automatic'&&e.outcome==='failed'),15000);
 await h.control('unblock',0);await h.run(automatic,false,'network.reconnect');
 const attempts=(await events('plugin.sodeck.network-proxy.run.reconnect.completed')).filter(e=>e.attributes.runId===autoRun.id);
 const manual=attempts.find(e=>e.attributes.trigger==='user'&&e.outcome==='succeeded'),automaticAttempt=attempts.find(e=>e.attributes.attemptId===manual?.attributes.retryOf);
 assert.ok(manual&&automaticAttempt);assert.notEqual(manual.attributes.attemptId,automaticAttempt.attributes.attemptId);assert.notEqual(manual.traceId,automaticAttempt.traceId);
 await h.invoke('network.stop',{id:automatic.id});await h.invoke('network.stop',{id:proxy.id});await h.invoke('network.stop',{id:fixed.id});await until(async()=>(await rpc(h.app.page,'net.resources.list')).items.length===0);
 h.note('Automatic failed attempt and successful manual retry retain run identity with distinct trace/attempt and explicit retryOf; all resources release');
 const evidence=await rpc(h.app.page,'evidence.create',{filter:{excludeDiagnosticCalls:true},allowPartial:true});await until(async()=>['succeeded','partial'].includes((await rpc(h.app.page,'evidence.get',{jobId:evidence.jobId})).job.state),30000);
 let text='',offset=0;for(;;){const p=await rpc(h.app.page,'evidence.get',{jobId:evidence.jobId,offset});text+=p.chunks.join('');if(p.nextOffset===null)break;offset=p.nextOffset;}
 for(const marker of markers)assert.ok(!text.includes(marker));await scan(join(h.app.data,'observability'));await writeFile(join(output,'evidence.jsonl'),text);
 const rows=text.trim().split('\n').map(s=>JSON.parse(s));assert.ok(!rows.filter(r=>r.recordType==='metric'&&r.value.source?.pluginId==='sodeck.network-proxy').some(r=>/bytes|traffic|bandwidth/i.test(r.value.type??'')));
 h.note('URL, authorization credential, header and body markers stay out of debug observation files and evidence; plugin metrics do not duplicate core traffic bytes');
 const data=h.app.data;await h.stop();await scan(join(data,'observability'));passed=true;await writeFile(join(output,'results.json'),JSON.stringify({passed,checks:h.checks,data,fixture:h.output,ended,attempts,meterBefore,meterAfter},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,checks:h.checks,fixture:h.output},null,2));throw e;}
finally{await h.close();}
