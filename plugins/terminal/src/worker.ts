import {diagnosticReference,errorMessage,type DiagnosticRef,isConnectionAuthenticationError} from '@sodeck/sdk';
import {createSettingsProvider} from '@sodeck/sdk/settings-provider';
import {settingsDescriptor,defaultFontSize,defaultShellIntegration,terminalPreferences} from './settings';
import {observation,sampler,stateChanged,reportSessions,errorCode,safeState,updateSessionDiagnostic,releaseSessionDiagnostic} from './observation';
import type {ObservationClient,Invocation,DiagnosticMetadata,Target} from '@sodeck/sdk/observation';
// Business state stays in the plugin process when a terminal surface moves.
import {DirectoryReporter,type ReporterBridge} from './directory-reporter';
import {shellCommand} from './shell-integration';
import {supportsShellIntegration} from './shell-support';
import {BoundedExecClient,Cancellation} from '@sodeck/sdk/bounded-exec';
import {workerClock,type Clock} from '@sodeck/sdk/runtime';
import {openPtyCommand} from '@sodeck/sdk/pty';
import type {WorkspaceScope} from '@sodeck/sdk/remote-read';
declare const __TERMINAL_VERSION__: string;
type Params = Record<string, unknown>;
interface CoreBridge { clock?:Clock; call<T = unknown>(method: string, params?: Params, metadata?:DiagnosticMetadata): Promise<T> }
interface Lease { leaseId: string; connectionId: string; generation: number }
interface Pty { ptyId: string; generation: number; streamId: string; observer?:{observerId:string;cursor:string} }
interface Session extends Lease, Pty {
  sessionId: string; serverId: string; workspaceId: string; title: string;
  status: string; viewId?: string; operationId?: string; error?: string; diagnosticRef?:DiagnosticRef; integrationAvailable?:boolean;
}
interface WorkerDefinition { prepare(context: Params): Promise<void>; activate(context: Params): Promise<void>; invoke(command: string, params: Params, invocation?:Invocation): Promise<unknown>; stop(): Promise<void> }
const runtime = globalThis as typeof globalThis & { sodeck: CoreBridge; sodeckPlugin: WorkerDefinition };
const sessions = new Map<string, Session>();
const sessionTargets=new Map<string,Target[]>();
const sessionAttempts=new Map<string,string>();
const reporters=new Map<string,DirectoryReporter>();
const shellProbeClient=new BoundedExecClient(observation,workerClock(runtime.sodeck),8);
const shellProbeCancellations=new Map<string,Cancellation>();
let shellIntegration=defaultShellIntegration;
const changing = new Set<string>(), operations = new Set<Promise<unknown>>();
const pendingOpens = new Map<string, { canceled: boolean;workspaceId:string }>();
// Cancellation can arrive before the worker starts the corresponding command.
const canceledOperations = new Set<string>();
let sequence = 0, operationSequence = 0, initialized = false, fontSize = defaultFontSize;
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
const fromContext = (ctx: Params): Session => {
  const data = (ctx.pluginData ?? {}) as Params;
  return [...sessions.values()].find(s => s.viewId === ctx.viewId) ?? find(data.sessionId);
};
const snapshot = (session: Session): Session => ({ ...session });
async function mutate<T>(id: string, operation: () => Promise<T>): Promise<T> {
  if (changing.has(id)) throw fault("BUSY", "该终端正在处理上一项操作");
  changing.add(id);
  const pending = operation(); operations.add(pending);
  try { return await pending; } finally { changing.delete(id); operations.delete(pending); }
}
async function publish(session: Session): Promise<void> {
  if (session.viewId && sessions.get(session.sessionId) === session) await runtime.sodeck.call("views.update", {
    viewId: session.viewId, title: session.title,
    displayStatus: session.status === "connecting" || session.status === "waitingAuth" ? "loading" : session.status === "running" ? "running" : session.status === "failed" ? "error" : "ready",
    actions: session.status === "connecting" || session.status === "waitingAuth" || session.status === "running" ? ["terminal.rename"] : ["terminal.rename", "terminal.reconnect"],
  });
}
async function open(params: Params, existing?: Session, inherited:ObservationClient=observation): Promise<Session> {
  let sessionIntegration=shellIntegration;
  if (!existing && sessions.size >= 8) throw fault("CAPACITY", "最多同时打开 8 个终端；关闭一个终端后重试");
  if (existing?.status === "connecting") throw fault("BUSY", "该终端正在连接");
  const serverId = string(params.serverId ?? existing?.serverId, "serverId");
  const workspaceId = string(params.workspaceId ?? existing?.workspaceId ?? serverId, "workspaceId");
  const operationId = params.operationId === undefined ? undefined : string(params.operationId, "operationId");
  const pending = { workspaceId,canceled: operationId ? canceledOperations.has(operationId) : false };
  if (operationId) {
    if (pendingOpens.has(operationId)) throw fault("REQUEST_CONFLICT", "该操作已经在进行中");
    pendingOpens.set(operationId, pending);
  }
  const session: Session = existing ?? {
    sessionId: `terminal-${Date.now()}-${++sequence}`, serverId, workspaceId, title: `终端 ${sequence}`,
    status: "connecting", ptyId: "", streamId: "", leaseId: "", connectionId: "", generation: 0,
  };
  const began=Date.now(),attemptId=`attempt-${++operationSequence}-${began}`,mode=existing?"reconnect":"new",before=session.status;
  const retryOf=sessionAttempts.get(session.sessionId);sessionAttempts.set(session.sessionId,attemptId);updateSessionDiagnostic(session.sessionId,{attemptId,recentErrorCode:undefined,integrationState:sessionIntegration?"starting":"disabled"});
  const attempt={sessionId:session.sessionId,attemptId,mode,previousState:safeState(before),...(retryOf?{retryOf}:{})};
  let diagnostic=await inherited.withTargets(await inherited.targets([{kind:"server",id:serverId},{kind:"workspace",id:workspaceId}])).beginAction("plugin.sodeck.terminal.session.open",attempt);
  void diagnostic.emit("plugin.sodeck.terminal.session.open.started",attempt);
  session.status = "connecting"; stateChanged(session,before,"worker-observed",diagnostic); session.error = undefined;session.diagnosticRef=undefined;session.integrationAvailable=undefined; session.operationId = operationId;
  sessions.set(session.sessionId, session);
  const check = () => {
    if (pending.canceled || sessions.get(session.sessionId) !== session || session.operationId !== operationId) throw fault("CANCELED", "已取消连接");
    if (!initialized) throw fault("PLUGIN_STOPPING", "终端插件正在停止");
  };
  let lease: Lease | undefined, pty: Pty | undefined;
  try {
    check();
    if (!session.viewId) {
      const view = await diagnostic.call<{ viewId: string }>("views.open", {
        workspaceId, viewType: "terminal.session", instanceKey: session.sessionId,
        title: session.title, displayStatus: "loading", actions: ["terminal.rename"],
        runtimeData: { sessionId: session.sessionId }, ...(operationId ? { operationId } : {}),
      });
      session.viewId = view.viewId;
    } else await publish(session);
    check();
    lease = await diagnostic.call<Lease>("connections.acquire", { serverId, workspaceId, ...(operationId ? { operationId } : {}) });
    check();
    if(sessionIntegration){
      const cancellation=new Cancellation();shellProbeCancellations.set(session.sessionId,cancellation);
      try{
        const presentation=await diagnostic.call<{scope:WorkspaceScope}>('views.presentation',{viewId:session.viewId!});
        check();
        await shellProbeClient.retryCleanup();check();
        sessionIntegration=await supportsShellIntegration(shellProbeClient,{scope:presentation.scope,leaseId:lease.leaseId,connectionGeneration:lease.generation,operationId:`shell-probe-${session.sessionId}-${++operationSequence}`},cancellation);
        session.integrationAvailable=sessionIntegration;
        updateSessionDiagnostic(session.sessionId,{integrationState:sessionIntegration?'starting':'unsupported'});
      }finally{shellProbeCancellations.delete(session.sessionId);}
      check();
    }
    const tag=`${Date.now().toString(16)}-${(++operationSequence).toString(16)}-${Math.random().toString(16).slice(2)}-${Math.random().toString(16).slice(2)}`;
    const startup={leaseId:lease.leaseId,cols:100,rows:30,operationId:operationId??`pty-${session.sessionId}-${++operationSequence}`};
    pty = sessionIntegration?await openPtyCommand(diagnostic,{...startup,command:shellCommand(tag),observe:true}):await diagnostic.call<Pty>('pty.open',startup);
    check();
    const refs=await diagnostic.targets([{kind:"server",id:serverId},{kind:"workspace",id:workspaceId},{kind:"view",id:session.viewId!},{kind:"resource",id:lease.connectionId,generation:String(lease.generation)},{kind:"resource",id:pty.ptyId,generation:String(pty.generation)}]);
    sessionTargets.set(session.sessionId,refs);updateSessionDiagnostic(session.sessionId,{resourceRefs:refs.filter(t=>t.kind==="resource").map(t=>t.ref)});diagnostic=diagnostic.withTargets(refs);
    const previous=session.status;Object.assign(session, lease, pty, { status: "running" });stateChanged(session,previous,"core-response",diagnostic);
    await diagnostic.call("views.bindResources", { viewId: session.viewId, ...(operationId ? { operationId } : {}), resources: [
      { kind: "pty", id: pty.ptyId, generation: pty.generation }, { kind: "lease", id: lease.leaseId },
    ] });
    check();
    await publish(session);
    // Reporting is an enhancement: failure cannot tear down a usable terminal.
    if(typeof (runtime.sodeck as ReporterBridge).onNotification==='function') {
      try {
        const presentation=await diagnostic.call<{scope:WorkspaceScope}>('views.presentation',{viewId:session.viewId});
        const reporter=new DirectoryReporter(runtime.sodeck as ReporterBridge,presentation.scope,serverId,session.viewId!,pty,tag,observation.withTargets(sessionTargets.get(session.sessionId)??[]),session.sessionId,(state,directoryRevision,recentErrorCode)=>{updateSessionDiagnostic(session.sessionId,{integrationState:state,directoryRevision,...(recentErrorCode?{recentErrorCode}:{})});reportSessions(sessions.values(),pendingOpens.values(),shellIntegration);});
        reporters.set(session.sessionId,reporter);
        void reporter.start().catch(()=>reporter.dispose());
      }catch{/* Core capability/version errors remain visible through unknown directory state. */}
    }
    void diagnostic.emit("plugin.sodeck.terminal.session.open.completed",{...attempt,durationMs:Date.now()-began},{outcome:"succeeded"});
    void diagnostic.end("succeeded");sampler.metric(mode==="new"?"session.open.duration":"session.reconnect.duration",workspaceId,"histogram","ms",Date.now()-began);sampler.metric("session.open.result",workspaceId,"counter","count",1,{outcome:"succeeded"});
    return snapshot(session);
  } catch (error) {
    let cleanupError: unknown;
    if (pty) try { await diagnostic.call("pty.close", { ptyId: pty.ptyId, generation: pty.generation }); } catch (failure) { cleanupError = failure; }
    if (lease) try { await diagnostic.call("connections.release", { leaseId: lease.leaseId }); } catch (failure) { cleanupError ??= failure; }
    if (sessions.get(session.sessionId) === session && session.operationId === operationId) {
      Object.assign(session, { ptyId: "", streamId: "", leaseId: "", connectionId: "", generation: 0 });
      const code = (error as {code?:string})?.code;
      const previous=session.status;session.status = pending.canceled || code === "CANCELED" ? "canceled" : isConnectionAuthenticationError(code ?? "") ? "waitingAuth" : "failed";
      updateSessionDiagnostic(session.sessionId,{recentErrorCode:errorCode(error)});stateChanged(session,previous,"core-response",diagnostic);session.error = errorMessage(error);session.diagnosticRef=diagnosticReference(error);
      if (!session.viewId) {sessions.delete(session.sessionId);sessionAttempts.delete(session.sessionId);releaseSessionDiagnostic(session.sessionId);}
      else {
        if (pty) await diagnostic.call("views.bindResources", {viewId:session.viewId,resources:[]}).catch(() => {});
        await publish(session).catch(() => {}); // Closing can remove this page during resource cleanup.
      }
    }
    const outcome=session.status==="canceled"?"canceled":session.status==="waitingAuth"?"unknown":"failed";
    void diagnostic.emit("plugin.sodeck.terminal.session.open.completed",{...attempt,durationMs:Date.now()-began},{outcome,errorCode:errorCode(error)});void diagnostic.end(outcome,{},errorCode(error));sampler.metric("session.open.result",workspaceId,"counter","count",1,{outcome});
    if(cleanupError)void diagnostic.emit("plugin.sodeck.terminal.session.cleanup.failed",{sessionId:session.sessionId,originalCode:errorCode(error),cleanupCode:errorCode(cleanupError)},{outcome:"failed"});
    if (cleanupError) throw { ...fault("CLEANUP_REQUIRED", "连接结束但资源未完全释放，请在核心资源页检查"), details: { leaseId: lease?.leaseId, ptyId: pty?.ptyId, originalCode: (error as { code?: string })?.code, cleanupCode: (cleanupError as {code?:string})?.code } };
    throw error;
  } finally {
    if (operationId) pendingOpens.delete(operationId);reportSessions(sessions.values(),pendingOpens.values(),shellIntegration);
  }
}
async function close(session: Session, inherited:ObservationClient=observation, reason="user"): Promise<void> {
  shellProbeCancellations.get(session.sessionId)?.cancel();
  const diagnostic=await inherited.withTargets(sessionTargets.get(session.sessionId)??[]).beginAction("plugin.sodeck.terminal.session.close",{sessionId:session.sessionId,reason});
  const began=Date.now();void diagnostic.emit("plugin.sodeck.terminal.session.close.started",{sessionId:session.sessionId,reason});
  try {
  const reporter=reporters.get(session.sessionId);reporters.delete(session.sessionId);await reporter?.dispose();
  if (session.ptyId) await diagnostic.call("pty.close", { ptyId: session.ptyId, generation: session.generation });
  if (session.leaseId) await diagnostic.call("connections.release", { leaseId: session.leaseId });
  const previous=session.status;Object.assign(session, { status: "ended", ptyId: "", streamId: "", leaseId: "", connectionId: "", generation: 0 });stateChanged(session,previous,"core-response",diagnostic);
  void diagnostic.emit("plugin.sodeck.terminal.session.close.completed",{sessionId:session.sessionId,reason,durationMs:Date.now()-began},{outcome:"succeeded"});void diagnostic.end("succeeded");
  }catch(error){void diagnostic.emit("plugin.sodeck.terminal.session.close.completed",{sessionId:session.sessionId,reason,durationMs:Date.now()-began},{outcome:"failed",errorCode:errorCode(error)});void diagnostic.end("failed",{},errorCode(error));throw error;}finally{reportSessions(sessions.values(),pendingOpens.values(),shellIntegration);}
}
async function cancel(operationId: string, diagnostic:ObservationClient=observation): Promise<unknown> {
  canceledOperations.add(operationId);
  if (canceledOperations.size > 10000) canceledOperations.delete(canceledOperations.values().next().value!);
  const pending = pendingOpens.get(operationId);
  if (pending) pending.canceled = true;
  for(const session of sessions.values())if(session.operationId===operationId)shellProbeCancellations.get(session.sessionId)?.cancel();
  for (const session of sessions.values()) if (session.operationId === operationId && session.status === "waitingAuth") {
    const previous=session.status;session.status = "canceled";stateChanged(session,previous,"worker-observed",diagnostic); session.error = "已取消连接"; await publish(session);
  }
  void diagnostic.emit("plugin.sodeck.terminal.session.cancel.requested",{operationId});
  try{const result=await diagnostic.call("operations.cancel", { operationId });void diagnostic.emit("plugin.sodeck.terminal.session.cancel.completed",{operationId},{outcome:"succeeded"});return result;}
  catch(error){void diagnostic.emit("plugin.sodeck.terminal.session.cancel.completed",{operationId},{outcome:"failed",errorCode:errorCode(error)});throw error;}
}
async function refreshStatuses(): Promise<void> {
  const resources = await runtime.sodeck.call<{ ptys: { ptyId?: string; id?: string; state?: string; status?: string }[] }>("resources.list");
  for (const session of sessions.values()) {
    if (!session.ptyId) continue;
    const resource = resources.ptys.find(item => (item.ptyId ?? item.id) === session.ptyId);
    const previous=session.status;session.status = resource?.state ?? resource?.status ?? "ended";stateChanged(session,previous,"core-response",observation.withTargets(sessionTargets.get(session.sessionId)??[]));
  }
}
const settingsProvider=createSettingsProvider({pluginId:'sodeck.terminal',descriptor:settingsDescriptor,bridge:{call:(method,params)=>runtime.sodeck.call(method,params as Params)},apply:(_group,snapshot)=>{const next=terminalPreferences(snapshot);fontSize=next.fontSize;shellIntegration=next.shellIntegration;}});
runtime.sodeckPlugin = {
  async prepare() {
    try{await settingsProvider.initialize();}catch(error){
      // A broken optional settings contribution must not disable SSH terminals.
      // Keep declared defaults in memory; the settings page remains unavailable.
      if(!['SETTINGS_UNAVAILABLE','STORAGE_ERROR','READ_ONLY','CONFLICT','CAPACITY'].includes(errorCode(error)))throw error;
    }
  },
  async activate() { initialized = true;reportSessions(sessions.values(),[],shellIntegration); },
  async invoke(command, params = {}, invocation) {
    const diagnostic=observation.withInvocation(invocation);
    if (!initialized) throw fault("PLUGIN_NOT_READY", "终端插件尚未就绪");
    if(command.startsWith('settings.provider.'))return settingsProvider.invoke(command,params);
    switch (command) {
      case "command.execute": {
        const ctx = (params.context ?? {}) as Params, args = (params.args ?? {}) as Params;
        const data = (ctx.pluginData ?? {}) as Params;
        switch (params.commandId) {
          case "terminal.new": {
            const existing = ctx.viewId ? fromContext(ctx) : undefined;
            return mutate(existing?.sessionId ?? `opening-${++operationSequence}`, () => open({ ...args, serverId: ctx.serverId, workspaceId: ctx.workspaceId, operationId: ctx.operationId }, existing,diagnostic));
          }
          case "terminal.rename": return runtime.sodeckPlugin.invoke("sessions.rename", { sessionId: fromContext(ctx).sessionId, title: args.title },invocation);
          case "terminal.reconnect": {
            const session = fromContext(ctx);
            if (session.status === "connecting") throw fault("BUSY", "该终端正在连接");
            return mutate(session.sessionId, async () => { await close(session,diagnostic); return open({ operationId: ctx.operationId }, session,diagnostic); });
          }
          default: throw fault("METHOD_NOT_FOUND", "终端未声明此命令");
        }
      }
      case "lifecycle.initialize": {
        if(params.viewType==="terminal.sessions") {await refreshStatuses();return {sessions:[...sessions.values()].filter(s=>s.workspaceId===params.workspaceId).map(snapshot)};}
        const data=(params.pluginData??{}) as Params;
        return {fontSize,shellIntegration,session:snapshot(fromContext(params))};
      }
      case "lifecycle.prepareClose": return { allow: true };
      case "lifecycle.restore": {
        const sessionId = string(params.instanceKey || params.viewId, "instanceKey");
        const viewId = string(params.viewId, "viewId");
        sessions.set(sessionId, { sessionId, viewId, serverId: string(params.serverId, "serverId"), workspaceId: string(params.workspaceId, "workspaceId"), title: string(params.title, "title"), status: "ended", ptyId: "", streamId: "", leaseId: "", connectionId: "", generation: 0 });
        return { displayStatus: "ended", actions: ["terminal.rename", "terminal.reconnect"] };
      }
      case "sessions.status": {
        const session = find(params.sessionId);
        if (session.ptyId !== params.ptyId) return { updated: false };
        const state = string(params.status, "status");
        const previous=session.status;session.status = state;stateChanged(session,previous,"ui-reported",diagnostic);
        const live = ["active", "running", "connected"].includes(state);
        if (session.viewId) await runtime.sodeck.call("views.update", {viewId: session.viewId, displayStatus: live ? "running" : "ready", actions: live ? ["terminal.rename"] : ["terminal.rename", "terminal.reconnect"]});
        return { updated: true };
      }
      case "lifecycle.close": {
        const session = [...sessions.values()].find(item => item.viewId === params.viewId);
        if (session) {
          sessions.delete(session.sessionId);sessionAttempts.delete(session.sessionId);releaseSessionDiagnostic(session.sessionId);
          if (session.operationId) await cancel(session.operationId,diagnostic);
          await close(session,diagnostic);
        }
        return { closed: true };
      }
      case "sessions.open": case "sessions.create": return mutate(`opening-${++operationSequence}`, () => open(params,undefined,diagnostic));
      case "sessions.cancelOpen": case "lifecycle.cancel": {
        const operationId = string(params.operationId, "operationId");
        return cancel(operationId,diagnostic);
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
        return mutate(session.sessionId, async () => {
          if (session.viewId) await runtime.sodeck.call("views.update", { viewId: session.viewId, title });
          session.title = title;void diagnostic.emit("plugin.sodeck.terminal.session.metadata.changed",{sessionId:session.sessionId,changedField:"title"}); return snapshot(session);
        });
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
        const session = find(params.sessionId); return mutate(session.sessionId, async () => { await close(session,diagnostic); return open({}, session,diagnostic); });
      }
      case "settings.get": return { fontSize,shellIntegration };
      default: throw fault("METHOD_NOT_FOUND", "终端插件未提供此操作");
    }
  },
  async stop() {
    initialized = false;
    for(const cancellation of shellProbeCancellations.values())cancellation.cancel();
    await shellProbeClient.dispose();
    await Promise.allSettled([...pendingOpens.keys()].map(id=>cancel(id)));
    await Promise.allSettled([...operations]);
    await Promise.allSettled([...sessions.values()].map(session=>close(session,observation,"app-exit")));sessions.clear();sessionTargets.clear();
    sampler.stop();await observation.stop();
  },
};

export {};
