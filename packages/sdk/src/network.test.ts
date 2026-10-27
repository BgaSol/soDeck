import {describe,it,expect} from 'vitest';
import {frame,parse,Kind,MAX_OFFSET} from './network-wire';
import {NetworkStreams} from './network-stream';
import {validateNetwork} from './network';
describe('network contracts and bounded binary frames',()=>{
  it('enforces frame length/version/offset before reading payload',()=>{
    const bytes=frame(Kind.Data,1,1,0,Uint8Array.of(1,2,3));expect(parse(bytes).payload).toEqual(Uint8Array.of(1,2,3));
    for(const index of [0,1,3,20]){const bad=bytes.slice();bad[index]=255;expect(()=>parse(bad)).toThrow();}
    expect(()=>frame(Kind.Data,1,1,MAX_OFFSET,Uint8Array.of(1))).toThrow();
    expect(()=>parse(bytes.subarray(0,bytes.length-1))).toThrow();
  });
  it('handles early DATA, FIN before data, duplicate credits and stale epochs',async()=>{
    let receive!:(bytes:Uint8Array)=>void;const sent:Uint8Array[]=[];
    const streams=new NetworkStreams({version:1,send:b=>{sent.push(b);return true;},onFrame:fn=>{receive=fn;return()=>{};}});
    receive(frame(Kind.Fin,1,2,3));receive(frame(Kind.Data,1,2,0,Uint8Array.of(4,5,6)));
    const session=streams.attach({id:'one',generation:1},{slot:1,epoch:2,window:65536,readOffset:0,writeOffset:0,writeCredit:65536});
    expect(await session.readable()).toEqual(Uint8Array.of(4,5,6));session.consume(3);expect(session.eof).toBe(true);
    receive(frame(Kind.Reset,1,1,0));expect(session.peek()).toHaveLength(0);
    expect(streams.bufferedBytes).toBe(0);expect(sent.map(parse).map(f=>f.kind)).toEqual([Kind.Ack,Kind.Credit]);streams.dispose();
  });
  it('reserves control order on a full transport and rejects excess credit',async()=>{
    let receive!:(bytes:Uint8Array)=>void;let blocked=true;const sent:number[]=[];
    const streams=new NetworkStreams({version:1,send:b=>{if(blocked)return false;sent.push(parse(b).kind);return true;},onFrame:fn=>{receive=fn;return()=>{};}});
    const session=streams.attach({id:'one',generation:1},{slot:1,epoch:1,window:65536,readOffset:0,writeOffset:0,writeCredit:65536});
    receive(frame(Kind.Data,1,1,0,Uint8Array.of(1)));session.consume(1);expect(sent).toEqual([]);blocked=false;await new Promise(r=>setTimeout(r,5));expect(sent).toEqual([Kind.Ack,Kind.Credit]);
    receive(frame(Kind.Credit,1,1,65537));expect(()=>session.peek()).toThrow();streams.dispose();expect(streams.bufferedBytes).toBe(0);
  });
  it('allows global operation receipts but validates all handles and scoped create',()=>{
    expect(validateNetwork('x-methods','net.operations.get',{operationId:'global'})).toBeUndefined();
    expect(validateNetwork('x-methods','net.groups.create',{viewId:'v',operationId:'one'})).toBeTruthy();
    expect(validateNetwork('x-methods','net.resources.stop',{resource:{id:'one',generation:0},operationId:'one'})).toBeTruthy();
  });
});
