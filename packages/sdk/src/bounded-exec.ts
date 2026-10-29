import {decode, encode} from './bytes';
import {validateRemoteRead} from './remote-read';
import type {WorkspaceScope, ExecReadResult} from './remote-read';
import type {Clock} from './runtime';

export interface ExecBridge {call<T = unknown>(method: string, params?: unknown): Promise<T>}
export const execFault = (code: string, message: string) => Object.assign(new Error(message), {code});
export class Cancellation {
  canceled = false;
  private listeners = new Set<() => void>();
  cancel() {if (this.canceled) return; this.canceled = true; for (const fn of this.listeners) fn(); this.listeners.clear();}
  listen(fn: () => void) {if (this.canceled) fn(); else this.listeners.add(fn); return () => {this.listeners.delete(fn);};}
}

/** Timed-out requests keep admission slots until their actual transports settle. */
export class RpcBudget implements ExecBridge {
  private pending = 0;
  private ordinary = 0;
  constructor(private bridge: ExecBridge, readonly maximum = 16, readonly ordinaryMaximum = 12) {}
  counts() {return {pending: this.pending, ordinary: this.ordinary};}
  async call<T = unknown>(method: string, params?: unknown): Promise<T> {
    const control = /(?:\.cancel|\.close|\.release|\.forget)$/.test(method) || method === 'state.ack';
    if (this.pending >= this.maximum || (!control && this.ordinary >= this.ordinaryMaximum))
      throw execFault('CAPACITY', '调用额度已满，等待现有请求结束');
    this.pending++; if (!control) this.ordinary++;
    try {return await this.bridge.call<T>(method, params);}
    finally {this.pending--; if (!control) this.ordinary--;}
  }
}
export interface ExecRequest {
  scope: WorkspaceScope; leaseId: string; connectionGeneration: number;
  operationId: string; command: string; stdin?: Uint8Array;
  remoteTimeoutMs: number; deadlineMs: number; maxOutputBytes: number; cancellation?: Cancellation;
}
export interface ExecResult {
  stdout: Uint8Array; stderr: Uint8Array; exitCode: number | null;
  endedReason: string | null; truncated: boolean; elapsedMs: number; cleanupPending: boolean;
}
interface Job {
  request: ExecRequest; token: Cancellation; deadline: number; started: number;
  execId?: string; receiptDone: boolean; receiptForgotten: boolean;
  running: boolean; cleanupPending: boolean; cleaning?: Promise<void>; closedIds: Set<string>;
}
const join = (chunks: Uint8Array[], count: number) => {
  const result = new Uint8Array(count); let offset = 0;
  for (const chunk of chunks) {result.set(chunk, offset); offset += chunk.length;}
  return result;
};
export class BoundedExecClient {
  private jobs = new Set<Job>();
  private stopped = false;
  constructor(private bridge: ExecBridge, private clock: Clock, private maximum = 4) {}
  counts() {return {active: this.jobs.size, cleanupPending: [...this.jobs].filter(j => j.cleanupPending).length};}
  private async call<T>(method: string, params: unknown): Promise<T> {
    const invalid = validateRemoteRead('x-methods', method, params);
    if (invalid) throw execFault('INVALID_PARAMS', invalid);
    const value = await this.bridge.call<T>(method, params);
    const error = validateRemoteRead('x-results', method, value);
    if (error) throw execFault('PROTOCOL_ERROR', error);
    return value;
  }
  private wait<T>(promise: Promise<T>, deadline: number, token?: Cancellation): Promise<T> {
    const remaining = deadline - this.clock.monotonicMs();
    if (remaining <= 0) {void promise.catch(() => {}); return Promise.reject(execFault('TIMEOUT', '执行总期限已到'));}
    return new Promise((resolve, reject) => {
      let done = false, off = () => {};
      const finish = (fn: () => void) => {if (done) return; done = true; clearTimeout(timer); off(); fn();};
      const timer = setTimeout(() => finish(() => reject(execFault('TIMEOUT', '执行总期限已到'))), remaining);
      off = token?.listen(() => finish(() => reject(execFault('CANCELED', '执行已取消')))) ?? off;
      promise.then(value => finish(() => resolve(value)), error => finish(() => reject(error)));
    });
  }
  private delay(ms: number, job: Job) {
    return this.wait(new Promise<void>(resolve => setTimeout(resolve, ms)), job.deadline, job.token);
  }
  private target(job: Job) {
    const {scope, leaseId, connectionGeneration} = job.request;
    return {scope, leaseId, connectionGeneration, execId: job.execId!};
  }
  private async forget(job: Job, deadline: number) {
    if (!job.receiptDone || job.receiptForgotten) return;
    await this.wait(this.call('remote.operations.forget', {operationId: job.request.operationId}), deadline);
    job.receiptForgotten = true;
  }
  private accepted(job: Job, id: string) {
    job.receiptDone = true;
    if (job.closedIds.has(id)) return;
    job.execId = id; this.jobs.add(job);
    if (!job.running && !job.cleaning) void this.cleanup(job);
  }
  private receipt(job: Job) {
    return this.call<{state: string; result?: {execId?: string}; error?: unknown}>('remote.operations.get', {operationId: job.request.operationId})
      .then(value => {if (value.state !== 'running') job.receiptDone = true; if (value.result?.execId) this.accepted(job, value.result.execId); return value;});
  }
  private async close(job: Job, deadline: number) {
    while (job.execId) {
      const id = job.execId, target = {...this.target(job), execId: id};
      if (job.token.canceled || job.running) await this.wait(this.call('exec.cancel', target), deadline).catch(() => {});
      try {await this.wait(this.call('exec.close', target), deadline);}
      catch (error) {if (!['STALE_HANDLE','STALE_WORKSPACE','CONNECTION_NOT_READY'].includes((error as {code?:string}).code ?? '')) throw error;}
      job.closedIds.add(id); if (job.execId === id) job.execId = undefined;
    }
  }
  private cleanup(job: Job): Promise<void> {
    if (job.cleaning) return job.cleaning;
    job.cleaning = (async () => {
      const deadline = this.clock.monotonicMs() + 4000;
      try {
        if (job.execId) {
          await this.close(job, deadline);
        } else if (!job.receiptDone) {
          await this.wait(this.bridge.call('operations.cancel', {operationId: job.request.operationId}), deadline);
          const receipt = await this.wait(this.receipt(job), deadline);
          if (receipt.state === 'running') throw execFault('RESULT_UNKNOWN', '仍在核对执行资源');
          job.receiptDone = true;
          await this.close(job, deadline);
        }
        await this.forget(job, deadline);
        job.cleanupPending = false; if (!job.running) this.jobs.delete(job);
      } catch (error) {
        if (['RESULT_EXPIRED', 'STALE_WORKSPACE', 'STALE_HANDLE', 'CONNECTION_NOT_READY'].includes((error as {code?: string}).code ?? '')) {
          job.cleanupPending = false; this.jobs.delete(job);
        } else job.cleanupPending = true;
      }
    })().finally(() => {job.cleaning = undefined; if (!job.running && job.execId && !job.cleanupPending) void this.cleanup(job);});
    return job.cleaning;
  }
  async run(request: ExecRequest): Promise<ExecResult> {
    if (this.stopped || request.cancellation?.canceled) throw execFault('CANCELED', '执行客户端已停止');
    if (this.jobs.size >= this.maximum) throw execFault('CAPACITY', '执行并发已满');
    if (!(request.deadlineMs > 0 && request.deadlineMs <= 60000) || request.maxOutputBytes < 1 || request.maxOutputBytes > 4194304 || (request.stdin?.length ?? 0) > 65536)
      throw execFault('INVALID_PARAMS', '执行预算无效');
    const started = this.clock.monotonicMs(), token = new Cancellation();
    const job: Job = {request, token, started, deadline: started + request.deadlineMs, receiptDone: false, receiptForgotten: false, running: true, cleanupPending: false, closedIds: new Set()};
    this.jobs.add(job);
    const off = request.cancellation?.listen(() => token.cancel());
    const stdout: Uint8Array[] = [], stderr: Uint8Array[] = [];
    let out = 0, err = 0, sequence = 0, exitCode: number | null = null, endedReason: string | null = null, truncated = false;
    try {
      const {scope, leaseId, connectionGeneration, operationId, command, remoteTimeoutMs, maxOutputBytes} = request;
      const creation = this.call<{execId: string}>('exec.start', {scope, leaseId, connectionGeneration, operationId, command, timeoutMs: remoteTimeoutMs, maxOutputBytes, ...(request.stdin ? {stdinBase64: encode(request.stdin)} : {})})
        .then(value => {this.accepted(job, value.execId); return value;});
      try {await this.wait(creation, job.deadline, token);}
      catch (error) {
        if ((error as {code?: string}).code !== 'RESULT_UNKNOWN') throw error;
        for (let attempt = 0; ; attempt++) {
          const receipt = await this.wait(this.receipt(job), job.deadline, token);
          if (receipt.state === 'completed' && receipt.result?.execId) break;
          if (receipt.state !== 'running') {job.receiptDone = true; throw receipt.error ?? execFault('EXEC_FAILED', '执行未能开始');}
          await this.delay([100, 250, 500][Math.min(attempt, 2)], job);
        }
      }
      while (!token.canceled) {
        const page = await this.wait(this.call<ExecReadResult>('exec.read', {...this.target(job), afterSequence: sequence, maxBytes: 32768}), job.deadline, token);
        for (const frame of page.frames) {
          if (frame.sequence <= sequence) continue;
          if (frame.sequence !== sequence + 1) throw execFault('PROTOCOL_ERROR', '执行输出存在缺口');
          const bytes = decode(frame.bytesBase64); if (bytes.length !== frame.length) throw execFault('PROTOCOL_ERROR', '执行帧长度不匹配'); sequence = frame.sequence;
          if (out + err + bytes.length > maxOutputBytes) {truncated = true; break;}
          if (frame.stream === 'stdout') {stdout.push(bytes); out += bytes.length;} else {stderr.push(bytes); err += bytes.length;}
        }
        await this.wait(this.call('exec.ack', {...this.target(job), sequence}), job.deadline, token);
        truncated ||= page.truncated;
        if (truncated) {endedReason = 'LIMIT_EXCEEDED'; token.cancel(); break;}
        if (page.state === 'failed' || page.state === 'canceled') {endedReason = page.reason ?? 'EXEC_FAILED'; break;}
        if (page.state === 'completed' && sequence === page.lastSequence) {exitCode = page.exitCode; endedReason = page.reason; break;}
        await this.delay(100, job);
      }
      if (token.canceled && !truncated) throw execFault('CANCELED', '执行已取消');
    } catch (error) {endedReason = (error as {code?: string}).code ?? 'EXEC_FAILED'; token.cancel();}
    finally {job.running = false; off?.(); await this.cleanup(job);}
    return {stdout: join(stdout, out), stderr: join(stderr, err), exitCode, endedReason, truncated, elapsedMs: this.clock.monotonicMs() - started, cleanupPending: job.cleanupPending};
  }
  async cancelAll() {for (const job of this.jobs) job.token.cancel(); await Promise.all([...this.jobs].map(job => this.cleanup(job)));}
  async retryCleanup() {await Promise.all([...this.jobs].filter(job => !job.running).map(job => this.cleanup(job)));}
  async dispose() {this.stopped = true; await this.cancelAll();}
}
export const createBoundedExecClient = (bridge: ExecBridge, clock: Clock, limits: {maximum?: number} = {}) => new BoundedExecClient(bridge, clock, limits.maximum);
