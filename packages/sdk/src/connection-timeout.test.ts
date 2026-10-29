import {describe,it,expect,vi} from 'vitest';
import {connectionCommandTimeout} from './connection-timeout';

describe('connection command timeout',()=>{
  it('uses the core budget without traversing the server directory',async()=>{
    const call=vi.fn().mockResolvedValueOnce({capabilities:['connections.route.v2']}).mockResolvedValueOnce({saved:{attemptTimeoutMs:900000}});
    expect(await connectionCommandTimeout({call},'target','workspace')).toBe(960_000);
    expect(call.mock.calls).toEqual([['capabilities.list'],['connections.route.get',{serverId:'target',workspaceId:'workspace'}]]);
  });
  it('rejects absent capability and transport errors without a fixed fallback',async()=>{
    const call=vi.fn().mockResolvedValue({capabilities:[]});
    await expect(connectionCommandTimeout({call},'target')).rejects.toMatchObject({code:'UNSUPPORTED_API'});
    expect(call).toHaveBeenCalledTimes(1);
    const unavailable=vi.fn().mockRejectedValue(new Error('offline'));
    await expect(connectionCommandTimeout({call:unavailable},'target')).rejects.toThrow('offline');
  });
});

it('rejects missing, unsafe and out of range budgets',async()=>{for(const budget of [undefined,NaN,119999,900001]){const call=vi.fn().mockResolvedValueOnce({capabilities:['connections.route.v2']}).mockResolvedValueOnce({saved:{attemptTimeoutMs:budget}});await expect(connectionCommandTimeout({call},'s')).rejects.toMatchObject({code:'PROTOCOL_ERROR'});}});
