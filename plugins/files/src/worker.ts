import {observation,sampler,configureObservation} from './observation';
import type {Invocation} from '@sodeck/sdk/observation';
import {workerClient} from '@sodeck/sdk/worker';
import { Explorer } from "./state";
import {FollowController} from './follow';
import type {StateBridge} from '@sodeck/sdk/state';
import { path, type RemotePath } from "./remote-path";
import { fault, type Bridge, type WorkspaceScope } from "./remote";
type Params = Record<string, unknown>;
const runtime = globalThis as typeof globalThis & {
  sodeck: StateBridge;
  sodeckPlugin: unknown;
};
const bridge=workerClient(runtime.sodeck);configureObservation(bridge);
const states = new Map<string, Explorer>();
const follows=new Map<string,FollowController>();
function follow(s:Explorer){return follows.get(s.scope.workspaceGeneration)!;}
const showHidden = true;
function state(p: Params): Explorer {
  const s = states.get(String(p.viewId));
  if (!s || s.disposed) throw fault("STALE_WORKSPACE", "文件视图已关闭");
  return s;
}
function initialize(p: Params) {
  const viewId = String(p.viewId);
  let s = states.get(viewId);
  if (!s) {
    if (
      typeof p.workspaceGeneration !== "string" ||
      typeof p.workspaceId !== "string"
    )
      throw fault("INVALID_PARAMS", "缺少工作区创建身份");
    s = new Explorer(
      bridge,
      {
        workspaceId: p.workspaceId,
        workspaceGeneration: p.workspaceGeneration,
      },
      String(p.serverId),
      viewId,
      () => [...states.values()].reduce((n, s) => n + s.cacheBytes(), 0),
    );
    s.showHidden = showHidden;
    states.set(viewId, s);
    let controller=follows.get(s.scope.workspaceGeneration);
    if(!controller){controller=new FollowController(bridge);follows.set(s.scope.workspaceGeneration,controller);}
    const owner=controller;s.followSnapshot=()=>owner.snapshot();s.followDiagnostics=()=>owner.observationSnapshot();
    void controller.attach(s).catch(()=>{});
  }
  return s;
}
runtime.sodeckPlugin = {
  async prepare() {
    bridge.onNotification(event=>{
      if(event.type!=="workspace.closed")return;
      const generation=(event.payload as {workspaceGeneration:string}).workspaceGeneration;
      const controller=follows.get(generation);follows.delete(generation);void controller?.dispose();
    });

  },
  activate() {},
  async invoke(command: string, p: Params, invocation?:Invocation) {
    const diagnostic=observation?.withInvocation(invocation);
    if (command === "lifecycle.initialize") return initialize(p).snapshot();
    if (command === "lifecycle.prepareClose") return { allow: true };
    if (command === "lifecycle.presentation") {
      const s = states.get(String(p.viewId));
      if (s) await s.presentation(p.visible === true, Number(p.revision));
      return { accepted: true };
    }
    if (command === "lifecycle.close") {
      const s = states.get(String(p.viewId));
      states.delete(String(p.viewId));
      if(s)await follow(s).detach();
      await s?.dispose();
      return { closed: true };
    }
    if (command === "lifecycle.cancel") {
      await runtime.sodeck.call("operations.cancel", {
        operationId: p.operationId,
      });
      return { canceled: true };
    }
    if (command === "command.execute") {
      const ctx = p.context as Params;
      let s = ctx.viewId
        ? state(ctx)
        : [...states.values()].find(
            (s) =>
              s.scope.workspaceId === ctx.workspaceId &&
              s.scope.workspaceGeneration === ctx.workspaceGeneration,
          );
      if(!s){const opened=await bridge.call<{viewId:string}>('views.open',{viewType:'files.explorer',workspaceId:ctx.workspaceId,operationId:ctx.operationId,title:'文件'});s=initialize({...ctx,viewId:opened.viewId});}
      await s.connect(String(ctx.operationId),diagnostic);
      return { viewId: s.viewId };
    }
    const s = state(p);
    const controller=follow(s);
    if(['files.select','files.toggle','files.locate','files.refresh'].includes(command)){controller.manual();s.manualNavigation();}
    switch (command) {
      case "files.snapshot":
        void controller.tick();
        return s.snapshot(Number(p.startUnit)||0,Number(p.countUnits)||100,Number(p.overscanUnits)||0,typeof p.anchorId==='string'?p.anchorId:undefined,Number(p.anchorOffset)||0,p.continuation===true);
      case 'follow.setEnabled':await controller.setEnabled(p.enabled===true,diagnostic);break;
      case 'follow.locateOnce':controller.locateOnce(diagnostic);break;
      case 'follow.interaction':controller.interactionState(p.active===true,p.submitted===true);break;
      case "files.diagnostics":
        return {
          follow:controller.diagnostics(),
          workspaceControllers:follows.size,
          nodes: s.nodes.size,
          capacities:s.capacities.diagnostics(),
          cacheBytes: s.cacheBytes(),
          snapshotTiming: s.snapshotTiming,
        };
      case "files.presentation": {
        const presentation = await runtime.sodeck.call<{
          visible: boolean;
          revision: number;
        }>("views.presentation", { viewId: s.viewId });
        await controller.setVisible(presentation.visible, presentation.revision);
        return s.snapshot();
      }
      case "files.toggle":
        s.toggle(String(p.id),diagnostic);
        break;
      case "files.select":
        await s.schedule(() => s.select(String(p.id)),'select',diagnostic);
        break;
      case "files.more":
        await s.schedule(client => s.load(String(p.id),false,client),"readMore",diagnostic);
        break;
      case "files.refresh":
        await s.refresh(typeof p.id === "string" ? p.id : undefined,diagnostic);
        break;
      case "files.locate": {
        const target =
          typeof p.text === "string" ? path(p.text) : (p.path as RemotePath);
        if (!target?.bytesBase64) throw fault("INVALID_PARAMS", "路径无效");
        await s.schedule(client => s.locate(target,client),"locate",diagnostic);
        break;
      }
      case 'files.linkTarget':return s.linkTarget(String(p.id));
      case 'files.row':return p.unit===undefined?s.rowAt(Number(p.index)||0):s.rowAtUnit(Number(p.unit)||0);
      case "files.hidden":return {accepted:true,showHidden:true};
      case "files.scroll":
        s.scrollTop = Math.max(0, Number(p.top) || 0);
        break;
      case "files.cancel":
        await controller.cancel();
        await s.cancel();
        break;
      default:
        throw fault("UNSUPPORTED_API", "未知文件操作");
    }
    return { accepted: true, revision: s.revision };
  },
  async stop() {
    await Promise.all([...follows.values()].map(s=>s.dispose()));follows.clear();
    await Promise.all([...states.values()].map((s) => s.dispose()));
    states.clear();
    sampler?.stop();await observation?.stop();
  },
};
