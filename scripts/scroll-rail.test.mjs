import assert from 'node:assert/strict';
import {test} from 'node:test';
import {resolveScrollRailMode} from '../packages/ui/scroll-rail.js';

test('minimap replaces the vertical rail only while an interactive navigation surface is available',()=>{
  const ready={preferred:true,mounted:true,usable:true,forcedColors:false,narrow:false};
  assert.equal(resolveScrollRailMode(ready),'minimap');
  for(const key of ['preferred','mounted','usable','forcedColors','narrow'])assert.equal(resolveScrollRailMode({...ready,[key]:!ready[key]}),'native',key);
  assert.equal(resolveScrollRailMode(ready),'minimap','Recovery uses current capability rather than a latched fallback');
});
