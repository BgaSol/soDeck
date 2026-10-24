// Business state stays in the plugin process when a terminal surface moves.
declare const __TERMINAL_VERSION__: string;
type Params = Record<string, unknown>;
interface CoreBridge { call<T = unknown>(method: string, params?: Params): Promise<T> }
interface Lease { leaseId: string; connectionId: string; generation: number }
interface Pty { ptyId: string; generation: number; streamId: string }
interface Session extends Lease, Pty {
  sessionId: string; serverId: string; workspaceId: string; title: string;
  status: string; viewId?: string;
}
interface WorkerDefinition { prepare(context: Params): Promise<void>; activate(context: Params): Promise<void>; invoke(command: string, params: Params): Promise<unknown>; stop(): Promise<void> }
const runtime = globalThis as typeof globalThis & { sodeck: CoreBridge; sodeckPlugin: WorkerDefinition };
const sessions = new Map<string, Session>();
const changing = new Set<string>(), operations = new Set<Promise<unknown>>();
const pendingOpens = new Map<string, { canceled: boolean }>();
let sequence = 0, operationSequence = 0, initialized = false, fontSize = 14, opening = 0;
const fault = (code: string, message: string) => ({ code, message });
const string = (value: unknown, name: string): string => {
  if (typeof value !== "string" || !value || value.length > 256) throw fault("INVALID_PARAMS", `${name} 无效`);
  return value;
};
const find = (id: unknown): Session => {
  const session = sessions.get(string(id, "sessionId"));
  if (!session) throw fault("SESSION_ENDED", "会话已经结束，请明确新建连接");
  return session;
};
const snapshot = (session: Session): Session => ({ ...session });
async function mutate<T>(id: string, operation: () => Promise<T>): Promise<T> {
  if (changing.has(id)) throw fault("BUSY", "该终端正在处理上一项操作");
  changing.add(id);
  const pending = operation(); operations.add(pending);
  try { return await pending; } finally { changing.delete(id); operations.delete(pending); }
}
async function open(params: Params, existing?: Session): Promise<Session> {
  if (!existing && sessions.size + opening >= 8) throw fault("CAPACITY", "最多同时打开 8 个终端");
  const serverId = string(params.serverId ?? existing?.serverId, "serverId");
  const workspaceId = string(params.workspaceId ?? existing?.workspaceId ?? serverId, "workspaceId");
  const operationId = params.operationId === undefined ? undefined : string(params.operationId, "operationId");
  const pending = { canceled: false };
  if (operationId) {
    if (pendingOpens.has(operationId)) throw fault("REQUEST_CONFLICT", "该新建操作已经在进行中");
    pendingOpens.set(operationId, pending);
  }
  opening++;
  let lease: Lease | undefined, pty: Pty | undefined;
  try {
    lease = await runtime.sodeck.call<Lease>("connections.acquire", { serverId, workspaceId, ...(operationId ? { operationId } : {}) });
    if (pending.canceled) throw fault("CANCELED", "已取消连接");
    pty = await runtime.sodeck.call<Pty>("pty.open", { leaseId: lease.leaseId, cols: 100, rows: 30, ...(operationId ? { operationId } : {}) });
    const session: Session = {
      ...lease, ...pty, sessionId: existing?.sessionId ?? `terminal-${Date.now()}-${++sequence}`,
      serverId, workspaceId, title: existing?.title ?? `终端 ${sequence}`, status: "running",
      ...(existing?.viewId ? { viewId: existing.viewId } : {}),
    };
    if (pending.canceled) throw fault("CANCELED", "已取消连接");
    if (!initialized) throw fault("PLUGIN_STOPPING", "终端插件正在停止");
    sessions.set(session.sessionId, session); return snapshot(session);
  } catch (error) {
    let cleanupError: unknown;
    if (pty) try { await runtime.sodeck.call("pty.close", { ptyId: pty.ptyId, generation: pty.generation }); } catch (failure) { cleanupError = failure; }
    if (lease) try { await runtime.sodeck.call("connections.release", { leaseId: lease.leaseId }); } catch (failure) { cleanupError ??= failure; }
    if (cleanupError) throw { ...fault("CLEANUP_REQUIRED", "创建终端结束但资源未完全释放，请在核心资源页检查"), details: { leaseId: lease?.leaseId, ptyId: pty?.ptyId, originalCode: (error as { code?: string })?.code, cleanupCode: (cleanupError as { code?: string })?.code } };
    throw error;
  } finally {
    opening--;
    if (operationId) pendingOpens.delete(operationId);
  }
}
async function close(session: Session): Promise<void> {
  await runtime.sodeck.call("pty.close", { ptyId: session.ptyId, generation: session.generation });
  session.status = "ended";
  await runtime.sodeck.call("connections.release", { leaseId: session.leaseId });
}
async function refreshStatuses(): Promise<void> {
  const resources = await runtime.sodeck.call<{ ptys: { ptyId?: string; id?: string; state?: string; status?: string }[] }>("resources.list");
  for (const session of sessions.values()) {
    const resource = resources.ptys.find(item => (item.ptyId ?? item.id) === session.ptyId);
    session.status = resource?.state ?? resource?.status ?? "ended";
  }
}
runtime.sodeckPlugin = {
  async prepare() {
    const setting = await runtime.sodeck.call<unknown>("storage.get", { key: "fontSize" });
    if (typeof setting === "number" && Number.isInteger(setting) && setting >= 10 && setting <= 24) fontSize = setting;
  },
  async activate() { initialized = true; },
  async invoke(command, params = {}) {
    if (!initialized) throw fault("PLUGIN_NOT_READY", "终端插件尚未就绪");
    switch (command) {
      case "sessions.open": case "sessions.create": return mutate(`opening-${++operationSequence}`, () => open(params));
      case "sessions.cancelOpen": {
        const operationId = string(params.operationId, "operationId");
        const pending = pendingOpens.get(operationId);
        if (pending) pending.canceled = true;
        return runtime.sodeck.call("operations.cancel", { operationId });
      }
      case "sessions.get": {
        const session = find(params.sessionId);
        if (params.viewId) session.viewId = string(params.viewId, "viewId");
        return snapshot(session);
      }
      case "sessions.bind": {
        const session = find(params.sessionId); session.viewId = string(params.viewId, "viewId"); return snapshot(session);
      }
      case "sessions.rename": {
        const session = find(params.sessionId);
        const title = typeof params.title === "string" ? params.title.trim() : "";
        if (!title || [...title].length > 80 || /[\u0000-\u001f\u007f-\u009f]/.test(title)) throw fault("INVALID_PARAMS", "标题须为 1–80 个字符，且不含控制字符");
        return mutate(session.sessionId, async () => { session.title = title; return snapshot(session); });
      }
      case "sessions.list":
        await refreshStatuses();
        return [...sessions.values()].filter(s => !params.workspaceId || s.workspaceId === params.workspaceId).map(snapshot);
      case "sessions.close": {
        const id = string(params.sessionId, "sessionId"), session = sessions.get(id);
        if (session) await mutate(id, async () => { await close(session); sessions.delete(id); });
        return { closed: true };
      }
      case "sessions.reconnect": {
        const session = find(params.sessionId); return mutate(session.sessionId, async () => { await close(session); return open({}, session); });
      }
      case "settings.get": return { fontSize };
      case "settings.set": {
        const value = params.fontSize;
        if (typeof value !== "number" || !Number.isInteger(value) || value < 10 || value > 24) throw fault("INVALID_PARAMS", "字号须为 10–24 的整数");
        await runtime.sodeck.call("storage.set", { key: "fontSize", value }); fontSize = value; return { fontSize };
      }
      case "settings.reset": {
        if (__TERMINAL_VERSION__ === "0.1.0") throw fault("METHOD_NOT_FOUND", "当前终端版本不提供恢复默认字号");
        await runtime.sodeck.call("storage.set", { key: "fontSize", value: 14 }); fontSize = 14; return { fontSize };
      }
      default: throw fault("METHOD_NOT_FOUND", "终端插件未提供此操作");
    }
  },
  async stop() {
    initialized = false;
    await Promise.allSettled([...operations]);
    for (const session of sessions.values()) { await close(session); sessions.delete(session.sessionId); }
  },
};

export {};
