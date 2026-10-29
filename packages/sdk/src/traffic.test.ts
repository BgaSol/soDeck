import {describe,it,expect} from 'vitest';
import {byteCount,formatByteCount,validateTraffic,communicationObjectId,formatTrafficRate} from './traffic';
import {validate} from './schema';
describe('communication counters and scope',()=>{
  it('matches the Rust canonical target key while distinguishing epochs, layers and generations',async()=>{
    const request={target:{kind:'server' as const,configEpoch:'epoch',serverId:'server-a'}};
    expect(await communicationObjectId(request)).toBe('64f8601ee329f7fbb34350d357605f32e878723bd3594e24ddd777d509b5d5e5');
    expect(await communicationObjectId({...request,layer:'channel'})).toBe(await communicationObjectId(request));
    expect(await communicationObjectId({...request,layer:'transport'})).not.toBe(await communicationObjectId(request));
    expect(formatTrafficRate(125000,'bits')).toBe('1 Mbit/s');expect(formatTrafficRate(null)).toBe('—');
  });
  it('retains unsigned 64-bit precision and rejects overflow',()=>{
    expect(byteCount('18446744073709551615')).toBe(18446744073709551615n);
    expect(byteCount('9007199254740993')).toBe(9007199254740993n);
    for(const value of ['18446744073709551616','01','-1','NaN','1e6'])expect(()=>byteCount(value)).toThrow();
    expect(formatByteCount('1024')).toBe('1.0 KiB');
    expect(validate({$defs:{}},{type:'string',format:'uint64'},'18446744073709551616','bytes')).toBeTruthy();
  });
  it('validates generation-qualified targets and bounded pages',()=>{
    expect(validateTraffic('x-methods','communication.summary',{})).toBeUndefined();
    expect(validateTraffic('x-methods','communication.summary',{target:{kind:'resource',resourceId:'r',generation:'1',configEpoch:'e'}})).toBeTruthy();
    expect(validateTraffic('x-methods','communication.sources',{limit:201})).toBeTruthy();
    expect(validateTraffic('x-methods','communication.summary',{layer:'carrier'})).toBeTruthy();
    expect(validateTraffic('x-methods','communication.summary',{pluginId:'spoof'})).toBeTruthy();
  });
});
