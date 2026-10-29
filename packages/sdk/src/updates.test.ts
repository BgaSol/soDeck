import {describe,it,expect} from 'vitest';
import {validateUpdates,updatesClient} from './updates';
import {createCore} from './index';
describe('updates v1',()=>{
  const operationId='00000000-0000-4000-8000-000000000001';
  it('rejects URL/path injection, unknown fields and oversized choices before transport',async()=>{
    let calls=0;const core=createCore(async r=>{calls++;return {requestId:r.requestId,result:{}};});
    await expect(core.call('updates.download',{operationId,planRevision:'revision',componentIds:['core'],url:'https://example.com'})).rejects.toMatchObject({code:'INVALID_PARAMS'});
    expect(calls).toBe(0);
    expect(validateUpdates('x-methods','updates.install',{operationId,planRevision:'revision',componentIds:Array.from({length:65},(_,i)=>`plugin.${i}`)})).toBeTruthy();
    expect(validateUpdates('x-methods','updates.install',{operationId,planRevision:'revision',componentIds:['core']})).toBeUndefined();
  });
  it('keeps checking, downloading and installing as distinct requests',async()=>{
    const calls:{method:string;params:unknown}[]=[];
    const client=updatesClient({async call<T>(method:string,params?:unknown){calls.push({method,params});return {} as T;}});
    await client.check(operationId);expect(calls.map(c=>c.method)).toEqual(['updates.check']);
    await client.download({operationId,planRevision:'a',componentIds:['sodeck.terminal']});
    expect(calls).toHaveLength(2);expect(calls[1].params).toMatchObject({componentIds:['sodeck.terminal']});
  });
  it('rejects malformed state and preserves requiresUserAction receipts',()=>{
    const receipt={operationId,kind:'updates.install',stage:'requiresUserAction',running:false,components:[],error:null,updatedAt:0};
    expect(validateUpdates('x-results','updates.install',receipt)).toBeUndefined();
    expect(validateUpdates('x-results','updates.install',{...receipt,stage:'autoInstalling'})).toBeTruthy();
  });
});
