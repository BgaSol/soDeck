import test from 'node:test';
import assert from 'node:assert/strict';
import {panelGeometry} from '../src/panel-layout.ts';

test('dragging either panel stops at available room without hiding it',()=>{
  for(const side of ['left','right']){
    const layout=panelGeometry(596,{leftWidth:side==='left'?350:280,rightWidth:side==='right'?350:280,drag:{side,otherWidth:280}},true,true,side==='left'?'right':'left');
    assert.equal(layout.left,true);assert.equal(layout.right,true);
    assert.equal(layout[side+'Width'],316);assert.equal(layout[(side==='left'?'right':'left')+'Width'],280);
  }
});
test('responsive constraints preserve preferences and hide only below the minimum room',()=>{
  const widths={leftWidth:400,rightWidth:400};
  assert.deepEqual(panelGeometry(596,widths,true,true,'right'),{left:true,right:true,leftWidth:196,rightWidth:400});
  assert.deepEqual(panelGeometry(316,widths,true,true,'right'),{left:false,right:true,leftWidth:0,rightWidth:316});
  assert.deepEqual(panelGeometry(159,widths,true,true,'right'),{left:false,right:false,leftWidth:0,rightWidth:0});
  assert.deepEqual(widths,{leftWidth:400,rightWidth:400});
});
test('fractional display geometry still produces integral widths accepted by the core',()=>{
  const layout=panelGeometry(596.75,{leftWidth:350.25,rightWidth:280,drag:{side:'left',otherWidth:280}},true,true,'right');
  assert.equal(layout.leftWidth,316);assert(Number.isInteger(layout.leftWidth));
});
