import test from 'node:test';
import assert from 'node:assert/strict';
import {newCopyDraft,toInput,canCopySecret} from '../src/server-api.ts';

const source=()=>({id:'source',protocol:'ssh',name:'Web',host:'10.0.0.1',port:22,username:'ops',tagIds:['prod'],authentication:{type:'password',hasSavedSecret:true,hasSessionSecret:false},route:[{id:'step',kind:'ssh',serverId:'gateway'}]});
test('copy is an independent draft, copies credentials by reference only when unchanged, and allocates new route IDs on save',()=>{
  const original=source(),draft=newCopyDraft(original);
  assert.equal(draft.id,null);assert.equal(draft.password,'');assert.equal(draft.name,'Web 副本');
  assert.notEqual(draft.route[0].id,'step');const key=draft.route[0].id;
  draft.host='10.0.0.2';draft.username='admin';draft.tagIds.push('web');
  const input=toInput(draft,false);assert.equal(input.copyFromId,'source');assert.equal(input.route[0].id,'');
  assert.equal(draft.route[0].id,key);assert.deepEqual(original.tagIds,['prod']);
  draft.password='new';assert.equal(toInput(draft,false).copyFromId,undefined);assert.equal(toInput(draft,false).authentication.secret.action,'replace');
  draft.password='';assert.equal(toInput(draft,false).copyFromId,'source');
  draft.clearSecret=true;assert.equal(toInput(draft,false).copyFromId,undefined);assert.equal(toInput(draft,false).authentication.secret.action,'clear');
  draft.clearSecret=false;draft.authType='privateKey';assert.equal(canCopySecret(draft),false);
  assert.equal(toInput(draft,true).copyFromId,undefined);
});
test('temporary credentials stay temporary, missing secrets need no source, and key passphrases require the original key path',()=>{
  const record=source();record.authentication.hasSavedSecret=false;record.authentication.hasSessionSecret=true;
  assert.equal(newCopyDraft(record).remember,false);
  record.authentication.hasSessionSecret=false;assert.equal(newCopyDraft(record).remember,true);assert.equal(toInput(newCopyDraft(record),false).copyFromId,undefined);
  record.authentication={type:'privateKey',privateKeyPath:'/keys/id',hasSavedSecret:true,hasSessionSecret:false};
  const draft=newCopyDraft(record);assert.equal(canCopySecret(draft),true);draft.privateKeyPath='/keys/other';assert.equal(canCopySecret(draft),false);
  draft.clearSecret=true;draft.password='replacement-key-passphrase';assert.equal(toInput(draft,false).authentication.secret.action,'replace');
});
