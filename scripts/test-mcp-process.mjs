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
assert.deepEqual(manifest.ui.views.map(({id,container})=>({id,container})),[{id:'status',container:'sidebar'}],'MCP contributes only its existing sidebar');
assert.equal(manifest.name,'Agent MCP');assert.equal(manifest.ui.views[0].title,'Agent MCP');assert.equal(manifest.settings.pages[0].title,'Agent MCP');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-mcp-process-'));
const socket=net.createServer();await new Promise(r=>socket.listen(0,'127.0.0.1',r));const port=socket.address().port;await new Promise(r=>socket.close(r));
const contexts=Object.fromEntries(['a','b','c'].map(view=>[view,{workspaceId:view,serverId:'server-'+view,connectionId:'connection-'+view,generation:1}]));
const capabilities=['session.target.v1','session.stream.process.v1','files.pluginData.v1','files.operations.v2','ui.background.v1','settings.v1'];
const streams=new Map(),requests=[],releases=[],files=new Map(),batches=new Set(),failedBatches=new Set();let number=0,lost=true,peer,confirmUnknown=false,releaseGate;
const bytes=Buffer.from('Architecture: x86_64\nCPU(s): 56\nThread(s) per core: 2\nCore(s) per socket: 14\nSocket(s): 2\nModel name: Intel(R) Xeon(R) Gold 5117 CPU @ 2.00GHz\nL3 cache: 38.5 MiB\nVirtualization: VT-x\n'),chunk={v:1,sequence:'1',source:'stdout',receivedAt:Date.now(),dataBase64:bytes.toString('base64')};
peer=pluginPeer(path.join(root,manifest.entry),(method,p)=>{
  if(method==='settings.get')return {values:{port:String(port)}};
  if(['logging.write','messages.publish'].includes(method))return {};
  if(method==='session.state')return {context:contexts[p.viewId],state:'connected',closing:false,target:{name:p.viewId,protocol:'ssh',endpoint:p.viewId+':22',username:'ops'}};
  if(method==='session.stream.open'){
    requests.push(p);const streamId='stream-'+(++number);streams.set(streamId,{p,held:['hang','quiet','unknown'].includes(p.command),cancelled:false,started:Date.now()});
    if(p.command==='unknown')throw Object.assign(new Error('unknown receipt'),{code:-32003});
    if(lost){lost=false;throw Object.assign(new Error('lost receipt'),{code:-32003});}if(p.command==='delayed')return delay(300).then(()=>({streamId,state:'opening'}));return {streamId,state:'opening'};
  }
  if(method==='session.stream.lookup'){const entry=[...streams].find(([,s])=>s.p.operationId===p.operationId);if(!entry||entry[1].p.command==='unknown'&&!confirmUnknown)return {known:false};assert.deepEqual(entry[1].p,p);return {known:true,streamId:entry[0]};}
  if(method==='session.stream.poll'){
    const s=streams.get(p.streamId);assert(s);if(s.p.command.startsWith('large-error'))throw Object.assign(new Error('diagnostic payload'),{code:-32000,data:{payload:'\\'.repeat(26000)}});const done=!s.held||s.cancelled;
    if(s.p.command==='changing'){const n=Number(p.offset)+1,finished=s.cancelled||Date.now()-s.started>=3000;return {chunks:[{...chunk,sequence:String(n),dataBase64:Buffer.from('line-'+n+'\n').toString('base64')}],nextOffset:String(n),watermark:String(n),status:{state:finished?'completed':'running',workerFinished:finished,execSent:true,exitCode:0,remoteExitConfirmed:true,outputComplete:true,received:n}};}
    if(s.p.command==='pages')return {chunks:p.offset==='0'?[1,2].map(n=>({...chunk,sequence:String(n),dataBase64:Buffer.from(String(n).repeat(9000)).toString('base64')})):[],nextOffset:'2',watermark:'2',status:{state:'completed',workerFinished:true,execSent:true,exitCode:0,remoteExitConfirmed:true,outputComplete:true}};
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
const samples=[],shortRuns=[];let token,sequence=0;
async function call(method,params,headers={},signal){
  const started=performance.now();
  const response=await fetch(`http://127.0.0.1:${port}/mcp`,{method:'POST',signal,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25',...headers},body:JSON.stringify({jsonrpc:'2.0',id:++sequence,method,params})});
  assert.equal(response.status,200,await response.clone().text());const envelope=await response.json(),r=envelope.result??envelope;if(method==='tools/call'&&samples.length<32)samples.push({tool:params.name,args:params.arguments,elapsedMs:performance.now()-started,bytes:Buffer.byteLength(JSON.stringify(r)),result:r});return r;
}
async function tool(name,arguments_={},signal){const result=await call('tools/call',{name,arguments:arguments_},{},signal);assert(!('structuredContent' in result));assert.equal(result.content.length,1);assert.equal(result.content[0].type,'text');assert(Buffer.byteLength(JSON.stringify(result))<=256*1024);return {value:JSON.parse(result.content[0].text),error:result.isError};}
async function until(read,predicate){const end=Date.now()+10000;while(Date.now()<end){const value=await read();if(predicate(value))return value;await delay(30);}throw new Error('MCP task timeout');}
try{
  await assert.rejects(peer.request('initialize',{dataDirectory:directory,capabilities:[]}),/核心缺少/);
  await peer.request('initialize',{dataDirectory:directory,capabilities});token=fs.readFileSync(path.join(directory,'access-token'),'utf8');
  for(const [viewId,context]of Object.entries(contexts))assert.equal((await peer.request('ui.createView',{viewId,viewType:'status',context})).metadata.title,'Agent MCP');
  await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.service==='ready');
  const initialized=await call('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'sodeck-test',version:'1'}});assert(initialized.capabilities.tools);
  const list=await call('tools/list',{});assert.equal(list.tools.length,6);assert.equal(list.tools.find(t=>t.name==='sodeck_exec_start').annotations.readOnlyHint,false);
  const targets=(await tool('sodeck_targets_list')).value.targets;assert.equal(targets.length,3);
  const sidebar={viewId:'a',viewType:'status',context:contexts.a};
  await peer.request('ui.createView',sidebar);assert.equal((await tool('sodeck_targets_list')).value.targets.length,3,'the sidebar reuses its background target');
  await assert.rejects(peer.request('ui.createView',{viewId:'old-page',viewType:'operations',context:contexts.a}),/未知视图/);
  const target=targets.find(t=>t.context.workspaceId==='a').targetId;
  assert.deepEqual((await tool('sodeck_tasks_list')).value.tasks,[]);
  const input={targetId:target,requestId:'once',command:'collect'};
  const start=(await tool('sodeck_exec_start',input)).value;
  assert.equal(start.done,true);assert.equal(start.wait.reason,'done');assert.equal(start.wait.requestedMs,1000);assert.equal(start.output.stdout,bytes.toString());
  assert.equal((await tool('sodeck_exec_start',input)).value.taskId,start.taskId);
  assert.equal((await tool('sodeck_exec_start',{...input,command:'different'})).error,true);
  let done=await until(async()=>(await tool('sodeck_task_get',{taskId:start.taskId})).value,s=>s.done);
  assert.equal(done.evidence.complete,true);assert.equal(done.resourceReleased,true);assert.equal(done.output.stdout,bytes.toString());assert.equal(requests.length,1);
  assert.equal(done.target.username,'ops');for(const key of ['input','context','directory','createdAt','finishedAt','observedAt','outputInfo'])assert(!(key in done));assert(!('workerFinished' in done.native));assert(!('chunks' in done.output));
  const diagnostic=(await tool('sodeck_task_get',{taskId:start.taskId,detail:true,outputMode:'none'})).value;
  assert.equal(diagnostic.input.command,'collect');assert.deepEqual(diagnostic.context,contexts.a);assert(diagnostic.createdAt>0&&diagnostic.finishedAt>=diagnostic.createdAt);assert(!('output' in diagnostic));
  assert(start.taskId.includes(path.basename(path.dirname(diagnostic.directory))),'task identity includes its plugin run');
  const original=(await tool('sodeck_task_get',{taskId:start.taskId,outputMode:'full'})).value;
  assert(!('stdout' in original.output)&&!('stderr' in original.output)&&!('previewEncoding' in original.output));assert.deepEqual(Buffer.from(original.output.chunks[0].dataBase64,'base64'),bytes);assert.equal(original.output.nextCursor,done.output.nextCursor);
  const outputPath=path.join(diagnostic.directory,'output.jsonl');fs.renameSync(outputPath,outputPath+'.held');
  const noBody=(await tool('sodeck_task_get',{taskId:start.taskId,detail:true,outputMode:'none'})).value;assert.equal(noBody.done,true);
  const readFailure=await tool('sodeck_task_get',{taskId:start.taskId});assert.equal(readFailure.error,true);assert.equal(readFailure.value.error.code,'evidenceReadFailed');assert.equal(readFailure.value.task.revision,done.revision);assert(!('task' in readFailure.value.error));assert(!('input' in readFailure.value.task));
  const startReadFailure=await tool('sodeck_exec_start',{...input,waitMs:20000});assert.equal(startReadFailure.error,true);assert.equal(startReadFailure.value.task.taskId,start.taskId);assert.equal(startReadFailure.value.task.revision,done.revision);assert.equal(requests.length,1);
  const detailFailure=await tool('sodeck_task_get',{taskId:start.taskId,detail:true});assert.equal(detailFailure.value.task.input.command,'collect');fs.renameSync(outputPath+'.held',outputPath);
  const detail=(await peer.request('ui.event',{viewId:'a',data:{action:'task',taskId:start.taskId,cursor:'0'}})).data;
  assert.equal(detail.output.stdout,bytes.toString());assert.equal(detail.input.command,'collect');
  await assert.rejects(peer.request('ui.event',{viewId:'b',data:{action:'task',taskId:start.taskId}}),/任务不属于当前服务器连接/);
  assert.deepEqual((await tool('sodeck_task_get',{taskId:start.taskId})).value,done,'released task evidence remains queryable');
  assert.equal((await tool('sodeck_task_get',{taskId:start.taskId,cursor:'1'})).error,true);
  const textResult=(await tool('sodeck_task_get',{taskId:start.taskId,outputMode:'text'})).value;
  assert.equal(textResult.output.stdout,done.output.stdout);assert.equal(textResult.output.nextCursor,done.output.nextCursor);assert(!('chunks' in textResult.output));
  const none=(await tool('sodeck_task_get',{taskId:start.taskId,outputMode:'none'})).value;assert(!('output' in none));assert.equal(none.outputInfo.savedBytes,done.output.savedBytes);
  assert.equal((await tool('sodeck_task_get',{taskId:start.taskId,waitMs:20000})).value.wait.reason,'done');
  const cancelDone=(await tool('sodeck_task_cancel',{taskId:start.taskId})).value;assert.equal(cancelDone.cancelRequested,false);assert.deepEqual(cancelDone.result,none,'terminal cancellation does not change any facts');
  for(let i=0;i<30;i++){const before=sequence,began=performance.now();const result=await tool('sodeck_exec_start',{...input,requestId:'short-'+i});assert.equal(result.error,false);assert.equal(result.value.done,true);assert.equal(result.value.wait.reason,'done');assert.equal(result.value.output.stdout,bytes.toString());assert.equal(result.value.output.hasMore,false);assert.equal(result.value.evidence.complete,true);assert.equal(result.value.resourceReleased,true);assert.equal(sequence-before,1);shortRuns.push({calls:sequence-before,elapsedMs:performance.now()-began,wait:result.value.wait,stdout:result.value.output.stdout});}
  for(const waitMs of [-1,0.5,20001,'1000',null,true]){const before=requests.length;assert.equal((await tool('sodeck_exec_start',{...input,requestId:'invalid-wait',waitMs})).value.error.code,'invalidParams');assert.equal(requests.length,before);}
  const invalidDownload=await call('tools/call',{name:'sodeck_file_download',arguments:{targetId:target,requestId:'invalid-download',remotePath:'/tmp/file',waitMs:0}});assert.equal(invalidDownload.error.code,-32602);
  const beforeDuplicate=requests.length,dupeInput={...input,requestId:'concurrent'};const duplicate=await Promise.all([tool('sodeck_exec_start',{...dupeInput,waitMs:0}),tool('sodeck_exec_start',{...dupeInput,waitMs:5000})]);assert.equal(duplicate[0].value.taskId,duplicate[1].value.taskId);assert.equal(duplicate[1].value.done,true);assert.equal(requests.length-beforeDuplicate,1);const dupeDetail=(await tool('sodeck_task_get',{taskId:duplicate[1].value.taskId,detail:true,outputMode:'none'})).value;assert(!('waitMs' in dupeDetail.input));assert(!('waitMs' in JSON.parse(fs.readFileSync(path.join(dupeDetail.directory,'metadata.json'))).input));
  const abortStart=new AbortController(),lostInput={...input,requestId:'lost-start',command:'quiet',waitMs:20000};const lostResponse=tool('sodeck_exec_start',lostInput,abortStart.signal).catch(e=>e);const admitted=await until(async()=>(await tool('sodeck_tasks_list',{targetId:target,requestId:'lost-start'})).value.tasks[0],Boolean);await until(async()=>requests.some(p=>p.operationId===admitted.taskId),Boolean);abortStart.abort();await lostResponse;await delay(120);const recovered=(await tool('sodeck_exec_start',{...lostInput,waitMs:0})).value;assert.equal(recovered.taskId,admitted.taskId);assert.equal(recovered.done,false);assert(!('wait' in recovered));assert.equal(requests.filter(p=>p.operationId===recovered.taskId).length,1);await tool('sodeck_task_cancel',{taskId:recovered.taskId});await until(async()=>(await tool('sodeck_task_get',{taskId:recovered.taskId,outputMode:'none'})).value,s=>s.done);
  const changingAt=performance.now(),changing=(await tool('sodeck_exec_start',{...input,requestId:'changing',command:'changing'})).value;assert.equal(changing.done,false);assert.equal(changing.wait.reason,'timeout');assert(changing.wait.elapsedMs>=950);assert(performance.now()-changingAt<2200);assert(changing.output.stdout.includes('line-'));assert(BigInt(changing.revision)>2n);await tool('sodeck_task_cancel',{taskId:changing.taskId});await until(async()=>(await tool('sodeck_task_get',{taskId:changing.taskId,outputMode:'none'})).value,s=>s.done);
  const pages=(await tool('sodeck_exec_start',{...input,requestId:'pages',command:'pages'})).value;assert.equal(pages.done,true);assert.equal(pages.output.hasMore,true);const page2=(await tool('sodeck_task_get',{taskId:pages.taskId,cursor:pages.output.nextCursor})).value;assert.equal(page2.output.hasMore,false);assert.equal(pages.output.stdout+page2.output.stdout,'1'.repeat(9000)+'2'.repeat(9000));
  const quiet=(await tool('sodeck_exec_start',{targetId:target,requestId:'quiet',command:'quiet'})).value;
  const quietResult=await until(async()=>(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none'})).value,s=>s.native.state==='running');
  const timed=(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:180})).value;assert.equal(timed.wait.reason,'timeout');assert(timed.wait.elapsedMs>=160);assert.equal(timed.revision,quietResult.revision,'clock-only polling does not change revision');
  for(const p of [{waitMs:-1},{waitMs:0.5},{waitMs:20001},{outputMode:'bad'},{detail:'true'},{detail:1},{sinceRevision:'999999999'},{outputMode:'none',cursor:'1'}])assert.equal((await tool('sodeck_task_get',{taskId:quiet.taskId,...p})).error,true);
  const controllers=[new AbortController(),new AbortController()];
  const readers=controllers.map(c=>tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000},c.signal).catch(e=>e));
  await delay(120);const overflow=await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000});assert.equal(overflow.value.error.code,'resourceBusy');
  const retryBusy=await tool('sodeck_exec_start',{targetId:target,requestId:'quiet',command:'quiet',waitMs:5000});assert.equal(retryBusy.value.error.code,'resourceBusy');assert.equal(retryBusy.value.task.taskId,quiet.taskId);
  const abortAt=Date.now();controllers.forEach(c=>c.abort());await Promise.all(readers);
  await delay(120);const afterAbort=(await tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:50})).value;assert.equal(afterAbort.wait.reason,'timeout');assert(Date.now()-abortAt<1000,'HTTP disconnect releases waiting quotas within one second');assert.equal(afterAbort.done,false);
  const waking=tool('sodeck_task_get',{taskId:quiet.taskId,outputMode:'none',waitMs:20000,sinceRevision:quietResult.revision});await delay(80);await tool('sodeck_task_cancel',{taskId:quiet.taskId});const woke=(await waking).value;assert(['changed','done'].includes(woke.wait.reason));assert(BigInt(woke.revision)>BigInt(quietResult.revision));await until(async()=>(await tool('sodeck_task_get',{taskId:quiet.taskId})).value,s=>s.done);
  const quotaTasks=[];for(let i=0;i<16;i++){const job=(await tool('sodeck_exec_start',{...input,requestId:'quota-'+i,command:'quiet',waitMs:0})).value;assert(!('wait' in job));await until(async()=>(await tool('sodeck_task_get',{taskId:job.taskId,outputMode:'none'})).value,s=>s.native.state==='running');quotaTasks.push(job);}
  const globalControllers=Array.from({length:16},()=>new AbortController());const globalReaders=globalControllers.map((c,i)=>tool('sodeck_task_get',{taskId:quotaTasks[Math.floor(i/2)].taskId,outputMode:'none',waitMs:20000},c.signal).catch(e=>e));await delay(150);
  assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none',waitMs:20000})).value.error.code,'resourceBusy');assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none'})).value.done,false,'short queries still work when waits are full');
  const countBeforeBusy=(await tool('sodeck_tasks_list',{targetId:target})).value.counts,filesBeforeBusy=fs.readdirSync(path.dirname(diagnostic.directory)),requestsBeforeBusy=requests.length;
  const newBusy=await tool('sodeck_exec_start',{...input,requestId:'wait-full',command:'must-not-run'});assert.equal(newBusy.value.error.code,'resourceBusy');assert(!('task' in newBusy.value));assert.deepEqual((await tool('sodeck_tasks_list',{targetId:target})).value.counts,countBeforeBusy);assert.deepEqual(fs.readdirSync(path.dirname(diagnostic.directory)),filesBeforeBusy);assert.equal(requests.length,requestsBeforeBusy);
  assert.equal((await tool('sodeck_exec_start',{...input,waitMs:20000})).value.done,true,'finished duplicate bypasses full waiting pool');
  const zero=(await tool('sodeck_exec_start',{...input,requestId:'zero-wait',waitMs:0})).value;assert(zero.taskId);assert(!('wait' in zero));await until(async()=>(await tool('sodeck_task_get',{taskId:zero.taskId,outputMode:'none'})).value,s=>s.done);
  globalControllers.forEach(c=>c.abort());await Promise.all(globalReaders);await delay(120);assert.equal((await tool('sodeck_task_get',{taskId:quotaTasks[8].taskId,outputMode:'none',waitMs:30})).value.wait.reason,'timeout');
  for(let i=16;i<24;i++){const job=(await tool('sodeck_exec_start',{...input,requestId:'quota-'+i,command:'quiet',waitMs:0})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:job.taskId,outputMode:'none'})).value,s=>s.native.state==='running');quotaTasks.push(job);}
  assert.equal((await tool('sodeck_exec_start',{...input,requestId:'over-active',command:'must-not-run'})).value.error.code,'resourceBusy');
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
  await peer.request('ui.createView',sidebar);assert.equal((await tool('sodeck_targets_list')).value.targets.length,3);assert.equal((await tool('sodeck_task_get',{taskId:hang.taskId})).value.done,false,'remounting the existing sidebar keeps background operations');
  assert((await peer.request('ui.getView',sidebar)).data.tasks.some(task=>task.taskId===hang.taskId),'the sidebar sees the same operation');
  for(let i=0;i<51;i++){
    const recent=(await tool('sodeck_exec_start',{...input,requestId:'recent-'+i,command:'collect'})).value;
    await until(async()=>(await tool('sodeck_task_get',{taskId:recent.taskId})).value,s=>s.done);
  }
  const history=(await peer.request('ui.getView',{viewId:'a'})).data.tasks;
  assert.equal(history.length,50);assert.equal(history[0].taskId,hang.taskId,'active operations remain visible beyond the recent record limit');
  assert(!('input' in history[0])&&!('directory' in history[0]));assert((await peer.request('ui.getView',{viewId:'a'})).data.recordCount>50);
  const firstPage=(await tool('sodeck_tasks_list',{targetId:target,limit:20})).value;assert.equal(firstPage.tasks.length,20);assert(firstPage.hasMore);assert.equal(firstPage.counts.active,1);for(const row of firstPage.tasks){assert.equal(row.serverId,'server-a');for(const k of ['input','context','directory','output','outputInfo','file'])assert(!(k in row));assert(row.createdAt>0);}
  const inserted=(await tool('sodeck_exec_start',{...input,requestId:'inserted',command:'collect'})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:inserted.taskId})).value,s=>s.done);
  let next=firstPage,all=[...next.tasks];while(next.hasMore){next=(await tool('sodeck_tasks_list',{targetId:target,limit:20,cursor:next.nextCursor})).value;all.push(...next.tasks);}
  assert.equal(new Set(all.map(t=>t.taskId)).size,all.length);assert.equal(all.length,firstPage.counts.active+firstPage.counts.done);assert(!all.some(t=>t.taskId===inserted.taskId),'new records do not enter an existing pagination ceiling');
  assert.equal((await tool('sodeck_tasks_list',{targetId:target,limit:21,cursor:firstPage.nextCursor})).value.error.code,'invalidCursor');
  const foreign=JSON.parse(Buffer.from(firstPage.nextCursor,'base64'));foreign.run='old-run';assert.equal((await tool('sodeck_tasks_list',{targetId:target,limit:20,cursor:Buffer.from(JSON.stringify(foreign)).toString('base64')})).value.error.code,'runMismatch');
  const filtered=(await tool('sodeck_tasks_list',{serverId:'server-a',requestId:'quiet',kind:'exec',status:'done'})).value;assert.equal(filtered.tasks.length,1);assert.equal(filtered.tasks[0].taskId,quiet.taskId);assert.equal(filtered.counts.done,1);
  const uiCancel=(await peer.request('ui.event',{viewId:'a',data:{action:'cancel',taskId:hang.taskId}})).data;assert.equal(uiCancel.cancelRequested,true);assert.equal(uiCancel.result.taskId,hang.taskId);assert(!('input' in uiCancel.result));done=await until(async()=>(await tool('sodeck_task_get',{taskId:hang.taskId})).value,s=>s.done);assert.equal(done.native.state,'cancelled');assert.equal(done.evidence.complete,false);
  const nonzero=(await tool('sodeck_exec_start',{...input,requestId:'exit',command:'exit 7'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:nonzero.taskId})).value,s=>s.done);assert.equal(done.native.exitCode,7);assert.equal(done.evidence.complete,true);
  const delayed=(await tool('sodeck_exec_start',{...input,requestId:'delayed',command:'delayed',waitMs:0})).value;
  await until(async()=>requests.some(p=>p.command==='delayed'),Boolean);
  const busy=await tool('sodeck_exec_start',{...input,requestId:'queued',command:'must-not-run'});assert.equal(busy.error,true);assert.equal(busy.value.error.code,'resourceBusy');assert(!busy.value.taskId);assert(!requests.some(p=>p.command==='must-not-run'));assert.equal((await tool('sodeck_exec_start',{...input,requestId:'delayed',command:'delayed'})).value.taskId,delayed.taskId);await until(async()=>(await tool('sodeck_task_get',{taskId:delayed.taskId})).value,s=>s.done);
  const download=(await tool('sodeck_file_download',{targetId:target,requestId:'download',remotePath:'/tmp/consul.tar.gz'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:download.taskId})).value,s=>s.done);assert.equal(done.evidence.complete,true);assert.deepEqual(fs.readFileSync(done.file.localPath),bytes);assert.equal(files.size,0);assert.equal(batches.size,0);
  assert.equal(done.file.remotePath,'/tmp/consul.tar.gz');assert(!('output' in done));
  const downloadedDetail=(await tool('sodeck_task_get',{taskId:download.taskId,detail:true,outputMode:'none'})).value;
  const savedMetadata=JSON.parse(fs.readFileSync(path.join(downloadedDetail.directory,'metadata.json'),'utf8'));assert.equal(savedMetadata.target.username,'ops');assert.equal(savedMetadata.schemaVersion,1);assert.deepEqual(savedMetadata.input,downloadedDetail.input);assert.deepEqual(savedMetadata.context,contexts.a);assert.equal(savedMetadata.evidence.path,done.file.localPath);
  for(const command of ['malformed','output limit']){const task=(await tool('sodeck_exec_start',{...input,requestId:command,command})).value;const result=await until(async()=>(await tool('sodeck_task_get',{taskId:task.taskId})).value,s=>s.done);assert.equal(result.evidence.complete,false);assert.equal(result.resourceReleased,true);}
  const missing=(await tool('sodeck_file_download',{targetId:target,requestId:'missing',remotePath:'/missing'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:missing.taskId})).value,s=>s.done);assert.equal(done.native.admission,'rejected');assert.equal(done.file.localPath,null);assert.equal(batches.size,0,'known rejection ends the batch');
  const cleanup=(await tool('sodeck_file_download',{targetId:target,requestId:'cleanup',remotePath:'/cleanup-failure'})).value;const cleaned=await until(async()=>(await tool('sodeck_task_get',{taskId:cleanup.taskId})).value,s=>s.done);assert.equal(cleaned.evidence.complete,true);assert.equal(cleaned.resourceReleased,false);assert.equal(cleaned.cleanupError.detailsAvailable,true);assert(!('details' in cleaned.cleanupError));const cleanupDetail=(await tool('sodeck_task_get',{taskId:cleanup.taskId,detail:true,outputMode:'none'})).value;assert.deepEqual(cleanupDetail.cleanupError.details.map(e=>e.step),['release','batch.end']);
  const failing=(await tool('sodeck_exec_start',{...input,requestId:'final-save-fail',command:'final-save-fail',waitMs:0})).value;await until(async()=>typeof releaseGate==='function',Boolean);
  const finalizing=(await tool('sodeck_task_get',{taskId:failing.taskId,detail:true,outputMode:'none'})).value;assert.equal(finalizing.phase,'finalizing');assert.equal(finalizing.done,false);assert.equal(finalizing.evidence.complete,false);
  assert.equal((await tool('sodeck_task_cancel',{taskId:failing.taskId})).value.cancelRequested,false);
  fs.mkdirSync(path.join(finalizing.directory,'metadata.tmp'));releaseGate({released:true});const failed=await until(async()=>(await tool('sodeck_task_get',{taskId:failing.taskId,outputMode:'none'})).value,s=>s.done);assert.equal(failed.evidence.state,'failed');assert.equal(failed.evidence.complete,false);assert.equal(failed.resourceReleased,true);
  fs.rmdirSync(path.join(finalizing.directory,'metadata.tmp'));assert.deepEqual((await tool('sodeck_task_get',{taskId:failing.taskId,outputMode:'none'})).value,failed);assert.equal(JSON.parse(fs.readFileSync(path.join(finalizing.directory,'metadata.json'))).phase,'finalizing','failed final save leaves last valid disk facts');
  const huge=(await tool('sodeck_exec_start',{...input,requestId:'large-error',command:'large-error '+ '\\'.repeat(16000)})).value;
  const hugeResult=await until(async()=>(await tool('sodeck_task_get',{taskId:huge.taskId})).value,s=>s.done);assert.equal(hugeResult.native.error.detailsAvailable,true);assert.equal(hugeResult.native.error.code,-32000);
  const hugeDetail=await tool('sodeck_task_get',{taskId:huge.taskId,detail:true,outputMode:'none'});assert.equal(hugeDetail.error,true);assert.equal(hugeDetail.value.error.code,'resourceLimit');assert.equal(hugeDetail.value.error.taskId,huge.taskId);
  const hugeAgain=(await tool('sodeck_task_get',{taskId:huge.taskId,outputMode:'none'})).value;assert.equal(hugeAgain.revision,hugeResult.revision);assert.equal(hugeAgain.done,true);assert.equal(hugeAgain.native.error.code,-32000);
  const unknown=(await tool('sodeck_exec_start',{...input,requestId:'unknown',command:'unknown'})).value;await until(async()=>(await tool('sodeck_task_get',{taskId:unknown.taskId,outputMode:'none'})).value,s=>s.native.admission==='unknown');
  await tool('sodeck_task_cancel',{taskId:unknown.taskId});assert.equal((await tool('sodeck_exec_start',{...input,requestId:'unknown',command:'unknown'})).value.taskId,unknown.taskId);assert.equal((await tool('sodeck_exec_start',{...input,requestId:'must-not-replay',command:'collect'})).value.error.code,'resourceBusy');assert.equal(requests.filter(p=>p.command==='unknown').length,1);confirmUnknown=true;await until(async()=>(await tool('sodeck_task_get',{taskId:unknown.taskId})).value,s=>s.done);
  for(const headers of [{Authorization:'Bearer wrong'},{Origin:'https://example.com'},{Host:'example.com'}]){
    const status=await new Promise((resolve,reject)=>{const request=http.request({hostname:'127.0.0.1',port,path:'/mcp',method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream',...headers}},response=>{response.resume();response.on('end',()=>resolve(response.statusCode));});request.on('error',reject);request.end(JSON.stringify({jsonrpc:'2.0',id:99,method:'tools/list',params:{}}));});
    assert([401,403].includes(status),JSON.stringify({headers:Object.keys(headers),status}));
  }
  contexts.a={...contexts.a,generation:2};assert.equal((await tool('sodeck_exec_start',{...input,requestId:'stale'})).value.taskId.startsWith('task-'),true);
  assert.equal((await peer.request('ui.getView',{viewId:'a'})).data.target.state,'unavailable','the sidebar does not present another connection generation as its recorded target');
  const countBefore=requests.length;await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.tasks.some(t=>t.requestId==='stale'&&t.done));assert.equal(requests.length,countBefore,'stale target must not execute');
  for(const viewId of Object.keys(contexts))await peer.request('ui.disposeView',{viewId});
  await peer.close();peer=null;await assert.rejects(fetch(`http://127.0.0.1:${port}/mcp`));
  if(process.env.SODECK_TEST_MCP_REPORT)fs.writeFileSync(process.env.SODECK_TEST_MCP_REPORT,JSON.stringify({version:manifest.version,fixture:'representative lscpu output, not a live server',stdout:bytes.toString(),samples,shortRuns},null,2)+'\n');
  console.log('MCP: six tools, scopes, stable finalization, text/none, keyset paging, wait timeout/wake/disconnect quotas, unknown admission, cleanup failures and exit passed');
}finally{if(peer)await peer.kill();assert.equal(path.dirname(directory),os.tmpdir());fs.rmSync(directory,{recursive:true,force:true});}
