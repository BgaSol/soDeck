import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readDocument} from '../packages/file-editor/src/read-document.js';

test('file editor restarts a snapshot when a staged draft changes its size and revision during reading',async()=>{
  let changed=false;
  const data=async action=>{
    if(action==='describe')return {path:'/file',size:changed?4:3,revision:changed?1:0};
    if(action==='chunk'){changed=true;return {size:4,revision:1,data:[4,5,6,7]};}
    throw new Error(action);
  };
  const result=await readDocument(data,'/file');
  assert.equal(result.info.revision,1);assert.deepEqual([...result.bytes],[4,5,6,7]);
});

test('file editor never combines chunks from two revisions with equal lengths',async()=>{
  let changed=false,reads=0;
  const data=async(action,args)=>{
    if(action==='describe'){reads++;return {path:'/file',size:16385,revision:changed?1:0};}
    if(args.offset===16384)changed=true;
    return {size:16385,revision:changed?1:0,data:Array(Math.min(16384,16385-args.offset)).fill(changed?9:2)};
  };
  const {bytes}=await readDocument(data,'/file');assert.equal(reads,2);assert(bytes.every(v=>v===9));
});

test('file editor stops after repeated revision changes instead of looping indefinitely',async()=>{
  let revision=0;
  await assert.rejects(readDocument(async action=>action==='describe'
    ?{path:'/file',size:1,revision}
    :{size:1,revision:++revision,data:[1]},'/file'),/持续变化/);
  assert.equal(revision,3);
});

test('file editor rejects incomplete chunks with a useful error',async()=>{
  await assert.rejects(readDocument(async action=>action==='describe'
    ?{path:'/file',size:2,revision:0}
    :{size:2,revision:0,data:[1]},'/file'),/数据块不完整/);
});
