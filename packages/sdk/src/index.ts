import {validateWorkbenchUi} from './workbench-ui';
export {openDialog,dialogContext,readyDialog,completeDialog,closeDialog,onDialogCancel} from './dialogs';
export type {WorkbenchContributionsV2,PluginMetadata,ToolContribution,TabCreateContribution,DialogResult} from './workbench-ui';
import {validateDirectory} from './directory';
import {validateCatalog} from './server-catalog';
export type {Group,CatalogItem,CatalogPage,GroupsPage,CatalogSave,OperationResult} from './server-catalog';
import {validateNetwork} from './network';
import {validateConnectionRoute} from './connection-route';
export {connectionCommandTimeout} from './connection-timeout';
export {decodeRouteStatus,decodeRouteStatusV2} from './connection-route';
export type {ConnectionRoute,RouteServer,RouteSummary,RouteStatus} from './connection-route.generated';
export type {RouteMeta,RouteReference,NodePage,ServerPage,RouteStatusV2,ResourcePage,ResourceRouteV2,DependencyPage,SegmentPage} from './connection-route-v2.generated';
import {validatePty} from './pty';
import { validateWorkbenchRequest,validateWorkbenchResult,validateWorkbenchEvent } from "./workbench";
import { validateRemoteRead } from './remote-read';
import { validateState } from './state';
export {observeState,presentation} from './state';
export type {StateSubscription,StateBridge} from './state';
export * as remoteReadContract from './remote-read';
export * as workbenchContract from "./workbench";
export { initializeTheme, applyTheme, showMenu, currentTheme, subscribeTheme, onThemeError, retryTheme } from './presentation';
export type { ThemeResult, LiveTheme, MenuItem, ThemePreference, ThemeCatalog, ThemeCatalogEntry } from './presentation';
export interface RpcRequest { apiVersion: 1; requestId: string; method: string; params: unknown; timeoutMs?: number }
export interface RpcFault { code: string; message: string; details?: unknown; operationId?: string; phase?: string }
export interface RpcResponse<T = unknown> { requestId: string; result?: T; error?: RpcFault }
export interface CoreNotification { type: string; payload?: unknown }
export interface ViewContext {
  dialog?:import('./dialogs').DialogContext;surfaceWindowId?:string;
  pluginId?: string; workspaceGeneration?: string;
  theme?: import('./workbench.generated').ThemeResponse;
  viewId?: string; viewType?: string; workspaceId?: string; serverId?: string; serverName?: string;
  pluginVersion?: string; mountGeneration?: number; windowId?: string;
  detachedViewId?: string; pluginData?: Record<string, unknown>; pluginState?: unknown; snapshot?: unknown;
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
      const invalid = validateWorkbenchUi('x-methods',method,params) ?? validateCatalog('x-methods',method,params) ?? validateNetwork('x-methods',method,params) ?? validateConnectionRoute('x-methods',method,params) ?? validateWorkbenchRequest(method, params) ?? validateRemoteRead('x-methods',method,params) ?? validateState('x-methods',method,params) ?? validateDirectory('x-methods',method,params) ?? validatePty('x-methods',method,params);
      if (invalid) throw new CoreError({ code: "INVALID_PARAMS", message: invalid });
      const request: RpcRequest = { apiVersion: 1, requestId: crypto.randomUUID(), method, params, timeoutMs };
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const response = await Promise.race([
          transport(request),
          new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new CoreError({ code: "RESULT_UNKNOWN", message: "尚未取得操作结果，请查询状态后再继续。", operationId: request.requestId })), timeoutMs); }),
        ]);
        if (response.requestId !== request.requestId) throw new CoreError({ code: "PROTOCOL_ERROR", message: "核心响应与当前请求不匹配" });
        if (response.error) throw new CoreError(response.error);
        const invalidResult=validateWorkbenchUi('x-results',method,response.result) ?? validateCatalog('x-results',method,response.result) ?? validateNetwork('x-results',method,response.result) ?? validateConnectionRoute('x-results',method,response.result) ?? validateWorkbenchResult(method,response.result) ?? validateRemoteRead('x-results',method,response.result) ?? validateState('x-results',method,response.result) ?? validateDirectory('x-results',method,response.result) ?? validatePty('x-results',method,response.result);
        if(invalidResult)throw new CoreError({code:'PROTOCOL_ERROR',message:invalidResult,details:response.result});
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
  window.__sodeckNotify = notification => {
    const invalid=validateWorkbenchUi('x-events',notification.type,notification.payload) ?? validateWorkbenchEvent(notification.type,notification.payload) ?? validateState('x-events',notification.type,notification.payload);
    if(invalid){console.error(invalid);return;}
    for (const listener of listeners) listener(notification);
  };
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
