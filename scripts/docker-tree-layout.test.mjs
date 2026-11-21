import {test} from 'node:test';
import assert from 'node:assert/strict';
import {TreeLayout} from '../plugins/docker/ui/tree-layout.js';
test('one measured region preserves pixel/index mapping before, within and after its anchor',()=>{
  const layout=new TreeLayout();layout.reset(1000,{index:20,node:{key:'a'}});layout.measure(187);
  assert.equal(layout.size,30187);assert.equal(layout.index(650),20);assert.equal(layout.index(816),20);
  for(const n of [0,19,20,21,400,999])assert.equal(layout.index(layout.offset(n)+15),n);
  const old=layout.offset(400);layout.measure(287);assert.equal(layout.offset(400)-old,100);
  layout.reset(1001,{index:21,node:{key:'a'}});assert.equal(layout.height,287);
  layout.reset(1001,{index:21,node:{key:'b'}});assert.equal(layout.height,0);
  layout.reset(1000,null);assert.equal(layout.size,30000);assert.equal(layout.index(99999),999);
});
