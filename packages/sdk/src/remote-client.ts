import { validateRemoteRead } from "./remote-read";
import type {
  WorkspaceScope,
  Lease,
  Attributes,
  FilesReadDirectoryResult,
} from "./remote-read";
import { decode, type RemotePath } from "./bytes";
export type { WorkspaceScope, Lease, Attributes };
export interface Bridge {
  call<T = unknown>(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<T>;
}
export const sleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export const fault = (code: string, message: string) => ({ code, message });
export const message = (e: unknown) =>
  typeof e === "object" && e && "message" in e ? String(e.message) : String(e);
let operationSequence = 0;
interface Limiter {
  active: number;
  queue: Array<() => void>;
}
export class Remote {
  lease?: Lease;
  private disposed = false;
  private closing = false;
  private epoch = 0;
  private operations = new Set<string>();
  private handles = new Map<string, "directory" | "file" | "exec">();
  constructor(
    readonly bridge: Bridge,
    readonly scope: WorkspaceScope,
    private limiter: Limiter = { active: 0, queue: [] },
  ) {}
  fork() {
    const r = new Remote(this.bridge, this.scope, this.limiter);
    r.lease = this.lease;
    return r;
  }
  async call<T>(
    method: string,
    extra: Record<string, unknown> = {},
    operation = true,
  ): Promise<T> {
    if (
      this.disposed ||
      (this.closing &&
        !method.endsWith("close") &&
        !method.endsWith("closeDirectory"))
    )
      throw fault("CANCELED", "文件读取已结束");
    const epoch = this.epoch;
    const lease = this.lease;
    if (!lease) throw fault("CONNECTION_NOT_READY", "请先连接并读取");
    const operationId = `read-${Date.now()}-${++operationSequence}`;
    const params = {
      scope: this.scope,
      leaseId: lease.leaseId,
      connectionGeneration: lease.generation,
      ...extra,
      ...(operation ? { operationId } : {}),
    };
    const invalid = validateRemoteRead("x-methods", method, params);
    if (invalid) throw fault("INVALID_PARAMS", invalid);
    if (operation) {
      if (this.limiter.active >= 4)
        await new Promise<void>((r) => this.limiter.queue.push(r));
      else this.limiter.active++;
      if (this.disposed || this.closing || epoch !== this.epoch) {
        this.releaseSlot();
        throw fault("CANCELED", "文件读取已结束");
      }
      this.operations.add(operationId);
    }
    try {
      let result: T;
      try {
        result = await this.bridge.call<T>(method, params);
      } catch (e) {
        if ((e as { code?: string })?.code !== "RESULT_UNKNOWN" || !operation)
          throw e;
        for (;;) {
          const receipt = await this.bridge.call<{
            state: string;
            result: T;
            error: unknown;
          }>("remote.operations.get", { operationId });
          if (receipt.state === "completed") {
            result = receipt.result;
            break;
          }
          if (receipt.state !== "running") throw receipt.error;
          await sleep(50);
        }
      }
      const invalid = validateRemoteRead("x-results", method, result);
      if (invalid) throw fault("PROTOCOL_ERROR", invalid);
      if (operation && epoch !== this.epoch) {
        const resource = result as Record<string, unknown>;
        for (const [key, close] of [
          ["directoryId", "files.closeDirectory"],
          ["fileId", "files.close"],
          ["execId", "exec.close"],
        ]) {
          if (resource?.[key])
            await this.bridge
              .call(close, {
                scope: this.scope,
                leaseId: lease.leaseId,
                connectionGeneration: lease.generation,
                [key]: resource[key],
              })
              .catch(() => {});
        }
        throw fault("CANCELED", "读取已取消");
      }
      return result;
    } finally {
      if (operation) {
        this.operations.delete(operationId);
        this.releaseSlot();
        await this.bridge
          .call("remote.operations.forget", { operationId })
          .catch(() => {});
      }
    }
  }
  private releaseSlot() {
    const next = this.limiter.queue.shift();
    if (next) next();
    else this.limiter.active--;
  }
  stat(p: RemotePath, followLinks = false) {
    return this.call<Attributes>("files.stat", { path: p, followLinks });
  }
  async openDirectory(p: RemotePath) {
    const r = await this.call<{ directoryId: string; cursor: number }>(
      "files.openDirectory",
      { path: p },
    );
    this.handles.set(r.directoryId, "directory");
    return r;
  }
  async page(directoryId: string, cursor: number) {
    const r = await this.call<FilesReadDirectoryResult>("files.readDirectory", {
      directoryId,
      cursor,
      maxEntries: 256,
    });
    if (r.eof) this.handles.delete(directoryId);
    return r;
  }
  async close(id: string) {
    const kind = this.handles.get(id);
    if (!kind) return;
    this.handles.delete(id);
    await this.call(
      kind === "exec"
        ? "exec.close"
        : kind === "directory"
          ? "files.closeDirectory"
          : "files.close",
      {
        [kind === "exec"
          ? "execId"
          : kind === "directory"
            ? "directoryId"
            : "fileId"]: id,
      },
      false,
    );
  }
  transfer(id:string,target:Remote){
    const kind=this.handles.get(id);
    if(!kind)return;
    if(this.lease?.leaseId!==target.lease?.leaseId||this.scope.workspaceGeneration!==target.scope.workspaceGeneration)throw fault('SOURCE_INVALID','读取句柄不能跨租约转移');
    this.handles.delete(id);target.handles.set(id,kind);
  }
  async readFile(p: RemotePath, limit = 4 * 1024 * 1024): Promise<Uint8Array> {
    const { fileId } = await this.call<{ fileId: string }>("files.openRead", {
      path: p,
    });
    this.handles.set(fileId, "file");
    let offset = "0",
      total = 0;
    const chunks: Uint8Array[] = [];
    try {
      for (;;) {
        const r = await this.call<{
          bytesBase64: string;
          eof: boolean;
          nextOffset: string;
        }>("files.read", { fileId, offset, maxBytes: 65536 });
        const bytes = decode(r.bytesBase64);
        total += bytes.length;
        if (total > limit)
          throw fault("LIMIT_EXCEEDED", "远端数据超过读取上限");
        chunks.push(bytes);
        offset = r.nextOffset;
        if (r.eof) break;
      }
      const result = new Uint8Array(total);
      let at = 0;
      for (const b of chunks) {
        result.set(b, at);
        at += b.length;
      }
      return result;
    } finally {
      await this.close(fileId);
    }
  }
  async exec(command: string, timeoutMs = 15000): Promise<Uint8Array> {
    const { execId } = await this.call<{ execId: string }>("exec.start", {
      command,
      timeoutMs,
      maxOutputBytes: 4 * 1024 * 1024,
    });
    this.handles.set(execId, "exec");
    let afterSequence = 0,
      total = 0;
    const chunks: Uint8Array[] = [];
    try {
      for (;;) {
        const r = await this.call<{
          frames: { sequence: number; stream: string; bytesBase64: string }[];
          lastSequence: number;
          state: string;
          exitCode: number | null;
          reason: string | null;
          truncated: boolean;
        }>("exec.read", { execId, afterSequence, maxBytes: 65536 }, false);
        for (const frame of r.frames) {
          afterSequence = frame.sequence;
          if (frame.stream === "stdout") {
            const bytes = decode(frame.bytesBase64);
            total += bytes.length;
            if (total > 4 * 1024 * 1024)
              throw fault("LIMIT_EXCEEDED", "查询结果超过读取上限");
            chunks.push(bytes);
          }
        }
        await this.call("exec.ack", { execId, sequence: afterSequence }, false);
        if (r.truncated) throw fault("LIMIT_EXCEEDED", "查询结果被截断");
        if (r.state === "failed" || r.state === "canceled")
          throw fault(r.reason ?? "EXEC_FAILED", "远端采集未完成");
        if (r.state === "completed" && afterSequence === r.lastSequence) {
          if (r.exitCode !== 0)
            throw fault("EXEC_FAILED", "服务器未提供所需的只读采集能力");
          break;
        }
        await sleep(15);
      }
      const out = new Uint8Array(total);
      let at = 0;
      for (const b of chunks) {
        out.set(b, at);
        at += b.length;
      }
      return out;
    } finally {
      await this.close(execId);
    }
  }
  async cancel() {
    this.epoch++;
    await Promise.all(
      [...this.operations].map((operationId) =>
        this.bridge.call("operations.cancel", { operationId }).catch(() => {}),
      ),
    );
    await Promise.all(
      [...this.handles.keys()].map((id) => this.close(id).catch(() => {})),
    );
  }
  async dispose() {
    if (this.disposed || this.closing) return;
    this.closing = true;
    await this.cancel();
    this.disposed = true;
    if (this.lease)
      await this.bridge.call("connections.release", {
        leaseId: this.lease.leaseId,
      });
    this.lease = undefined;
  }
}
