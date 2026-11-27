// Real plugin process; deliberately lose observation replies after a mutation is accepted.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {pluginPeer, until} from './docker-process.mjs';

const executable = process.env.SODECK_TEST_FILES_EXECUTABLE;
if (!executable) throw new Error('Set SODECK_TEST_FILES_EXECUTABLE to the native files plugin executable');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-files-process-'));
const context = {workspaceId:'files-test',serverId:'server',connectionId:'connection',generation:1};
const operations = new Map(), mutations = [];
let number = 0, losePoll = true, loseRelease = true, copyOutcome='holding';
const copies=[];
const copySnapshot=op=>({id:op.id,state:copyOutcome==='holding'?'running':copyOutcome==='failed'?'failed':'completed',received:'20',total:'20',transfer:{phase:copyOutcome==='completed'?'finished':'finalizing',stage:copyOutcome==='completed'?'finished':'flush',commitState:copyOutcome==='completed'?'committed':copyOutcome==='failed'?'notCommitted':'pending',commitAttempted:copyOutcome==='completed'},...(copyOutcome==='failed'?{error:{code:'noSpace',message:'disk full',stage:'flush',osCode:28,effect:'none',retryable:true}}:{result:{size:'20'}})});
const peer = pluginPeer(executable, async (method, p) => {
  if (method === 'logging.write' || method === 'messages.publish') return {};
  if (!method.startsWith('files.')) throw new Error('Unexpected call: '+method);
  const action = method === 'files.operations' ? 'operation.'+p.action : method.slice('files.'.length);
  if (action === 'open') return {id:'files'};
  if (action.startsWith('batch.')) return {};
  if (action === 'operation.start') {
    const id = String(++number);
    const mutation = p.operation.kind === 'createEmpty';
    if (mutation) mutations.push(p.requestId);
    const copy=p.operation.kind==='copyFile';if(copy)copies.push(p.requestId);
    const result = mutation ? {done:true} : {attrs:p.operation.endpoint?.path==='/source'?{kind:'file',size:'20',stamp:'stable'}:null};
    operations.set(id,{id,state:'completed',result,requestId:p.requestId,mutation,copy});
    return copy?copySnapshot(operations.get(id)):{id,state:'running'};
  }
  if (action === 'operation.poll' || action === 'operation.lookup') {
    const op = action === 'operation.lookup' ? [...operations.values()].find(v=>v.requestId===p.requestId) : operations.get(p.operationId);
    assert(op, 'Original operation must still be available');
    if (op.mutation && losePoll) throw Object.assign(new Error('Simulated connection interruption'),{code:-32003,data:{dispatch:'unknown'}});
    return op.copy?copySnapshot(op):op;
  }
  if (action === 'operation.release') {
    const op=operations.get(p.operationId);operations.delete(p.operationId);
    if (op?.mutation&&loseRelease) { loseRelease=false;throw Object.assign(new Error('Release reply lost'),{code:-32003,data:{dispatch:'unknown'}}); }
    if (!op) throw Object.assign(new Error('Already released'),{code:-32003,data:{code:'expired'}});
    return {};
  }
  if (action === 'operation.cancel') return {};
  throw new Error('Unexpected files action: '+action);
});
const request = (method, data={}) => peer.request(method,{viewId:'browser',viewType:'files',context,...data});
const event = data => request('ui.event',{data}).then(r=>r.data);
try {
  await peer.initialize({dataDirectory:directory,});
  await request('ui.createView');
  const {batchId} = await event({action:'startBatch',parameters:{kind:'createEmpty',path:'/new.txt'}});
  const batch = await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===batchId&&['failed','partial','needsReview'].includes(t.state)));
  assert.equal(batch.tasks.find(t=>t.id===batchId).state,'needsReview','Lost mutation poll must not become a retryable failure');
  await assert.rejects(event({action:'retryFailed',batchId}),/确认|核对|未知/);
  await assert.rejects(event({action:'clearBatch',batchId}),/确认|核对|运行/);
  assert.equal(mutations.length,1);
  losePoll = false;
  await event({action:'confirmBatch',batchId});
  await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===batchId&&t.state==='needsReview'&&t.error?.includes('释放')));
  assert.equal(mutations.length,1);
  await event({action:'confirmBatch',batchId});
  const confirmed = await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===batchId&&t.state==='completed'));
  assert.equal(confirmed.tasks.find(t=>t.id===batchId).completed,'1');
  assert.equal(mutations.length,1,'Confirming a result must never replay its mutation');
  await assert.rejects(event({action:'startBatch',parameters:{kind:'move',roots:['/source'],targetPath:'/dest'}}),/无覆盖|不支持/);
  await assert.rejects(event({action:'startBatch',parameters:{kind:'rename',path:'/source',destination:'/dest'}}),/无覆盖|不支持/);
  const copying=await event({action:'startBatch',parameters:{kind:'copy',roots:['/source'],targetPath:'/dest'}});
  const full=await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===copying.batchId&&t.currentOperation?.received==='20'));
  const observed=full.tasks.find(t=>t.id===copying.batchId);assert.equal(observed.state,'running');assert.equal(observed.currentOperation.transfer.phase,'finalizing');assert.equal(observed.transferredBytes,'20');assert.equal(observed.completedBytes,'0');
  copyOutcome='failed';
  await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===copying.batchId&&t.state==='partial'));
  const journal=await event({action:'readItems',batchId:copying.batchId});assert.equal(journal.entries[0].failure.error.stage,'flush');assert.equal(journal.entries[0].failure.error.osCode,28);assert.equal(journal.entries[0].copy.transfer.commitState,'notCommitted');
  copyOutcome='completed';const retry=await event({action:'retryFailed',batchId:copying.batchId});
  const done=await until(()=>event({action:'readTasks'}),r=>r.tasks.some(t=>t.id===retry.batchId&&t.state==='completed'));const completed=done.tasks.find(t=>t.id===retry.batchId);assert.equal(completed.transferredBytes,'20');assert.equal(completed.completedBytes,'20');assert.equal(completed.currentOperation.transfer.phase,'finished');assert.equal(copies.length,2);
  console.log('Files process: lost mutation receipt, confirmation without replay, unsupported moves, full-byte flush failure, durable diagnostics and fresh retry meters passed.');
  await peer.close();
} finally {
  if (peer.child.exitCode === null) { peer.child.kill(); await new Promise(resolve=>peer.child.once('exit',resolve)); }
  assert.equal(path.dirname(directory),os.tmpdir());
  assert(path.basename(directory).startsWith('sodeck-files-process-'));
  fs.rmSync(directory,{recursive:true,force:true});
}
