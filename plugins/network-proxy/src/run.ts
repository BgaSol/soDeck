import {NetworkClient,networkOperation,type NetworkOperation} from '@sodeck/sdk/network-client';
import type {Accepted,AcceptedStream,GroupSummary,Handle,ListenerOpened,RouteReady,Scope,StreamOpened} from '@sodeck/sdk/network';
import {validateNetwork} from '@sodeck/sdk/network';
import {observeState,type StateBridge,type StateSubscription} from '@sodeck/sdk/state';
import type {RouteMeta,RouteStatusV2} from '@sodeck/sdk';
import type {NetworkStreams} from '@sodeck/sdk/network-stream';
import {CoreChannel} from './channel';
import {proxy} from './protocol';
import {code} from './protocol/channel';
import {fault,formatTarget,type Target} from './protocol/address';
import {ascii,Reader} from './protocol/io';
import {headers} from './protocol/http-body';
import type {Rule} from './rules';
import {targetOperation} from './bridge';

export type Status='stopped'|'starting'|'ready'|'reconnecting'|'needs-attention'|'stopping'|'failed';
export interface Diagnostic {time:number;stage:string;code:string;target?:Target}
export interface CheckResult {runId:string;revision:number;time:number;target:Target;protocol:'http'|'socks'|'tcp';state:'checking'|'succeeded'|'failed';code?:string}
export const authentication=(error:unknown)=>['HOST_KEY_REQUIRED','HOST_KEY_CHANGED','AUTH_REQUIRED','AUTH_FAILED'].includes(code(error));
const sleep=(ms:number)=>new Promise<void>(r=>setTimeout(r,ms));
export class Run {
  readonly id=networkOperation().id;readonly started=Date.now();readonly net:NetworkClient;
  private currentStatus:Status='starting';
  get status():Status{return this.currentStatus;}
  set status(value:Status){if(value!==this.currentStatus){this.currentStatus=value;this.changed();}}
  group?:Handle;listener?:Handle;route?:Handle;path?:RouteMeta;address?:{host:string;port:number};
  summary?:GroupSummary;error?:{code:string;message:string};commandId?:string;check?:CheckResult;
  diagnostics:Diagnostic[]=[];private subscription?:StateSubscription<GroupSummary>;
  private closed=false;private accepting=false;private operations=new Set<NetworkOperation>();private channels=new Set<CoreChannel>();
  private probe?:{id?:string;done:boolean;error?:unknown};private retryTimer?:ReturnType<typeof setTimeout>;private stopPromise?:Promise<void>;private recoveryGeneration=0;private reconnectFlight?:Promise<void>;
  constructor(readonly rule:Rule,readonly scope:Scope,readonly viewId:string,readonly bridge:StateBridge,streams:NetworkStreams,private changed:()=>void=()=>{}) {this.net=new NetworkClient(bridge,scope,streams);}
  private current(){if(this.closed)throw fault('CANCELED','本次运行已结束');}
  async execute<T>(method:string,params:Record<string,unknown>,operation=networkOperation()):Promise<T> {
    this.current();this.operations.add(operation);
    try{const value=await this.net.execute<T>(method,params,operation);if(this.closed)await this.bridge.call('net.operations.cancel',{scope:this.scope,operationId:operation.id});this.current();return value;}finally{this.operations.delete(operation);}
  }
  log(stage:string,error:unknown,target?:Target) {this.diagnostics.push({time:Date.now(),stage,code:code(error),...(target?{target}: {})});if(this.diagnostics.length>200)this.diagnostics.shift();}
  private fail(error:unknown) {this.error={code:code(error),message:String((error as {message?:string})?.message??'网络操作失败').slice(0,512)};this.log('route',error);}
  async start(commandId:string) {
    this.commandId=commandId;
    try {
      this.group=(await this.execute<{group:Handle}>('net.groups.create',{viewId:this.viewId})).group;
      const binding=await this.execute<ListenerOpened>('net.listeners.open',{group:this.group,host:'127.0.0.1',port:this.rule.port});
      this.listener=binding.listener;this.address=binding.address;
      const saved=await this.bridge.call<RouteStatusV2>('connections.route.v2.get',{serverId:this.rule.serverId,workspaceId:this.scope.workspaceId});this.current();this.path=saved.saved;
      const result=await this.execute<RouteReady>('net.routes.acquire',{group:this.group,serverId:this.rule.serverId,expectedRouteRevision:this.path.routeRevision},{id:commandId,canceled:false});
      this.route=result.route;
      await this.execute('net.listeners.setAccepting',{group:this.group,listener:this.listener,enabled:true});this.status='ready';
      const subscription=await observeState(this.bridge,{contract:'net.group.v1',target:{scope:this.scope,group:this.group}},value=>{
        const bad=validateNetwork('x-events','net.group.v1',value);if(bad)throw fault('PROTOCOL_ERROR',bad);return value as GroupSummary;
      },summary=>this.observe(summary),{onError:error=>{if(!this.closed){this.fail(error);void this.stop().catch(()=>{});}}});
      if(this.closed)await subscription.dispose();else this.subscription=subscription;
    }catch(error) {
      this.fail(error);
      if(authentication(error)&&!this.closed) {
        this.status='needs-attention';
        // The core challenge remains bound to the group until trust/auth is
        // resolved or canceled. A failed start does not retain a listening port.
        if(this.listener)await this.net.execute('net.listeners.close',{group:this.group,listener:this.listener}).catch(()=>{});
        this.listener=undefined;this.address=undefined;
      }else {await this.stop();this.status=code(error)==='CANCELED'?'stopped':'failed';}
      throw error;
    }
  }
  private observe(summary:GroupSummary) {
    if(this.closed)return;this.summary=summary;
    if(summary.state!=='active'){this.error={code:'RESOURCE_STOPPED',message:'运行资源已由核心停止'};void this.stop().catch(()=>{});return;}
    if(this.status==='ready'&&summary.availableRoutes===0){
      this.status=this.rule.autoReconnect?'reconnecting':'needs-attention';this.error={code:'TRANSPORT_LOST',message:'SSH 路径已中断，本地端口仍保留'};this.log('route',this.error);
      if(this.rule.autoReconnect)this.scheduleReconnect(0);
    }
    if(summary.pendingAccept>0&&!this.accepting)void this.accept().catch(error=>this.log('accept',error));
  }
  private async accept() {
    if(this.closed||!this.group||!this.listener)return;this.accepting=true;
    try {
      const result=await this.execute<Accepted>('net.listeners.accept',{group:this.group,listener:this.listener,maxItems:16});
      for(const item of result.items)void this.serve(item).catch(error=>this.log('client',error));
    }finally{this.accepting=false;}
  }
  private async channel(stream:Handle,onRelease?:()=>void) {
    const bytes=await this.net.exchange(this.group!,stream);const channel=new CoreChannel(this.net,this.group!,bytes,()=>{this.channels.delete(channel);onRelease?.();});
    if(this.closed){await channel.close();throw fault('CANCELED','运行已结束');}
    this.channels.add(channel);return channel;
  }
  private async closeChannel(channel:CoreChannel) {this.channels.delete(channel);await channel.close().catch(error=>this.log('cleanup',error));}
  async target(target:Target) {
    if(this.status!=='ready'||!this.route)throw fault('TRANSPORT_LOST','SSH 路径未就绪');
    try {return await targetOperation(this.bridge,()=>{
      this.current();if(this.status!=='ready')throw fault('TRANSPORT_LOST','SSH 路径未就绪');
      return this.execute<StreamOpened>('net.tcp.open',{group:this.group,route:this.route,host:target.host,port:target.port});
    });}
    catch(error){this.log('target',error,target);throw error;}
  }
  private async serve(item:AcceptedStream) {
    if(!this.group)return;
    if(this.rule.type==='port-forward') {
      const probe=item.probeId?this.probe:undefined;
      try {
        const target=await this.target({host:this.rule.targetHost,port:this.rule.targetPort});
        if(probe&&this.probe===probe&&(!probe.id||probe.id===item.probeId)){probe.id=item.probeId;probe.done=true;}
        await this.net.splice(this.group,item.stream,target.stream);
      }catch(error){if(probe&&this.probe===probe&&(!probe.id||probe.id===item.probeId)){probe.id=item.probeId;probe.done=true;probe.error=error;}await this.net.execute('net.streams.close',{group:this.group,stream:item.stream}).catch(()=>{});throw error;}
      return;
    }
    let client:CoreChannel|undefined;const upstream=new Set<CoreChannel>();
    try {
      client=await this.channel(item.stream);
      try {await proxy(client,async target=>{const opened=await this.target(target),channel=await this.channel(opened.stream,()=>upstream.delete(channel));upstream.add(channel);return channel;});}
      finally {await client.finish();}
    }finally {
      if(client)await this.closeChannel(client);
      for(const channel of upstream)await this.closeChannel(channel);
    }
  }
  private scheduleReconnect(attempt:number) {
    if(this.closed)return;
    if(attempt>=3){this.status='needs-attention';return;}
    const generation=this.recoveryGeneration;
    this.retryTimer=setTimeout(()=>{this.retryTimer=undefined;void this.reconnect().catch(error=>{
      if(this.closed||generation!==this.recoveryGeneration)return;
      if(authentication(error)||['ROUTE_CHANGED','ROUTE_INVALID','NOT_FOUND'].includes(code(error))){this.status='needs-attention';return;}
      this.status='reconnecting';this.scheduleReconnect(attempt+1);
    });},[1000,3000,10000][attempt]);
  }
  async reconnect(commandId?:string) {
    this.current();if(!this.route)throw fault('INVALID_STATE','请重新启动规则');
    if(commandId){clearTimeout(this.retryTimer);this.recoveryGeneration++;this.commandId=commandId;}
    if(this.reconnectFlight)return this.reconnectFlight;
    this.status='reconnecting';this.check=undefined;
    const work=(async()=>{try {await this.execute('net.routes.refresh',{group:this.group,route:this.route},commandId?{id:commandId,canceled:false}:networkOperation());this.status='ready';this.error=undefined;}
    catch(error){if(!this.closed){this.fail(error);this.status='needs-attention';}throw error;}})();
    this.reconnectFlight=work;try{await work;}finally{if(this.reconnectFlight===work)this.reconnectFlight=undefined;}
  }
  async test(target:Target,protocol:'http'|'socks') {
    this.current();if(this.status!=='ready'||this.check?.state==='checking')throw fault('INVALID_STATE','规则尚未就绪或正在检查');
    if(this.rule.type==='port-forward')target={host:this.rule.targetHost,port:this.rule.targetPort};
    const result:CheckResult={runId:this.id,revision:this.rule.revision,time:Date.now(),target,protocol:this.rule.type==='port-forward'?'tcp':protocol,state:'checking'};this.check=result;
    let channel:CoreChannel|undefined;const probe={done:false} as NonNullable<Run['probe']>;this.probe=probe;
    try {
      const opened=await this.execute<StreamOpened>('net.listeners.connectSelf',{group:this.group,listener:this.listener});probe.id=opened.probeId;
      channel=await this.channel(opened.stream);
      const reader=new Reader(channel),deadline=Date.now()+30000;
      if(result.protocol==='tcp') {
        while(!probe.done){this.current();if(Date.now()>deadline)throw fault('TIMEOUT','固定目标连接检查超时');await sleep(20);}
        if(probe.error)throw probe.error;
      }else if(protocol==='http') {
        const name=formatTarget(target);await channel.write(ascii(`CONNECT ${name} HTTP/1.1\r\nHost: ${name}\r\n\r\n`));
        const status=await reader.line(8192,deadline);await headers(reader,deadline);
        if(status!=='HTTP/1.1 200 Connection Established')throw fault('CHECK_FAILED','代理未能建立目标 TCP 连接');
      }else {
        await channel.write(Uint8Array.of(5,1,0));const method=await reader.exact(2,deadline);if(method[0]!==5||method[1]!==0)throw fault('CHECK_FAILED','SOCKS5 协商失败');
        const host=ascii(target.host);await channel.write(Uint8Array.of(5,1,0,3,host.length,...host,target.port>>8,target.port&255));
        const response=await reader.exact(10,deadline);if(response[0]!==5||response[1]!==0||response[3]!==1)throw fault('CHECK_FAILED','SOCKS5 目标连接失败');
      }
      this.current();result.state='succeeded';
    }catch(error){result.state='failed';result.code=code(error);this.log('check',error,target);}
    finally {if(this.probe===probe)this.probe=undefined;if(channel)await this.closeChannel(channel);}
    return result;
  }
  stop():Promise<void> {
    if(this.stopPromise)return this.stopPromise;
    this.closed=true;this.status='stopping';this.check=undefined;this.recoveryGeneration++;clearTimeout(this.retryTimer);
    for(const operation of this.operations)operation.canceled=true;
    this.stopPromise=(async()=>{
      if(this.group)await this.net.execute('net.groups.close',{group:this.group}).catch(error=>{if(!['STALE_SCOPE','STALE_HANDLE'].includes(code(error)))throw error;});
      await this.subscription?.dispose().catch(()=>{});
      for(const channel of this.channels)channel.bytes.dispose();this.channels.clear();this.probe=undefined;this.status='stopped';
    })();return this.stopPromise;
  }
  snapshot() {return {id:this.id,rule:this.rule,status:this.status,started:this.started,address:this.address,path:this.path,error:this.error,commandId:this.commandId,connections:this.summary?.connections??0,bytesRead:this.summary?.bytesRead??0,bytesWritten:this.summary?.bytesWritten??0,cleanupPending:this.summary?.cleanupPending??0,check:this.check};}
}
