import {configureObservation,observation,sampler,releaseRun} from './observation';
import type {Invocation} from '@sodeck/sdk/observation';
import {Editors} from './editor-sessions';
import {workerClient} from '@sodeck/sdk/worker';
import {NetworkStreams,type BinaryTransport} from '@sodeck/sdk/network-stream';
import type {StateBridge} from '@sodeck/sdk/state';
import {Rules} from './rules';
import {Manager} from './manager';
import {fault,validHost} from './protocol/address';
import {networkBridge} from './bridge';
type Params=Record<string,unknown>;
const runtime=globalThis as typeof globalThis&{sodeck:StateBridge&{binary:BinaryTransport};sodeckPlugin:unknown};
const bridge=networkBridge(workerClient(runtime.sodeck)),rules=new Rules(bridge),managers=new Map<string,Manager>();
configureObservation(workerClient(runtime.sodeck));
const editors=new Editors(rules);
let streams:NetworkStreams;
function manager(p:Params) {const value=managers.get(String(p.viewId));if(!value||value.closed)throw fault('VIEW_CLOSED','网络视图已关闭');return value;}
runtime.sodeckPlugin={
  async prepare(){await rules.load();streams=new NetworkStreams(runtime.sodeck.binary);},
  activate(){},
  async invoke(command:string,p:Params,invocation?:Invocation) {
    const diagnostic=observation?.withInvocation(invocation);
    if(command==='lifecycle.initialize') {
      if(typeof p.viewId!=='string'||typeof p.serverId!=='string'||typeof p.workspaceId!=='string'||typeof p.workspaceGeneration!=='string')throw fault('INVALID_PARAMS','缺少工作区身份');
      let m=managers.get(p.viewId);if(!m){m=new Manager(rules,{workspaceId:p.workspaceId,workspaceGeneration:p.workspaceGeneration},p.serverId,p.viewId,bridge,streams);managers.set(p.viewId,m);}return m.snapshot();
    }
    if(command==='lifecycle.prepareClose')return {allow:true};
    if(command==='lifecycle.presentation')return {accepted:true};
    if(command==='lifecycle.close'){editors.closeView(String(p.viewId));const m=managers.get(String(p.viewId));managers.delete(String(p.viewId));await m?.dispose();return {closed:true};}
    if(command==='lifecycle.cancel'){await Promise.all([...managers.values()].map(m=>m.cancel(String(p.operationId))));return {canceled:true};}
    if(command==='command.execute') {
      if(p.commandId==='network.open') {
        const ctx=p.context as Params;
        return bridge.call('views.open',{viewType:'network.manager',workspaceId:ctx.workspaceId,operationId:ctx.operationId,title:'网络隧道'});
      }
      const ctx=p.context as Params,args=p.args as Params,m=manager(ctx),id=String(args.id);
      if(p.commandId==='network.start')return m.start(id,String(ctx.operationId),args.restart===true,diagnostic);
      if(p.commandId==='network.reconnect')return m.reconnect(id,String(ctx.operationId),diagnostic);
      throw fault('UNSUPPORTED_API','未知网络命令');
    }
    const m=manager(p),id=String(p.id);
    switch(command) {
      case 'network.snapshot':return m.snapshot(Number(p.start)||0,typeof p.selected==='string'?p.selected:undefined,p.locate===true);
      case 'network.editor.open':return editors.open(m,p);
      case 'network.editor.get':return editors.get(m,p.editorToken);
      case 'network.editor.save':return editors.save(m,p,diagnostic);
      case 'network.editor.result':return editors.result(m,p.editorToken);
      case 'network.editor.close':return editors.close(m,p.editorToken);
      case 'network.save':return rules.save(m.serverId,p.rule,Number(p.expectedRevision),diagnostic);
      case 'network.stop':await m.stop(id,diagnostic);return {stopped:true};
      case 'network.delete':await m.stop(id,diagnostic);await rules.remove(m.serverId,id,Number(p.expectedRevision),diagnostic);releaseRun(m.runs.get(id)?.id??id);m.runs.delete(id);return {deleted:true};
      case 'network.diagnostics':{const all=m.runs.get(id)?.diagnostics??[],start=Math.max(0,Number(p.start)||0);return {items:all.slice(start,start+50),total:all.length};}
      case 'network.check':{
        const run=m.runs.get(id);if(!run)throw fault('INVALID_STATE','请先启动规则');
        const target={host:String(p.host??''),port:Number(p.port)};
        if(run.rule.type==='network-proxy'&&(!validHost(target.host)||!Number.isInteger(target.port)||target.port<1||target.port>65535))throw fault('INVALID_PARAMS','检查目标格式无效');
        return run.test(target,p.protocol==='socks'?'socks':'http',diagnostic);
      }
      default:throw fault('UNSUPPORTED_API','未知网络操作');
    }
  },
  async stop(){await Promise.all([...managers.values()].map(m=>m.dispose()));managers.clear();streams?.dispose();sampler?.stop();await observation?.stop();},
};
