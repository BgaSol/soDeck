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
let number = 0, losePoll = true, loseRelease = true;
const peer = pluginPeer(executable, async (method, p) => {
  if (method === 'logging.write' || method === 'messages.publish') return {};
  if (method !== 'files.request' && method !== 'files.operations') throw new Error('Unexpected call: '+method);
  const action = method === 'files.operations' ? 'op.'+p.action : p.action;
  if (action === 'open') return {id:'files'};
  if (action.startsWith('batch.')) return {};
  if (action === 'op.start') {
    const id = String(++number);
    const mutation = p.operation.kind === 'createEmpty';
    if (mutation) mutations.push(p.requestId);
    const result = mutation ? {done:true} : {attrs:null};
    operations.set(id,{id,state:'completed',result,requestId:p.requestId,mutation});
    return {id,state:'running'};
  }
  if (action === 'op.poll' || action === 'op.lookup') {
    const op = action === 'op.lookup' ? [...operations.values()].find(v=>v.requestId===p.requestId) : operations.get(p.operationId);
    assert(op, 'Original operation must still be available');
    if (op.mutation && losePoll) throw Object.assign(new Error('Simulated connection interruption'),{code:-32003});
    return op;
  }
  if (action === 'op.release') {
    const op=operations.get(p.operationId);operations.delete(p.operationId);
    if (op?.mutation&&loseRelease) { loseRelease=false;throw Object.assign(new Error('Release reply lost'),{code:-32003}); }
    if (!op) throw Object.assign(new Error('Already released'),{code:-32003,data:{code:'expired'}});
    return {};
  }
  if (action === 'op.cancel') return {};
  throw new Error('Unexpected files action: '+action);
});
const request = (method, data={}) => peer.request(method,{viewId:'browser',viewType:'files',context,...data});
const event = data => request('ui.event',{data}).then(r=>r.data);
try {
  await peer.request('initialize',{dataDirectory:directory,capabilities:['files.v1','files.directoryCursors.v1','files.operations.v1','files.operations.v2','files.localInputs.v1','files.editor.v1','viewData.v1','ui.workbench.v2','ui.openView.v1']});
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
  console.log('Files process: lost mutation receipt, confirmation without replay, and unsupported moves passed.');
  await peer.close();
} finally {
  if (peer.child.exitCode === null) { peer.child.kill(); await new Promise(resolve=>peer.child.once('exit',resolve)); }
  assert.equal(path.dirname(directory),os.tmpdir());
  assert(path.basename(directory).startsWith('sodeck-files-process-'));
  fs.rmSync(directory,{recursive:true,force:true});
}
