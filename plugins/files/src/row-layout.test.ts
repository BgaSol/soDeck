import {expect,it} from 'vitest';
import {RowLayout} from './row-layout';
it('keeps sparse mounts at one unit with exact viewport and keyboard offsets',()=>{
  const rows=Array.from({length:20000},(_,i)=>({mount:i%101===0})),layout=new RowLayout(rows);
  expect(layout.total).toBe(20000);
  rows.forEach((_,i)=>{expect(layout.at(i)).toBe(i);expect(layout.at(i+0.5)).toBe(i);expect(layout.offsets[i+1]).toBe(i+1);});
  const range=layout.range(102,3);expect(layout.offsets[range.first]).toBe(102);expect(layout.offsets[range.end]).toBeGreaterThanOrEqual(105);
  expect(layout.at(1e9)).toBe(rows.length-1);
});
