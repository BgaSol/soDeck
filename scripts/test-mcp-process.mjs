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
const streams=new Map(),requests=[],releases=[],files=new Map(),batches=new Set();let number=0,lost=true,peer;
const bytes=Buffer.from('中文 investigation\n'),chunk={v:1,sequence:'1',source:'stdout',receivedAt:Date.now(),dataBase64:bytes.toString('base64')};
peer=pluginPeer(path.join(root,manifest.entry),(method,p)=>{
  if(method==='settings.get')return {values:{port:String(port)}};
  if(['logging.write','messages.publish'].includes(method))return {};
  if(method==='session.state')return {context:contexts[p.viewId],state:'connected',closing:false,target:{name:p.viewId,protocol:'ssh',endpoint:p.viewId+':22',username:'ops'}};
  if(method==='session.stream.open'){
    requests.push(p);const streamId='stream-'+(++number);streams.set(streamId,{p,held:p.command==='hang',cancelled:false});
    if(lost){lost=false;throw Object.assign(new Error('lost receipt'),{code:-32003});}if(p.command==='delayed')return delay(300).then(()=>({streamId,state:'opening'}));return {streamId,state:'opening'};
  }
  if(method==='session.stream.lookup'){const entry=[...streams].find(([,s])=>s.p.operationId===p.operationId);if(!entry)return {known:false};assert.deepEqual(entry[1].p,p);return {known:true,streamId:entry[0]};}
  if(method==='session.stream.poll'){
    const s=streams.get(p.streamId);assert(s);const done=!s.held||s.cancelled;
    return {chunks:s.p.command==='malformed'?null:p.offset==='0'?[chunk]:[],nextOffset:'100',watermark:'100',status:{state:s.cancelled?'cancelled':done?'completed':'running',execSent:true,remoteExitConfirmed:done&&!s.cancelled,workerFinished:done,finished:done,outputComplete:done&&!s.cancelled&&s.p.command!=='output limit',error:s.p.command==='output limit'?{code:'outputLimit',message:'output limit'}:null,exitCode:s.p.command==='exit 7'?7:0}};
  }
  if(method==='session.stream.close'){streams.get(p.streamId).cancelled=true;return {accepted:true};}
  if(method==='session.stream.release'){releases.push(p.streamId);streams.delete(p.streamId);return {released:true};}
  if(method==='files.request'){if(p.action==='open')return {id:'file-'+p.viewId};if(p.action==='close')return {};if(p.action==='batch.begin'){batches.add(p.batchId);return {};}if(p.action==='batch.end'){batches.delete(p.batchId);return {};}}
  if(method==='files.operations'){
    if(p.action==='start'){
      if(p.operation.source.path==='/missing')throw Object.assign(new Error('missing'),{code:-32000});
      assert.equal(p.operation.destination.side,'pluginData');assert.equal(p.operation.destinationGuard.kind,'absent');
      const id='download-'+(++number);const output=path.join(directory,...p.operation.destination.relative);fs.writeFileSync(output,bytes);files.set(id,p);return {id,state:'running'};
    }
    if(p.action==='poll')return {id:p.operationId,state:'completed',result:{size:String(bytes.length)}};
    if(p.action==='release'){files.delete(p.operationId);return {};}
    if(p.action==='cancel')return {};
  }
  throw new Error('Unexpected RPC: '+method);
});
let token,sequence=0;
async function call(method,params,headers={}){
  const response=await fetch(`http://127.0.0.1:${port}/mcp`,{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25',...headers},body:JSON.stringify({jsonrpc:'2.0',id:++sequence,method,params})});
  assert.equal(response.status,200,await response.clone().text());return (await response.json()).result;
}
async function tool(name,arguments_={}){const result=await call('tools/call',{name,arguments:arguments_});return {value:result.structuredContent??JSON.parse(result.content[0].text),error:result.isError};}
async function until(read,predicate){const end=Date.now()+10000;while(Date.now()<end){const value=await read();if(predicate(value))return value;await delay(30);}throw new Error('MCP task timeout');}
try{
  await assert.rejects(peer.request('initialize',{dataDirectory:directory,capabilities:[]}),/核心缺少/);
  await peer.request('initialize',{dataDirectory:directory,capabilities});token=fs.readFileSync(path.join(directory,'access-token'),'utf8');
  for(const [viewId,context]of Object.entries(contexts))await peer.request('ui.createView',{viewId,viewType:'status',context});
  await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.service==='ready');
  const initialized=await call('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'sodeck-test',version:'1'}});assert(initialized.capabilities.tools);
  const list=await call('tools/list',{});assert.equal(list.tools.length,5);assert.equal(list.tools.find(t=>t.name==='sodeck_exec_start').annotations.readOnlyHint,false);
  const targets=(await tool('sodeck_targets_list')).value.targets;assert.equal(targets.length,3);
  const target=targets.find(t=>t.context.workspaceId==='a').targetId;
  const input={targetId:target,requestId:'once',command:'collect'};
  const start=(await tool('sodeck_exec_start',input)).value;
  assert.equal((await tool('sodeck_exec_start',input)).value.taskId,start.taskId);
  assert.equal((await tool('sodeck_exec_start',{...input,command:'different'})).error,true);
  let done=await until(async()=>(await tool('sodeck_task_get',{taskId:start.taskId})).value,s=>s.done);
  assert.equal(done.evidence.complete,true);assert.equal(done.resourceReleased,true);assert.equal(done.output.stdout,bytes.toString());assert.equal(requests.length,1);
  assert.deepEqual((await tool('sodeck_task_get',{taskId:start.taskId})).value,done,'released task evidence remains queryable');
  assert.equal((await tool('sodeck_task_get',{taskId:start.taskId,cursor:'1'})).error,true);
  const hang=(await tool('sodeck_exec_start',{...input,requestId:'hang',command:'hang'})).value;
  await until(async()=>(await tool('sodeck_task_get',{taskId:hang.taskId})).value,s=>s.native.state==='running');
  await tool('sodeck_task_cancel',{taskId:hang.taskId});done=await until(async()=>(await tool('sodeck_task_get',{taskId:hang.taskId})).value,s=>s.done);assert.equal(done.native.state,'cancelled');assert.equal(done.evidence.complete,false);
  const nonzero=(await tool('sodeck_exec_start',{...input,requestId:'exit',command:'exit 7'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:nonzero.taskId})).value,s=>s.done);assert.equal(done.native.exitCode,7);assert.equal(done.evidence.complete,true);
  const delayed=(await tool('sodeck_exec_start',{...input,requestId:'delayed',command:'delayed'})).value;
  await until(async()=>requests.some(p=>p.command==='delayed'),Boolean);
  const queued=(await tool('sodeck_exec_start',{...input,requestId:'queued',command:'must-not-run'})).value;await tool('sodeck_task_cancel',{taskId:queued.taskId});done=await until(async()=>(await tool('sodeck_task_get',{taskId:queued.taskId})).value,s=>s.done);assert.equal(done.native.execSent,false);assert(!requests.some(p=>p.command==='must-not-run'));await until(async()=>(await tool('sodeck_task_get',{taskId:delayed.taskId})).value,s=>s.done);
  const download=(await tool('sodeck_file_download',{targetId:target,requestId:'download',remotePath:'/tmp/consul.tar.gz'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:download.taskId})).value,s=>s.done);assert.equal(done.evidence.complete,true);assert.deepEqual(fs.readFileSync(done.evidence.path),bytes);assert.equal(files.size,0);assert.equal(batches.size,0);
  assert.equal(JSON.parse(fs.readFileSync(path.join(done.directory,'metadata.json'),'utf8')).target.username,'ops');
  for(const command of ['malformed','output limit']){const task=(await tool('sodeck_exec_start',{...input,requestId:command,command})).value;const result=await until(async()=>(await tool('sodeck_task_get',{taskId:task.taskId})).value,s=>s.done);assert.equal(result.evidence.complete,false);assert.equal(result.resourceReleased,true);}
  const missing=(await tool('sodeck_file_download',{targetId:target,requestId:'missing',remotePath:'/missing'})).value;done=await until(async()=>(await tool('sodeck_task_get',{taskId:missing.taskId})).value,s=>s.done);assert.equal(done.native.admission,'rejected');assert.equal(batches.size,0,'known rejection ends the batch');
  for(const headers of [{Authorization:'Bearer wrong'},{Origin:'https://example.com'},{Host:'example.com'}]){
    const status=await new Promise((resolve,reject)=>{const request=http.request({hostname:'127.0.0.1',port,path:'/mcp',method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Accept:'application/json, text/event-stream',...headers}},response=>{response.resume();response.on('end',()=>resolve(response.statusCode));});request.on('error',reject);request.end(JSON.stringify({jsonrpc:'2.0',id:99,method:'tools/list',params:{}}));});
    assert([401,403].includes(status),JSON.stringify({headers:Object.keys(headers),status}));
  }
  contexts.a={...contexts.a,generation:2};assert.equal((await tool('sodeck_exec_start',{...input,requestId:'stale'})).value.taskId.startsWith('task-'),true);
  const countBefore=requests.length;await until(async()=>(await peer.request('ui.getView',{viewId:'a'})).data,s=>s.tasks.some(t=>t.requestId==='stale'&&t.done));assert.equal(requests.length,countBefore,'stale target must not execute');
  for(const viewId of Object.keys(contexts))await peer.request('ui.disposeView',{viewId});
  await peer.close();peer=null;await assert.rejects(fetch(`http://127.0.0.1:${port}/mcp`));
  console.log('MCP: five tools, three targets, receipt recovery, dedupe, bytes, cancel, download, HTTP checks and exit passed');
}finally{if(peer)await peer.kill();assert.equal(path.dirname(directory),os.tmpdir());fs.rmSync(directory,{recursive:true,force:true});}
