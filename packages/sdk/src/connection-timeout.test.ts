import {describe,it,expect,vi} from 'vitest';
import {connectionCommandTimeout} from './connection-timeout';

describe('connection command timeout',()=>{
  it('uses public node count without fetching or traversing the server directory',async()=>{
    const call=vi.fn().mockResolvedValueOnce({capabilities:['connections.route.v2']}).mockResolvedValueOnce({saved:{nodeCount:16}});
    expect(await connectionCommandTimeout({call},'target','workspace')).toBe(1_230_000);
    expect(call.mock.calls).toEqual([['capabilities.list'],['connections.route.v2.get',{serverId:'target',workspaceId:'workspace'}]]);
  });
  it('uses the legacy budget only when the capability is absent',async()=>{
    const call=vi.fn().mockResolvedValue({capabilities:[]});
    expect(await connectionCommandTimeout({call},'target')).toBe(180_000);
    expect(call).toHaveBeenCalledTimes(1);
    const unavailable=vi.fn().mockRejectedValue(new Error('offline'));
    await expect(connectionCommandTimeout({call:unavailable},'target')).rejects.toThrow('offline');
  });
});
