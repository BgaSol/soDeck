import test from 'node:test';
import assert from 'node:assert/strict';
import {newDraft,commitTagInput,toInput} from '../src/server-api.ts';

test('tag input reuses IDs, deduplicates new names and preserves historical comma names',()=>{
  const catalog={tags:[{id:'old',name:'生产'},{id:'comma',name:'工厂，机房'}]};
  const draft=newDraft();draft.tagIds=['comma'];draft.tagInput=' 生产，数据库,数据库,, ';
  commitTagInput(draft,catalog);
  assert.deepEqual(draft.tagIds,['comma','old']);assert.deepEqual(draft.newTagNames,['数据库']);assert.equal(draft.tagInput,'');
  assert.deepEqual(toInput(draft,false).newTagNames,['数据库']);assert.equal('tagInput' in toInput(draft,false),false);
  assert.equal('newTagNames' in toInput(draft,true),false);
  assert.deepEqual(catalog.tags.map(t=>t.name),['生产','工厂，机房']);
});
