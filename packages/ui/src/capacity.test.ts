import {expect,it} from 'vitest';
import {capacityUsage} from './capacity';

it('classifies exact capacity boundaries independently of displayed precision',()=>{
  for(const [used,percent,level] of [['0',0,'normal'],['7999',79.9,'normal'],['8000',80,'warning'],['8999',89.9,'warning'],['9000',90,'critical'],['10000',100,'critical']] as const){
    expect(capacityUsage({used,total:'10000'})).toMatchObject({percent,level});
  }
  const total=10n**35n;
  expect(capacityUsage({used:String(total*8n/10n-1n),total:String(total)})?.level).toBe('normal');
  expect(capacityUsage({used:String(total*9n/10n-1n),total:String(total)})?.level).toBe('warning');
  expect(capacityUsage({used:String(total),total:String(total)})?.percent).toBe(100);
});

it('keeps zero totals and invalid capacity unknown instead of drawing a percentage',()=>{
  for(const value of [{used:'0',total:'0'},{used:'-1',total:'100'},{used:'101',total:'100'},{used:'NaN',total:'100'},{used:'1',total:'Infinity'},{used:'1.5',total:'100'}])expect(capacityUsage(value)).toBeNull();
});
