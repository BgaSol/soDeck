export interface RpcRequest { apiVersion: 1; requestId: string; method: string; params: unknown; timeoutMs?: number }
export interface RpcFault { code: string; message: string; details?: unknown; operationId?: string; phase?: string }
export interface RpcResponse<T = unknown> { requestId: string; result?: T; error?: RpcFault }
export interface CoreNotification { type: string; payload?: unknown }
export interface ViewContext {
  viewId?: string; viewType?: string; workspaceId?: string; serverId?: string; serverName?: string;
  pluginVersion?: string; mountGeneration?: number; ptyId?: string; generation?: number;
  detachedViewId?: string; sessionId?: string; snapshot?: unknown;
}
export class CoreError extends Error {
  readonly code: string;
  readonly details?: unknown;
  readonly operationId?: string;
  constructor(fault: RpcFault) { super(fault.message); this.name = "CoreError"; this.code = fault.code; this.details = fault.details; this.operationId = fault.operationId; }
}

declare global {
  interface Window {
    ipc?: { postMessage(message: string): void };
    __TAURI_INTERNALS__?: unknown;
    __SODECK_CONTEXT__?: ViewContext;
    __sodeckReceive?: (response: RpcResponse) => void;
    __sodeckNotify?: (notification: CoreNotification) => void;
  }
}

export type Transport = (request: RpcRequest) => Promise<RpcResponse>;
export function createCore(transport: Transport) {
  return {
    async call<T = unknown>(method: string, params: unknown = {}, timeoutMs = 35_000): Promise<T> {
      const request: RpcRequest = { apiVersion: 1, requestId: crypto.randomUUID(), method, params, timeoutMs };
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const response = await Promise.race([
          transport(request),
          new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new CoreError({ code: "RESULT_UNKNOWN", message: "尚未取得操作结果，请查询状态后再继续。", operationId: request.requestId })), timeoutMs); }),
        ]);
        if (response.requestId !== request.requestId) throw new CoreError({ code: "PROTOCOL_ERROR", message: "核心响应与当前请求不匹配" });
        if (response.error) throw new CoreError(response.error);
        return response.result as T;
      } finally { if (timer !== undefined) clearTimeout(timer); }
    },
  };
}

const pending = new Map<string, { resolve: (response: RpcResponse) => void; timer: ReturnType<typeof setTimeout> }>();
const listeners = new Set<(notification: CoreNotification) => void>();
let bridgeInstalled = false;
function installBridge() {
  if (bridgeInstalled || typeof window === "undefined") return;
  bridgeInstalled = true;
  window.__sodeckReceive = response => {
    const item = pending.get(response.requestId);
    if (!item) return;
    clearTimeout(item.timer); pending.delete(response.requestId); item.resolve(response);
  };
  window.__sodeckNotify = notification => { for (const listener of listeners) listener(notification); };
}
const nativeTransport: Transport = async request => {
  installBridge();
  if (window.__TAURI_INTERNALS__) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<RpcResponse>("rpc", { request });
  }
  if (window.ipc) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => pending.delete(request.requestId), request.timeoutMs ?? 35_000);
      pending.set(request.requestId, { resolve, timer });
      try { window.ipc!.postMessage(JSON.stringify(request)); }
      catch (error) { pending.delete(request.requestId); clearTimeout(timer); reject(error); }
    });
  }
  throw new CoreError({ code: "NATIVE_BRIDGE_UNAVAILABLE", message: "请从 SoDeck 桌面程序打开。浏览器预览无法建立 SSH 连接。" });
};

export const core = createCore(nativeTransport);
export function context(): Readonly<ViewContext> { return window.__SODECK_CONTEXT__ ?? {}; }
export function onCoreNotification(listener: (notification: CoreNotification) => void): () => void {
  installBridge(); listeners.add(listener); return () => listeners.delete(listener);
}
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string") return error.message;
  return "操作未完成，请重试或查看调用记录。";
}
export function encodeBytes(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 8192) binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
  return btoa(binary);
}
export function decodeBytes(encoded: string): Uint8Array {
  return Uint8Array.from(atob(encoded), char => char.charCodeAt(0));
}
