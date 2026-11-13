import {test} from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {pluginPeer} from './plugin-peer.mjs';

test('independent SDK consumer saves on close, retains newer input, and does not hold state locks during RPC',async()=>{
 execFileSync('cargo',['build','--manifest-path','crates/plugin-client/Cargo.toml','--example','file-editor-fixture','--locked'],{stdio:'pipe',windowsHide:true});
 const context={workspaceId:'sdk',serverId:'server',connectionId:'connection',generation:1};
 let saved='0',revision='1',input='1',state='idle',request,releaseQuery,queryEntered;
 const entered=new Promise(resolve=>queryEntered=resolve);let hold=false;
 const resources=new Set(),saves=[];
 const peer=pluginPeer(path.resolve('crates/plugin-client/target/debug/examples/file-editor-fixture'+(process.platform==='win32'?'.exe':'')),async(method,p)=>{
  if(method==='files.request'){if(p.action==='open'){resources.add(p.viewId);return {id:p.viewId};}if(p.action==='close'){resources.delete(p.id);return {};}}
  if(method==='ui.openView'){assert.equal(p.viewType,'configFile');assert.deepEqual(p.expectedContext,context);return {viewId:'config'};}
  if(method==='files.editor'){
   if(p.action==='save.start'){saves.push(p);request=p;state='committed';saved=p.expectedDraftRef.revision;}
   if(p.action==='save.release'){request=undefined;}
   if(hold&&p.action==='save.status'){hold=false;queryEntered();await new Promise(resolve=>releaseQuery=resolve);}
   return {state,request,draft:{draftRef:{documentId:'doc',revision},savedRevision:saved,source:{editRevision:input},saveSequence:String(saves.length),exportState:'idle'}};
  }
  throw Error('Unexpected SDK call '+method);
 });
 const view={viewId:'config',viewType:'configFile',context,params:{path:'/config'}};
 try{
  await peer.request('initialize',{capabilities:['files.v1','files.editor.v1','viewData.v1','ui.workbench.v2','ui.openView.v1']});
  await peer.request('ui.createView',view);
  const source={viewId:'source',viewType:'manager',context};
  assert.equal((await peer.request('ui.event',{...source,data:{action:'fileEditor.resource',expectedContext:context}})).data.id,'source');
  assert.equal((await peer.request('ui.event',{...source,data:{action:'fileEditor.open',viewType:'configFile',path:'/config',expectedContext:context,requestId:'open'}})).data.viewId,'config');
  await peer.request('ui.event',{...view,data:{action:'editorState',editRevision:1}});
  const closed=await peer.request('ui.save',view);assert.equal(closed.metadata.dirty,false);assert.equal(saves.length,1);assert.equal(saves[0].expectedDraftRef.revision,'1');
  await peer.request('ui.event',{...view,data:{action:'editorState',editRevision:2}});
  assert((await peer.request('ui.getView',view)).metadata.dirty);
  hold=true;const pending=peer.request('ui.beforeClose',view);await entered;
  assert.equal((await peer.request('ui.getView',view)).data.path,'/config');releaseQuery();await pending;
  await peer.request('ui.disposeView',source);assert(!resources.has('source'));assert(resources.has('config'));
  await peer.request('ui.disposeView',view);assert.equal(resources.size,0);await peer.close();
 }finally{releaseQuery?.();await peer.kill();}
});

test('closing a source or editor during resource creation releases the late resource',async()=>{
 for(const editor of [false,true]){
  let entered,release;
  const waiting=new Promise(resolve=>entered=resolve),closed=[];
  const peer=pluginPeer(path.resolve('crates/plugin-client/target/debug/examples/file-editor-fixture'+(process.platform==='win32'?'.exe':'')),async(method,p)=>{
   assert.equal(method,'files.request');
   if(p.action==='open'){entered();await new Promise(resolve=>release=resolve);return {id:'late'};}
   assert.equal(p.action,'close');closed.push(p.id);return {};
  });
  const view={viewId:'late-view',viewType:editor?'configFile':'manager',context:{workspaceId:'sdk'},params:{path:'/config'}};
  try{
   const pending=peer.request(editor?'ui.createView':'ui.event',{...view,data:{action:'fileEditor.resource',expectedContext:view.context}});
   // Observe rejection before continuing so late cleanup cannot create an unhandled rejection.
   const rejected=assert.rejects(pending,/已关闭/);
   await waiting;
   if(editor)await assert.rejects(peer.request('ui.createView',view),/正在创建/);
   await peer.request('ui.disposeView',view);release();await rejected;
   assert.deepEqual(closed,['late']);await peer.close();
  }finally{release?.();await peer.kill();}
 }
});
