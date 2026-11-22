import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compatiblePluginApi,currentPluginApi} from './plugin-api-version.mjs';

test('plugin packages use the same canonical major.minor rule as the protocol',()=>{
  assert.equal(currentPluginApi(),'2.0');
  assert(compatiblePluginApi('2.1','2.0'));
  for(const value of ['1','1.0','2','2.00','2.1','2.0.0','3.0','2.-1','','4294967296.0'])assert(!compatiblePluginApi('2.0',value),value);
});
