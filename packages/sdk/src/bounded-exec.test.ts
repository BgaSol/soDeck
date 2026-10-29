import {describe, it, expect, vi, afterEach} from 'vitest';
import {BoundedExecClient, Cancellation, RpcBudget, type ExecRequest} from './bounded-exec';
import {encode, utf8} from './bytes';
const request: ExecRequest = {scope: {workspaceId: 'w', workspaceGeneration: 'g'}, leaseId: 'l', connectionGeneration: 1, operationId: 'op', command: 'sh -s', remoteTimeoutMs: 4000, deadlineMs: 6000, maxOutputBytes: 1024};
const clock = {monotonicMs: () => performance.now()};
function fixture(overrides: Record<string, (p: any) => any> = {}) {
  const calls: {method: string; p: any}[] = [];
  const bridge = {async call<T>(method: string, p: any): Promise<T> {
    calls.push({method, p});
    if (overrides[method]) return await overrides[method](p);
    if (method === 'exec.start') return {execId: 'e'} as T;
    if (method === 'exec.read') return {frames: [{sequence: 1, stream: 'stdout', bytesBase64: encode(utf8('sample')), length: 6}], lastSequence: 1, acknowledged: 0, state: 'completed', exitCode: 0, reason: null, truncated: false} as T;
    if (method === 'exec.ack') return {acknowledged: p.sequence} as T;
    if (method === 'exec.close') return {closed: true} as T;
    if (method === 'exec.cancel') return {canceled: true} as T;
    if (method === 'remote.operations.forget') return {forgotten: true} as T;
    if (method === 'operations.cancel') return {canceled: true} as T;
    if (method === 'remote.operations.get') return {state: 'completed', result: {execId: 'e'}} as T;
    throw new Error(method);
  }};
  return {calls, client: new BoundedExecClient(bridge, clock), bridge};
}
afterEach(() => vi.useRealTimers());
describe('bounded remote execution', () => {
  it('reads, acknowledges and closes before forgetting a receipt', async () => {
    const f = fixture(), result = await f.client.run(request);
    expect(result.endedReason).toBeNull(); expect(result.cleanupPending).toBe(false);
    expect(new TextDecoder().decode(result.stdout)).toBe('sample');
    expect(f.calls.map(c => c.method)).toEqual(['exec.start', 'exec.read', 'exec.ack', 'exec.close', 'remote.operations.forget']);
    expect(f.client.counts().active).toBe(0);
  });
  it('recovers an unknown create using the same operation, never starts twice', async () => {
    const f = fixture({'exec.start': () => {throw {code: 'RESULT_UNKNOWN'};}});
    expect((await f.client.run(request)).endedReason).toBeNull();
    expect(f.calls.filter(c => c.method === 'exec.start')).toHaveLength(1);
    expect(f.calls.find(c => c.method === 'remote.operations.get')?.p.operationId).toBe('op');
  });
  it('cancels an overflowing output, but does not report it as complete data', async () => {
    const f = fixture(), r = await f.client.run({...request, maxOutputBytes: 3});
    expect(r.truncated).toBe(true); expect(r.endedReason).toBe('LIMIT_EXCEEDED');
    expect(f.calls.some(c => c.method === 'exec.cancel')).toBe(true);
    expect(f.calls.some(c => c.method === 'exec.close')).toBe(true);
  });
  it('keeps RPC slots while an underlying call is unresolved', async () => {
    let release!: (v: unknown) => void;
    const budget = new RpcBudget({call: <T>() => new Promise<T>(r => {release = value => r(value as T);})}, 2, 1);
    const pending = budget.call('exec.read');
    await expect(budget.call('exec.start')).rejects.toMatchObject({code: 'CAPACITY'});
    expect(budget.counts().pending).toBe(1); release({}); await pending;
    expect(budget.counts().pending).toBe(0);
  });
  it('canceled creation closes a late returned handle', async () => {
    let complete!: (v: unknown) => void; const token = new Cancellation();
    const f = fixture({'exec.start': () => new Promise(r => {complete = r;})});
    const running = f.client.run({...request, cancellation: token});
    await Promise.resolve(); token.cancel(); complete({execId: 'e'});
    const result = await running;
    expect(result.endedReason).toBe('CANCELED');
    expect(f.calls.some(c => c.method === 'exec.close')).toBe(true);
  });
  it('reports cleanup pending and blocks a replacement until close recovers', async () => {
    let failed = true;
    const f = fixture({'exec.close': () => {if(failed)throw {code:'TRANSPORT_ERROR'};return {closed:true};}});
    const result = await f.client.run(request);
    expect(result.cleanupPending).toBe(true);expect(f.client.counts().active).toBe(1);
    expect(f.calls.some(c=>c.method==='remote.operations.forget')).toBe(false);
    failed=false;await f.client.retryCleanup();expect(f.client.counts().active).toBe(0);
  });
  it('rejects a frame whose declared length differs from its payload', async () => {
    const f=fixture({'exec.read':()=>({frames:[{sequence:1,stream:'stdout',bytesBase64:encode(utf8('bad')),length:9}],lastSequence:1,acknowledged:0,state:'completed',exitCode:0,reason:null,truncated:false})});
    expect((await f.client.run(request)).endedReason).toBe('PROTOCOL_ERROR');
  });
  it('closes a claimed receipt even when it arrives after the cleanup deadline', async () => {
    vi.useFakeTimers({toFake:['setTimeout','clearTimeout','performance']});
    let deliver!: (value:unknown)=>void;
    const f=fixture({'exec.start':()=>{throw {code:'RESULT_UNKNOWN'};},'remote.operations.get':()=>new Promise(resolve=>{deliver=resolve;})});
    const pending=f.client.run({...request,deadlineMs:10});
    await vi.advanceTimersByTimeAsync(4011);
    expect((await pending).cleanupPending).toBe(true);
    deliver({state:'completed',result:{execId:'late'}});await vi.advanceTimersByTimeAsync(1);
    expect(f.calls.some(c=>c.method==='exec.close'&&c.p.execId==='late')).toBe(true);
  });
});
