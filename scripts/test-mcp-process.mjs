import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import http from 'node:http';
import {pluginPeer} from './plugin-peer.mjs';
import {testPluginDirectory} from './test-plugin-directory.mjs';
import {setTimeout as delay} from 'node:timers/promises';
const root=testPluginDirectory('mcp'),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-mcp-process-'));
const socket=net.createServer();await new Promise(r=>socket.listen(0,'127.0.0.1',r));const port=socket.address().port;await new Promise(r=>socket.close(r));
const contexts=Object.fromEntries(['a','b','c'].map(view=>[view,{workspaceId:view,serverId:'server-'+view,connectionId:'connection-'+view,generation:1}]));
const capabilities=['session.target.v1','session.stream.process.v1','files.pluginData.v1','files.operations.v2','ui.background.v1','settings.v1'];
const streams=new Map(),requests=[],releases=[],files=new Map(),batches=new Set(),failedBatches=new Set();let number=0,lost=true,peer,confirmUnknown=false,releaseGate;
const bytes=Buffer.from('中文 investigation\n'),chunk={v:1,sequence:'1',source:'stdout',receivedAt:Date.now(),dataBase64:bytes.toString('base64')};
peer=pluginPeer(path.join(root,manifest.entry),(method,p)=>{
  if(method==='settings.get')return {values:{port:String(port)}};
  if(['logging.write','messages.publish'].includes(method))return {};
  if(method==='session.state')return {context:contexts[p.viewId],state:'connected',closing:false,target:{name:p.viewId,protocol:'ssh',endpoint:p.viewId+':22',username:'ops'}};
  if(method==='session.stream.open'){
    requests.push(p);const streamId='stream-'+(++number);streams.set(streamId,{p,held:['hang','quiet','unknown'].includes(p.command),cancelled:false});
    if(p.command==='unknown')throw Object.assign(new Error('unknown receipt'),{code:-32003});
    if(lost){lost=false;throw Object.assign(new Error('lost receipt'),{code:-32003});}if(p.command==='delayed')return delay(300).then(()=>({streamId,state:'opening'}));return {streamId,state:'opening'};
  }
  if(method==='session.stream.lookup'){const entry=[...streams].find(([,s])=>s.p.operationId===p.operationId);if(!entry||entry[1].p.command==='unknown'&&!confirmUnknown)return {known:false};assert.deepEqual(entry[1].p,p);return {known:true,streamId:entry[0]};}
  if(method==='session.stream.poll'){
    const s=streams.get(p.streamId);assert(s);const done=!s.held||s.cancelled;
    const quiet=s.p.command==='quiet';return {chunks:s.p.command==='malformed'?null:!quiet&&p.offset==='0'?[chunk]:[],nextOffset:quiet?'0':'100',watermark:quiet?'0':'100',status:{state:s.cancelled?'cancelled':done?'completed':'running',execSent:true,remoteExitConfirmed:done&&!s.cancelled,workerFinished:done,finished:done,elapsedMs:Date.now(),outputComplete:done&&!s.cancelled&&s.p.command!=='output limit',error:s.p.command==='output limit'?{code:'outputLimit',message:'output limit'}:null,exitCode:s.p.command==='exit 7'?7:0}};
  }
  if(method==='session.stream.close'){streams.get(p.streamId).cancelled=true;return {accepted:true};}
  if(method==='session.stream.release'){releases.push(p.streamId);const stream=streams.get(p.streamId);streams.delete(p.streamId);if(stream.p.command==='final-save-fail')return new Promise(resolve=>{releaseGate=resolve;});return {released:true};}
  if(method==='files.request'){if(p.action==='open')return {id:'file-'+p.viewId};if(p.action==='close')return {};if(p.action==='batch.begin'){batches.add(p.batchId);return {};}if(p.action==='batch.end'){batches.delete(p.batchId);if(failedBatches.delete(p.batchId))throw new Error('batch end failed');return {};}}
  if(method==='files.operations'){
    if(p.action==='start'){
      if(p.operation.source.path==='/missing')throw Object.assign(new Error('missing'),{code:-32000});
      assert.equal(p.operation.destination.side,'pluginData');assert.equal(p.operation.destinationGuard.kind,'absent');
      const id='download-'+(++number);const output=path.join(directory,...p.operation.destination.relative);fs.writeFileSync(output,bytes);files.set(id,p);if(p.operation.source.path==='/cleanup-failure')failedBatches.add(p.batchId);return {id,state:'running'};
    }
    if(p.action==='poll')return {id:p.operationId,state:'completed',result:{size:String(bytes.length)}};
    if(p.action==='release'){const request=files.get(p.operationId);files.delete(p.operationId);if(request.operation.source.path==='/cleanup-failure')throw new Error('release failed');return {};}
    if(p.action==='cancel')return {};
  }
  throw new Error('Unexpected RPC: '+method);
});
let token,sequence=0;
async function call(method,params,headers={},signal){
  const response=await fetch(`http://127.0.0.1:${port}/mcp`,{method:'POST',signal,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25',...headers},body:JSON.stringify({jsonrpc:'2.0',id:++sequence,method,params})});
  assert.equal(response.status,200,await response.clone().text());return (await response.json()).result;
}
async function tool(name,arguments_={},signal){const result=await call('tools/call',{name,arguments:arguments_},{},signal);return {value:result.structuredContent??JSON.parse(result.content[0].text),error:result.isError};}
async function until(read,predicate){const end=Date.now()+10000;while(Date.now()<end){const value=await read();if(predicate(value))return value;await delay(30);}throw new Error('MCP task timeout');}
try{
  await assert.rejects(peer.request('initialize',{dataDirectory:directory,capabilities:[]}),/核心缺少/);
  await peer.request('initialize',{dataDirectory:directory,capabilities});token=fs.readFileSync(path.join(directory,'access-token'),'utf8');
  for(const [viewId,context]of Object.entries(contexts))await peer.request('ui.createView',{viewId,viewType:'status',context});
  await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.service==='ready');
  const initialized=await call('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'sodeck-test',version:'1'}});assert(initialized.capabilities.tools);
  const list=await call('tools/list',{});assert.equal(list.tools.length,6);assert.equal(list.tools.find(t=>t.name==='sodeck_exec_start').annotations.readOnlyHint,false);
  const targets=(await tool('sodeck_targets_list')).value.targets;assert.equal(targets.length,3);
  const page={viewId:'page-a',viewType:'operations',context:contexts.a};
  await peer.request('ui.createView',page);assert.equal((await tool('sodeck_targets_list')).value.targets.length,3,'operation pages reuse background targets');
  const target=targets.find(t=>t.context.workspaceId==='a').targetId;
  assert.deepEqual((await tool('sodeck_tasks_list')).value.tasks,[]);
  const input={targetId:target,requestId:'once',command:'collect'};
  const start=(await tool('sodeck_exec_start',input)).value;
  assert.equal((await tool('sodeck_exec_start',input)).value.taskId,start.taskId);
  assert.equal((await tool('sodeck_exec_start',{...input,command:'different'})).error,true);
  let done=await until(async()=>(await tool('sodeck_task_get',{taskId:start.taskId})).value,s=>s.done);
  assert.equal(done.evidence.complete,true);assert.equal(done.resourceReleased,true);assert.equal(done.output.stdout,bytes.toString());assert.equal(requests.length,1);
  assert.equal(done.input.command,'collect');assert.equal(done.target.username,'ops');assert(done.createdAt>0&&done.finishedAt>=done.createdAt);
  assert(start.taskId.includes(path.basename(path.dirname(done.directory))),'task identity includes its plugin run');
  const detail=(await peer.request('ui.event',{viewId:'a',data:{action:'task',taskId:start.taskId,cursor:'0'}})).data;
  assert.equal(detail.output.stdout,bytes.toString());assert.equal(detail.input.command,'collect');
  const pageDetail=(await peer.request('ui.event',{...page,data:{action:'task',taskId:start.taskId,cursor:'0'}})).data;assert.equal(pageDetail.output.stdout,bytes.toString());
  await assert.rejects(peer.request('ui.event',{viewId:'b',data:{action:'task',taskId:start.taskId}}),/任务不属于当前服务器连接/);
  assert.deepEqual((await tool('sodeck_task_get',{taskId:start.taskId})).value,done,'released task evidence remains queryable');
  assert.equal((await tool('sodeck_task_get',{taskId:start.taskId,cursor:'1'})).error,true);
  const textResult=(await tool('sodeck_task_get',{taskId:start.taskId,outputMode:'text'})).value;
  assert.equal(textResult.output.stdout,done.output.stdout);assert.equal(textResult.output.nextCursor,done.output.nextCursor);assert(!('chunks' in textResult.output));
  const none=(await tool('sodeck_task_get',{taskId:start.taskId,outputMode:'none'})).value;assert(!('output' in none));assert.equal(none.outputInfo.savedBytes,done.output.savedBytes);
  assert.equal((await tool('sodeck_task_get',{taskId:start.taskId,waitMs:20000})).value.wait.reason,'done');
  const cancelDone=(await tool('sodeck_task_cancel',{taskId:start.taskId})).value;assert.equal(cancelDone.cancelRequested,false);assert.deepEqual(cancelDone.result,none,'terminal cancellation does not change any facts');
  const quiet=(await tool('sodeck_exec_start',{targetId:target,requestId:'quiet',command:'quiet'})).value;
  const quietResult=await until(async()=>(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none'})).value,s=>s.native.state==='running');
  const timed=(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:180})).value;assert.equal(timed.wait.reason,'timeout');assert(timed.wait.elapsedMs>=160);assert.equal(timed.revision,quietResult.revision,'clock-only polling does not change revision');
  for(const p of [{waitMs:-1},{waitMs:0.5},{waitMs:20001},{outputMode:'bad'},{sinceRevision:'999999999'},{outputMode:'none',cursor:'1'}])assert.equal((await tool('sodeck_task_get',{taskId:quiet.taskId,...p})).error,true);
  const controllers=[new AbortController(),new AbortController()];
  const readers=controllers.map(c=>tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000},c.signal).catch(e=>e));
  await delay(120);const overflow=await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000});assert.equal(overflow.value.error.code,'resourceBusy');
  const abortAt=Date.now();controllers.forEach(c=>c.abort());await Promise.all(readers);
  await delay(120);const afterAbort=(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:50})).value;assert.equal(afterAbort.wait.reason,'timeout');assert(Date.now()-abortAt<1000,'HTTP disconnect releases waiting quotas within one second');assert.equal(afterAbort.done,false);
  const waking=tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000,sinceRevision:quietResult.revision});await delay(80);await tool('sodeck_task_cancel',{taskId:quiet.taskId});const woke=(await waking).value;assert(['changed','done'].includes(woke.wait.reason));assert(BigInt(woke.revision)>BigInt(quietResult.revision));await until(async()=>(await tool('sodeck_task_get',{taskId:quiet.taskId})).value,s=>s.done);
  const quotaTasks=[];for(let i=0;i<24;i++){const job=(await tool('sodeck_exec_start',{...input,requestId:'quota-'+i,command:'quiet'})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:job.taskId,outputMode:'none'})).value,s=>s.native.state==='running');quotaTasks.push(job);}
  assert.equal((await tool('sodeck_exec_start',{...input,requestId:'over-active',command:'must-not-run'})).value.error.code,'resourceBusy');
  const globalControllers=Array.from({length:16},()=>new AbortController());const globalReaders=globalControllers.map((c,i)=>tool('sodeck_task_get',{taskId:quotaTasks[Math.floor(i/2)].taskId,outputMode:'none',waitMs:20000},c.signal).catch(e=>e));await delay(150);
  assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none',waitMs:20000})).value.error.code,'resourceBusy');assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none'})).value.done,false,'short queries still work when waits are full');
  globalControllers.forEach(c=>c.abort());await Promise.all(globalReaders);await delay(120);assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none',waitMs:30})).value.wait.reason,'timeout');
  for(const job of quotaTasks)await tool('sodeck_task_cancel',{taskId:job.taskId});for(const job of quotaTasks)await until(async()=>(await tool('sodeck_task_get',{taskId:job.taskId,outputMode:'none'})).value,s=>s.done);
  const otherTarget=targets.find(t=>t.context.workspaceId==='b').targetId;
  const other=(await tool('sodeck_exec_start',{targetId:otherTarget,requestId:'scope-regression',command:'hang'})).value;
  await until(async()=>(await tool('sodeck_task_get',{taskId:other.taskId})).value,s=>s.native.state==='running');
  await assert.rejects(peer.request('ui.event',{viewId:'a',data:{action:'cancel',taskId:other.taskId}}),/任务不属于当前服务器连接/,'UI cancellation must use the same target scope as reads');
  await tool('sodeck_task_cancel',{taskId:other.taskId});
  await until(async()=>(await tool('sodeck_task_get',{taskId:other.taskId})).value,s=>s.done);
  const hang=(await tool('sodeck_exec_start',{...input,requestId:'hang',command:'hang'})).value;
  await until(async()=>(await tool('sodeck_task_get',{taskId:hang.taskId})).value,s=>s.native.state==='running');
  const live=(await peer.request('ui.event',{viewId:'a',data:{action:'task',taskId:hang.taskId,tail:true}})).data;
  assert.equal(live.output.stdout,bytes.toString());assert.equal(live.finishedAt,null);
  assert.equal((await tool('sodeck_task_get',{taskId:hang.taskId,outputMode:'text',waitMs:20000})).value.wait.reason,'output');
  await peer.request('ui.disposeView',page);assert.equal((await tool('sodeck_targets_list')).value.targets.length,3);assert.equal((await tool('sodeck_task_get',{taskId:hang.taskId})).value.done,false,'closing the page does not cancel background operations');
  await peer.request('ui.createView',page);assert((await peer.request('ui.getView',page)).data.tasks.some(task=>task.taskId===hang.taskId),'reopened page sees the same operation');
  for(let i=0;i<51;i++){
    const recent=(await tool('sodeck_exec_start',{...input,requestId:'recent-'+i,command:'collect'})).value;
    await until(async()=>(await tool('sodeck_task_get',{taskId:recent.taskId})).value,s=>s.done);
  }
  const history=(await peer.request('ui.getView',{viewId:'a'})).data.tasks;
  assert.equal(history.length,50);assert.equal(history[0].taskId,hang.taskId,'active operations remain visible beyond the recent record limit');
  assert(!('input' in history[0])&&!('directory' in history[0]));assert((await peer.request('ui.getView',{viewId:'a'})).data.recordCount>50);
  const firstPage=(await tool('sodeck_tasks_list',{targetId:target,limit:20})).value;assert.equal(firstPage.tasks.length,20);assert(firstPage.hasMore);assert.equal(firstPage.counts.active,1);
  const inserted=(await tool('sodeck_exec_start',{...input,requestId:'inserted',command:'collect'})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:inserted.taskId})).value,s=>s.done);
  let next=firstPage,all=[...next.tasks];while(next.hasMore){next=(await tool('sodeck_tasks_list',{targetId:target,limit:20,cursor:next.nextCursor})).value;all.push(...next.tasks);}
  assert.equal(new Set(all.map(t=>t.taskId)).size,all.length);assert.equal(all.length,firstPage.counts.active+firstPage.counts.done);assert(!all.some(t=>t.taskId===inserted.taskId),'new records do not enter an existing pagination ceiling');
  assert.equal((await tool('sodeck_tasks_list',{targetId:target,limit:21,cursor:firstPage.nextCursor})).value.error.code,'invalidCursor');
  const foreign=JSON.parse(Buffer.from(firstPage.nextCursor,'base64'));foreign.run='old-run';assert.equal((await tool('sodeck_tasks_list',{targetId:target,limit:20,cursor:Buffer.from(JSON.stringify(foreign)).toString('base64')})).value.error.code,'runMismatch');
  const filtered=(await tool('sodeck_tasks_list',{serverId:'server-a',requestId:'quiet',kind:'exec',status:'done'})).value;assert.equal(filtered.tasks.length,1);assert.equal(filtered.tasks[0].taskId,quiet.taskId);assert.equal(filtered.counts.done,1);
  const uiCancel=(await peer.request('ui.event',{viewId:'a',data:{action:'cancel',taskId:hang.taskId}})).data;assert.equal(uiCancel.cancelRequested,true);assert.equal(uiCancel.result.taskId,hang.taskId);assert(!('input' in uiCancel.result));done=await until(async()=>(await tool('sodeck_task_get',{taskId:hang.taskId})).value,s=>s.done);assert.equal(done.native.state,'cancelled');assert.equal(done.evidence.complete,false);
  const nonzero=(await tool('sodeck_exec_start',{...input,requestId:'exit',command:'exit 7'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:nonzero.taskId})).value,s=>s.done);assert.equal(done.native.exitCode,7);assert.equal(done.evidence.complete,true);
  const delayed=(await tool('sodeck_exec_start',{...input,requestId:'delayed',command:'delayed'})).value;
  await until(async()=>requests.some(p=>p.command==='delayed'),Boolean);
  const busy=await tool('sodeck_exec_start',{...input,requestId:'queued',command:'must-not-run'});assert.equal(busy.error,true);assert.equal(busy.value.error.code,'resourceBusy');assert(!busy.value.taskId);assert(!requests.some(p=>p.command==='must-not-run'));assert.equal((await tool('sodeck_exec_start',{...input,requestId:'delayed',command:'delayed'})).value.taskId,delayed.taskId);await until(async()=>(await tool('sodeck_task_get',{taskId:delayed.taskId})).value,s=>s.done);
  const download=(await tool('sodeck_file_download',{targetId:target,requestId:'download',remotePath:'/tmp/consul.tar.gz'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:download.taskId})).value,s=>s.done);assert.equal(done.evidence.complete,true);assert.deepEqual(fs.readFileSync(done.evidence.path),bytes);assert.equal(files.size,0);assert.equal(batches.size,0);
  assert.equal(JSON.parse(fs.readFileSync(path.join(done.directory,'metadata.json'),'utf8')).target.username,'ops');
  for(const command of ['malformed','output limit']){const task=(await tool('sodeck_exec_start',{...input,requestId:command,command})).value;const result=await until(async()=>(await tool('sodeck_task_get',{taskId:task.taskId})).value,s=>s.done);assert.equal(result.evidence.complete,false);assert.equal(result.resourceReleased,true);}
  const missing=(await tool('sodeck_file_download',{targetId:target,requestId:'missing',remotePath:'/missing'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:missing.taskId})).value,s=>s.done);assert.equal(done.native.admission,'rejected');assert.equal(batches.size,0,'known rejection ends the batch');
  const cleanup=(await tool('sodeck_file_download',{targetId:target,requestId:'cleanup',remotePath:'/cleanup-failure'})).value;const cleaned=await until(async()=>(await tool('sodeck_task_get',{taskId:cleanup.taskId})).value,s=>s.done);assert.equal(cleaned.evidence.complete,true);assert.equal(cleaned.resourceReleased,false);assert.deepEqual(cleaned.cleanupError.details.map(e=>e.step),['release','batch.end']);
  const failing=(await tool('sodeck_exec_start',{...input,requestId:'final-save-fail',command:'final-save-fail'})).value;await until(async()=>typeof releaseGate==='function',Boolean);
  const finalizing=(await tool('sodeck_task_get',{taskId:failing.taskId,outputMode:'none'})).value;assert.equal(finalizing.phase,'finalizing');assert.equal(finalizing.done,false);assert.equal(finalizing.evidence.complete,false);
  assert.equal((await tool('sodeck_task_cancel',{taskId:failing.taskId})).value.cancelRequested,false);
  fs.mkdirSync(path.join(finalizing.directory,'metadata.tmp'));releaseGate({released:true});const failed=await until(async()=>(await tool('sodeck_task_get',{taskId:failing.taskId,outputMode:'none'})).value,s=>s.done);assert.equal(failed.evidence.state,'failed');assert.equal(failed.evidence.complete,false);assert.equal(failed.resourceReleased,true);
  fs.rmdirSync(path.join(finalizing.directory,'metadata.tmp'));assert.deepEqual((await tool('sodeck_task_get',{taskId:failing.taskId,outputMode:'none'})).value,failed);assert.equal(JSON.parse(fs.readFileSync(path.join(finalizing.directory,'metadata.json'))).phase,'finalizing','failed final save leaves last valid disk facts');
  const unknown=(await tool('sodeck_exec_start',{...input,requestId:'unknown',command:'unknown'})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:unknown.taskId,outputMode:'none'})).value,s=>s.native.admission==='unknown');
  await tool('sodeck_task_cancel',{taskId:unknown.taskId});assert.equal((await tool('sodeck_exec_start',{...input,requestId:'unknown',command:'unknown'})).value.taskId,unknown.taskId);assert.equal((await tool('sodeck_exec_start',{...input,requestId:'must-not-replay',command:'collect'})).value.error.code,'resourceBusy');assert.equal(requests.filter(p=>p.command==='unknown').length,1);confirmUnknown=true;await until(async()=>(await tool('sodeck_task_get',{taskId:unknown.taskId})).value,s=>s.done);
  for(const headers of [{Authorization:'Bearer wrong'},{Origin:'https://example.com'},{Host:'example.com'}]){
    const status=await new Promise((resolve,reject)=>{const request=http.request({hostname:'127.0.0.1',port,path:'/mcp',method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream',...headers}},response=>{response.resume();response.on('end',()=>resolve(response.statusCode));});request.on('error',reject);request.end(JSON.stringify({jsonrpc:'2.0',id:99,method:'tools/list',params:{}}));});
    assert([401,403].includes(status),JSON.stringify({headers:Object.keys(headers),status}));
  }
  contexts.a={...contexts.a,generation:2};assert.equal((await tool('sodeck_exec_start',{...input,requestId:'stale'})).value.taskId.startsWith('task-'),true);
  await assert.rejects(peer.request('ui.getView',{...page,context:contexts.a}),/后台目标不可用/,'page must not reuse a target from another connection generation');
  const countBefore=requests.length;await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.tasks.some(t=>t.requestId==='stale'&&t.done));assert.equal(requests.length,countBefore,'stale target must not execute');
  for(const viewId of Object.keys(contexts))await peer.request('ui.disposeView',{viewId});
  await peer.close();peer=null;await assert.rejects(fetch(`http://127.0.0.1:${port}/mcp`));
  console.log('MCP: six tools, scopes, stable finalization, text/none, keyset paging, wait timeout/wake/disconnect quotas, unknown admission, cleanup failures and exit passed');
}finally{if(peer)await peer.kill();assert.equal(path.dirname(directory),os.tmpdir());fs.rmSync(directory,{recursive:true,force:true});}
