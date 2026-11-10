import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createDraftSync} from '../plugins/files/ui/draft-sync.js';

function fixture() {
 let text='old',coreRevision=0,receipt,upload,fail,hold;
 const calls=[];
 const data=async(action,p)=>{
  calls.push(action);
  if(fail===action){fail=undefined;throw new Error('lost reply');}
  if(action==='draft.begin'){
   if(receipt?.uploadId===p.uploadId)return receipt;
   upload={...p,bytes:[]};return {state:'pending',nextOffset:0};
  }
  if(action==='draft.append'){upload.bytes.push(...p.data);return {};}
  if(action==='draft.commit'){
   if(hold)await hold;
   receipt={...upload,state:'committed',draftRef:{documentId:'doc',revision:String(++coreRevision)}};
   if(fail==='reply'){fail=undefined;throw new Error('lost commit reply');}
   return receipt;
  }
  if(action==='draft.status')return receipt?.uploadId===p.uploadId?receipt:{state:'pending'};
  throw new Error(action);
 };
 const sync=createDraftSync({data,read:()=>new TextEncoder().encode(text),info:{draftRef:{documentId:'doc',revision:'0'}},error:()=>{}});
 return {sync,calls,edit(value){text=value;sync.change();},fail(value){fail=value;},hold(p){hold=p;},receipt:()=>receipt};
}
test('failed upload recovers without further input; lost commit reply does not duplicate revision',async()=>{
 for(const failure of ['draft.begin','draft.append','reply']){
  const f=fixture();f.fail(failure);f.edit('new');
  const ref=await f.sync.flush();
  assert.equal(ref.revision,'1');assert.equal(f.sync.state().committedRevision,1);
  assert.equal(new TextDecoder().decode(Uint8Array.from(f.receipt().bytes)),'new');
 }
});
test('explicit action captures R while later input remains dirty until another upload',async()=>{
 const f=fixture();let resolve;f.hold(new Promise(r=>resolve=r));f.edit('R');
 let accepted;const action=f.sync.action(async ref=>{accepted=ref;assert.equal(f.sync.state().revision,2);assert.equal(f.sync.state().committedRevision,1);});
 f.edit('R+1');resolve();await action;
 assert.equal(accepted.revision,'1');await f.sync.flush();
 assert.equal(f.sync.state().committedRevision,2);
 assert.equal(new TextDecoder().decode(Uint8Array.from(f.receipt().bytes)),'R+1');
});
test('metadata errors cannot poison the upload scheduler',async()=>{
 const f=fixture();f.edit('\uFEFFa\r\nb');await f.sync.action(async()=>{throw new Error('metadata');}).catch(()=>{});
 const ref=await f.sync.flush();assert.equal(ref.revision,'1');
 assert.deepEqual(f.receipt().bytes,Array.from(new TextEncoder().encode('\uFEFFa\r\nb')));
});
