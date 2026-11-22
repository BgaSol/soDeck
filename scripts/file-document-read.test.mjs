import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readDocument} from '../packages/file-editor/src/read-document.js';

test('file editor restarts a snapshot when a staged draft changes its size and revision during reading',async()=>{
  let changed=false;
  const data=async action=>{
    if(action==='draft.describe')return {path:'/file',size:changed?4:3,revision:changed?1:0,draftRef:{documentId:'doc',revision:changed?'1':'0'}};
    if(action==='draft.chunk'){changed=true;return {size:4,revision:1,draftRef:{documentId:'doc',revision:'1'},data:[4,5,6,7]};}
    throw new Error(action);
  };
  const result=await readDocument(data,'/file');
  assert.equal(result.info.revision,1);assert.deepEqual([...result.bytes],[4,5,6,7]);
});

test('file editor never combines chunks from two revisions with equal lengths',async()=>{
  let changed=false,reads=0;
  const data=async(action,args)=>{
    if(action==='draft.describe'){reads++;return {path:'/file',size:16385,revision:changed?1:0,draftRef:{documentId:'doc',revision:changed?'1':'0'}};}
    if(Number(args.offset)===16384)changed=true;
    return {size:16385,revision:changed?1:0,draftRef:{documentId:'doc',revision:changed?'1':'0'},data:Array(Math.min(16384,16385-args.offset)).fill(changed?9:2)};
  };
  const {bytes}=await readDocument(data,'/file');assert.equal(reads,2);assert(bytes.every(v=>v===9));
});

test('file editor stops after repeated revision changes instead of looping indefinitely',async()=>{
  let revision=0;
  await assert.rejects(readDocument(async action=>action==='draft.describe'
    ?{path:'/file',size:1,revision,draftRef:{documentId:'doc',revision:String(revision)}}
    :{size:1,revision:++revision,draftRef:{documentId:'doc',revision:String(revision)},data:[1]},'/file'),/持续变化/);
  assert.equal(revision,3);
});

test('file editor rejects incomplete chunks with a useful error',async()=>{
  await assert.rejects(readDocument(async action=>action==='draft.describe'
    ?{path:'/file',size:2,revision:0,draftRef:{documentId:'doc',revision:'0'}}
    :{size:2,revision:0,draftRef:{documentId:'doc',revision:'0'},data:[1]},'/file'),/数据块不完整/);
});

test('file editor restarts a revision conflict and keeps unrelated errors',async()=>{
  let revision=0;
  const {bytes}=await readDocument(async action=>{
    if(action==='draft.describe')return {path:'/file',size:1,draftRef:{documentId:'doc',revision:String(revision)}};
    if(revision++===0)throw Object.assign(new Error('changed'),{code:-32000,data:{code:'revisionConflict'}});
    return {draftRef:{documentId:'doc',revision:'1'},data:[9]};
  },'/file');
  assert.deepEqual([...bytes],[9]);
  const error=Object.assign(new Error('surface closed'),{data:{code:'staleContext'}});
  await assert.rejects(readDocument(async action=>{
    if(action==='draft.describe')return {path:'/file',size:1,draftRef:{documentId:'doc',revision:'1'}};
    throw error;
  },'/file'),e=>e===error);
});
