import {test} from 'node:test';
import assert from 'node:assert/strict';
import {decide} from './plan.mjs';
import {sameAssetUrl} from './github.mjs';
const compare=(old,next)=>old===next?'equal':next>old?'higher':'lower';
test('plugin-only version advances reuse the original core identity',()=>{
  const old={appVersion:'2.0.0',inputFingerprint:'core'};
  assert.equal(decide('2.0.0','core',old,compare),'reuse');
  assert.equal(decide('1.1.0','plugin-b',{version:'1.0.0',inputFingerprint:'plugin-a'},compare),'build');
});
test('changed input without version increment is a release error',()=>{
  assert.throws(()=>decide('1.0.0','new',{version:'1.0.0',inputFingerprint:'old'},compare),/without a version/);
  assert.throws(()=>decide('1.0.0','old',{version:'1.1.0',inputFingerprint:'old'},compare),/decrease/);
});
test('GitHub slash escaping does not change the exact repository asset',()=>{
  const base='https://github.com/BgaSol/soDeck/releases/download/';
  const expected=base+'sodeck-v2%2Fpreview%2F000001/catalog-v1.json';
  assert.equal(sameAssetUrl(base+'sodeck-v2/preview/000001/catalog-v1.json',expected),true);
  for(const url of [expected+'?other=1',expected.replace('github.com','evil.test'),expected.replace('000001','000002')])assert.equal(sameAssetUrl(url,expected),false);
});
