import {diagnosticReference,diagnosticSummary,errorMessage as connectionErrorMessage,type DiagnosticRef,type DiagnosticSummary,type DiagnosticList} from '@sodeck/sdk';
import type {ExecBridge} from '@sodeck/sdk/bounded-exec';
import type {WorkspaceScope, Lease} from '@sodeck/sdk/remote-read';
import type {Clock} from '@sodeck/sdk/runtime';
import {connectionCommandTimeout,type RouteStatus} from '@sodeck/sdk';

export const errorCode = (e: unknown) => String((e as {code?:string})?.code ?? 'UNKNOWN');
export const errorMessage = connectionErrorMessage;
interface Resources {
  connections: {id:string;serverId:string;workspaceId:string;generation:number}[];
  leases: {id:string;connectionId:string;instanceId:string}[];
}
// A retired controller must reconcile an uncertain explicit acquire before a
// replacement controller in the same workspace can create another owned lease.
const explicitOwners=new Map<string,Connection>();
export class Connection {
  lease?: Lease;
  diagnosticRef?:DiagnosticRef;
  recentDiagnostic?:DiagnosticSummary;
  name = '';
  busy = false;
  cleanupPending = false;
  private epoch = 0;
  private routeRevision?:string;
  private operation?: string;
  private releasing = new Map<string, Lease>();
  private receipts = new Set<string>();
  private pending = new Map<string,Promise<unknown>>();
  private unknownAcquire?: Set<string>;
  constructor(private bridge: ExecBridge, readonly scope: WorkspaceScope, readonly serverId: string, private viewId: string, private clock: Clock, private instanceId='') {}
  private ownerKey(){return JSON.stringify([this.instanceId,this.scope.workspaceId,this.serverId]);}
  private unlock(){if(!this.busy&&!this.cleanupPending&&explicitOwners.get(this.ownerKey())===this)explicitOwners.delete(this.ownerKey());}
  private async call<T=unknown>(method:string,params:unknown,accepted?:(value:T)=>void,timeoutMs=6000):Promise<T> {
    const key=method+JSON.stringify(params);
    let actual=this.pending.get(key) as Promise<T>|undefined;
    if(!actual){
      actual=this.bridge.call<T>(method,params).then(value=>{accepted?.(value);return value;});
      this.pending.set(key,actual);
      void actual.finally(()=>{this.pending.delete(key);}).catch(()=>{});
    }
    let timer:ReturnType<typeof setTimeout>|undefined;
    try{return await Promise.race([actual,new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(Object.assign(Error('连接控制结果待核对'),{code:'CLEANUP_PENDING'})),timeoutMs);})]);}
    finally{clearTimeout(timer);}
  }
  async acquire(explicit: boolean, operationId: string) {
    if (this.busy || this.cleanupPending || this.lease) return;
    const owner=explicitOwners.get(this.ownerKey());
    if(owner&&owner!==this)throw Object.assign(Error('上一监控实例正在核对连接结果'),{code:'CLEANUP_PENDING'});
    if(explicit)explicitOwners.set(this.ownerKey(),this);
    if(explicit){this.diagnosticRef=undefined;this.recentDiagnostic=undefined;}
    this.busy = true; const epoch = this.epoch; this.operation = operationId;
    const valid = () => epoch === this.epoch;
    try {
      const server = await this.call<{revision:number; name?:string; displayName?:string}>('servers.get',{serverId:this.serverId});
      this.name = server.name ?? server.displayName ?? this.serverId;
      if (!valid()) return;
      let lease: Lease;
      const accepted=(value:Lease)=>{this.releasing.set(value.leaseId,value);};
      if (explicit) {
        if(!this.instanceId)throw Object.assign(Error('监控实例身份尚未初始化'),{code:'INSTANCE_EXPIRED'});
        const timeout=await connectionCommandTimeout({call:<T>(method:string,params?:unknown)=>this.call<T>(method,params)},this.serverId,this.scope.workspaceId);
        const before=new Set((await this.call<Resources>('resources.list',{})).leases.map(l=>l.id));
        try {lease = await this.call<Lease>('connections.acquire',{serverId:this.serverId, workspaceId:this.scope.workspaceId, operationId},accepted,timeout);}
        catch(error){if(errorCode(error)==='RESULT_UNKNOWN')this.unknownAcquire=before;throw error;}
      }
      else {
        const state = await this.call<{state:string}>('connections.observe',{scope:this.scope,serverId:this.serverId,configRevision:server.revision,waitMs:1000});
        if(!valid())return;
        if(state.state!=='active'){
          const history=await this.call<DiagnosticList>('connections.diagnostics.list',{workspaceId:this.scope.workspaceId,workspaceGeneration:this.scope.workspaceGeneration,serverId:this.serverId,purpose:'session'});
          if(valid()){this.recentDiagnostic=history.attempts.find(a=>['running','waiting','failed','canceled'].includes(a.state));this.diagnosticRef=this.recentDiagnostic?.reference;}
          return;
        }
        this.receipts.add(operationId);
        try {
          lease = await this.call<Lease>('connections.acquireExisting',{scope:this.scope,serverId:this.serverId,configRevision:server.revision,operationId},accepted);
        } catch (error) {
          if (errorCode(error) !== 'RESULT_UNKNOWN') {this.receipts.delete(operationId); throw error;}
          const deadline = this.clock.monotonicMs() + 6000;
          for (;;) {
            const receipt = await this.call<{state:string;result?:Lease;error?:unknown}>('remote.operations.get',{operationId});
            if (receipt.state === 'completed' && receipt.result) {lease = receipt.result; break;}
            if (receipt.state !== 'running') {await this.forget(operationId); throw receipt.error ?? Error('连接未能取得');}
            if (!valid() || this.clock.monotonicMs() >= deadline) throw Object.assign(Error('连接结果待核对'),{code:'CLEANUP_PENDING'});
            await new Promise(resolve => setTimeout(resolve,250));
          }
        }
      }
      if (!valid()) {this.releasing.set(lease.leaseId,lease); await this.releaseOwned();}
      else {
        this.releasing.delete(lease.leaseId);
        this.routeRevision=undefined;this.lease = lease;this.diagnosticRef=undefined;this.recentDiagnostic=undefined;
        await this.call('views.bindResources',{viewId:this.viewId,resources:[{kind:'lease',id:lease.leaseId}]});
        if (valid() && !await this.verify()) throw Object.assign(Error('连接代次已失效'),{code:'CONNECTION_NOT_READY'});
        if (!valid()) {this.releasing.set(lease.leaseId,lease); if (this.lease === lease) this.lease = undefined; await this.releaseOwned();}
      }
      if (!explicit) await this.forget(operationId);
    } catch (error) {
      this.diagnosticRef=diagnosticReference(error);this.recentDiagnostic=undefined;
      this.epoch++;
      if (this.lease) {this.releasing.set(this.lease.leaseId,this.lease); this.lease=undefined; await this.releaseOwned();}
      throw error;
    } finally {
      this.operation = undefined; this.busy = false;
      this.cleanupPending = !!this.unknownAcquire || this.releasing.size > 0 || this.receipts.size > 0 || this.pending.size > 0;
      this.unlock();
    }
  }
  private async forget(operationId: string) {
    await this.call('remote.operations.forget',{operationId}); this.receipts.delete(operationId);
  }
  async verify() {
    const lease = this.lease; if (!lease) return false;
    const route = await this.call<RouteStatus>('connections.route.get',{serverId:this.serverId,workspaceId:this.scope.workspaceId});
    const held=route.running.find(r=>r.connectionId===lease.connectionId&&r.generation===lease.generation&&r.state==='active');
    if(this.lease!==lease||!held||held.configurationChanged||held.route.routeRevision!==route.saved.routeRevision)return false;
    this.routeRevision??=held.route.routeRevision;return this.routeRevision===held.route.routeRevision;
  }
  private async releaseOwned() {
    for (const [id] of this.releasing) {
      try {await this.call('connections.release',{leaseId:id}); this.releasing.delete(id);}
      catch (error) {if (['STALE_HANDLE','STALE_WORKSPACE','CONNECTION_NOT_READY','NOT_FOUND'].includes(errorCode(error))) this.releasing.delete(id);}
    }
  }
  async release() {
    this.epoch++;
    if (this.lease) {this.releasing.set(this.lease.leaseId,this.lease); this.lease = undefined;}
    this.cleanupPending = this.busy || !!this.unknownAcquire || this.releasing.size > 0 || this.receipts.size > 0 || this.pending.size > 0;
    if (this.operation) await this.call('operations.cancel',{operationId:this.operation}).catch(() => {});
    await this.call('views.bindResources',{viewId:this.viewId,resources:[]}).catch(() => {});
    await this.releaseOwned(); await this.retry();
  }
  async retry() {
    if(!this.busy&&this.unknownAcquire){
      try {
        const resources=await this.call<Resources>('resources.list',{});
        for(const lease of resources.leases){
          const connection=resources.connections.find(c=>c.id===lease.connectionId&&c.serverId===this.serverId&&c.workspaceId===this.scope.workspaceId);
          if(connection&&lease.instanceId===this.instanceId&&!this.unknownAcquire.has(lease.id))this.releasing.set(lease.id,{leaseId:lease.id,connectionId:connection.id,generation:connection.generation});
        }
        this.unknownAcquire=undefined;
      }catch{/* Keep the uncertain acquisition blocked until its ownership can be checked. */}
    }
    if (!this.busy) for (const operationId of this.receipts) {
      try {
        await this.call('operations.cancel',{operationId});
        const result = await this.call<{state:string;result?:Lease}>('remote.operations.get',{operationId});
        if (result.state === 'running') continue;
        if (result.result?.leaseId && result.result.leaseId !== this.lease?.leaseId) this.releasing.set(result.result.leaseId,result.result);
        await this.releaseOwned(); if (this.releasing.size) continue;
        await this.forget(operationId);
      } catch (error) {if (['RESULT_EXPIRED','STALE_WORKSPACE'].includes(errorCode(error))) this.receipts.delete(operationId);}
    }
    await this.releaseOwned();
    this.cleanupPending = this.busy || !!this.unknownAcquire || this.releasing.size > 0 || this.receipts.size > 0 || this.pending.size > 0;
    this.unlock();
  }
}
