import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {openRemoteFile} from '../packages/file-editor/src/open.js';
import {fileEditorSettings} from './plugin-file-editor-assets.mjs';
test('open uses the source resource, canonical path and identical retry identity',async()=>{
 const calls=[];let fail=true;
 const context={workspaceId:'workspace',generation:1};
 const result=await openRemoteFile({path:'/alias',mount:{context},fileId:'source',request:async(method,p)=>{
  calls.push({method,p});if(method==='data'){assert.equal(p.id,'source');return {path:'/real',attrs:{kind:'file',size:10}};}
  if(fail){fail=false;throw Error('lost open observation');}return {data:{viewId:'editor'}};
 }});
 assert.equal(result.viewId,'editor');assert.equal(calls.length,3);assert.deepEqual(calls[1],calls[2]);assert.equal(calls[1].p.path,'/real');assert.deepEqual(calls[1].p.expectedContext,context);
});
test('source without a file resource asks its own process; directory and oversized file never open',async()=>{
 for(const attrs of [{kind:'directory',size:0},{kind:'file',size:1048577}]){
  const calls=[];await assert.rejects(openRemoteFile({path:'/x',mount:{context:{}},request:async(method,p)=>{calls.push(p);if(method==='event')return {data:{id:'own'}};assert.equal(p.id,'own');return {path:'/x',attrs};}}));
  assert.equal(calls.length,2);assert.equal(calls[0].action,'fileEditor.resource');
 }
});
test('editor settings expansion keeps original identities and independent plugin declarations',()=>{
 const raw=JSON.parse(fs.readFileSync('plugins/files/release.json','utf8'));const expanded=fileEditorSettings(raw);
 const page=expanded.settings.pages.find(p=>p.id==='editor');assert.equal(page.fileEditor,undefined);assert.deepEqual(page.viewTypes,['editor']);assert.equal(page.groups[0].fields[0].key,'font');assert(raw.settings.pages.find(p=>p.id==='editor').fileEditor);
 const another={build:{fileEditor:true},settings:{pages:[{fileEditor:true,id:'config',viewTypes:['configFile']}]}};
 const cloned=fileEditorSettings(another);assert.equal(cloned.settings.pages[0].id,'config');assert.deepEqual(cloned.settings.pages[0].viewTypes,['configFile']);
 assert.throws(()=>fileEditorSettings({settings:another.settings}),/build.fileEditor/);
});
