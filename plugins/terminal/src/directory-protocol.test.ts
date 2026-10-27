import {describe,it,expect} from 'vitest';
import {DirectoryProtocol,type Report} from './directory-protocol';
import {encode,utf8} from '@sodeck/sdk/bytes';
const tag='0123456789abcdef';
const frame=(logical:Uint8Array,physical:Uint8Array)=>utf8(`\x1b]633;SoDeck;1;${tag};2;ready;123;42;1000;mnt:[1];1;2;${encode(logical)};${encode(new Uint8Array([...physical,10]))}\x1b\\`);
describe('shell byte protocol',()=>{
  it('preserves non-UTF8 and trailing newlines across every possible frame split',()=>{
    const path=new Uint8Array([47,116,109,112,47,255,10,10]);const bytes=frame(path,path);
    for(let split=0;split<=bytes.length;split++){
      const reports:Report[]=[];const parser=new DirectoryProtocol(tag,r=>reports.push(r),()=>{throw Error('invalid');});
      parser.push(bytes.subarray(0,split));parser.push(bytes.subarray(split));
      expect(reports).toHaveLength(1);expect((reports[0] as Extract<Report,{status:'ready'}>).physical.bytesBase64).toBe(encode(path));
    }
  });
  it('ignores unbound OSC, rejects invalid paths and recovers after oversized/truncated frames',()=>{
    const reports:Report[]=[];let invalid=0;const p=new DirectoryProtocol(tag,r=>reports.push(r),()=>invalid++);
    p.push(utf8('\x1b]7;file://fake/tmp\x07\x1b]633;SoDeck;1;other;9;busy\x07'));
    expect(reports).toHaveLength(0);
    p.push(frame(utf8('relative'),utf8('/tmp')));expect(invalid).toBe(1);
    p.push(utf8('\x1b]'+ 'x'.repeat(17000)));p.reset();
    p.push(frame(utf8('/tmp'),utf8('/tmp')));expect(reports).toHaveLength(1);expect(invalid).toBe(2);
  });
});
