import {diagnosticSummary} from '@sodeck/sdk';
import {isConnectionAuthenticationError} from '@sodeck/sdk';
import {Cancellation, createBoundedExecClient, type ExecBridge} from '@sodeck/sdk/bounded-exec';
import {observeState, presentation, type StateBridge, type StateSubscription} from '@sodeck/sdk/state';
import type {Clock} from '@sodeck/sdk/runtime';
import type {WorkspaceScope} from '@sodeck/sdk/remote-read';
import {utf8} from '@sodeck/sdk/bytes';
import {Connection, errorCode, errorMessage} from './connection';
import {LinuxCollector} from './collectors/linux';
import {Model, backoff} from './model';
import {Scheduler, type Lane} from './scheduler';
import type {Options, Snapshot, SnapshotResult} from './protocol';

let sequence = 0;
export class Controller {
  readonly model = new Model();
  readonly connection: Connection;
  private exec;
  private collector;
  visible = false;
  disposed = false;
  private revision = 1n;
  private presentationRevision = -1;
  private epoch = 0;
  private admitted = false;
  private once = false;
  private stopping?: Promise<void>;
  private feed?: StateSubscription<ReturnType<typeof presentation>>;
  private feedTimer?: ReturnType<typeof setTimeout>;
  private attaching = false;
  private timer?: ReturnType<typeof setTimeout>;
  private token = new Cancellation();
  private jobs = new Set<Lane>();
  private nextFast = 0;
  private nextInventory = 0;
  private nextConnection = 0;
  private failures = {fast:0,slow:0};
  private unsupported = false;
  private error?: string;
  private actions = new Set<string>();
  lastVisible = 0;
  constructor(readonly bridge: StateBridge & ExecBridge, readonly clock: Clock, readonly scheduler: Scheduler,
    readonly scope: WorkspaceScope, readonly serverId: string, readonly viewId: string, private budget: () => boolean, private report?: (lane:Lane,durationMs:number,failed:boolean)=>void, instanceId='') {
    this.connection = new Connection(bridge,scope,serverId,viewId,clock,instanceId);
    this.exec = createBoundedExecClient(bridge,clock,{maximum:2}); this.collector = new LinuxCollector(this.exec);
  }
  private id() {return `monitor-${++sequence}-${Math.floor(this.clock.monotonicMs())}`;}
  changed() {this.revision++;}
  async attach() {
    if(this.disposed||this.attaching||this.feed)return;this.attaching=true;
    try {
    const feed = await observeState(this.bridge,{contract:'view.presentation.v1',target:{scope:this.scope,viewId:this.viewId}},presentation,value => {
      void this.presentation(value.visible,value.revision);
    },{onError:error => {this.error=errorMessage(error); this.changed(); void this.presentation(false,this.presentationRevision);}});
    if (this.disposed) await feed.dispose(); else this.feed=feed;
    } catch(error) {
      if(!this.disposed){this.error=errorMessage(error);this.changed();this.feedTimer=setTimeout(()=>void this.attach(),5000);}
    } finally {this.attaching=false;}
  }
  presentation(visible: boolean, revision: number) {
    if (this.disposed || revision < this.presentationRevision) return;
    this.presentationRevision=revision; if (this.visible===visible) return;
    this.visible=visible; this.changed();
    if (!visible) {this.once=false; void this.stopSampling();}
    else {this.lastVisible=this.clock.monotonicMs(); this.wake();}
  }
  private wanted() {return !this.disposed && this.visible && (!this.model.options.paused || this.once) && !this.unsupported;}
  private wake(delay=0) {if(this.disposed)return;clearTimeout(this.timer);this.timer=setTimeout(()=>{void this.tick().catch(error=>{this.error=errorMessage(error);this.changed();this.wake(5000);});},delay);}
  private async tick() {
    if(this.disposed)return;
    if(this.stopping){this.wake(500);return;}
    if(this.exec.counts().cleanupPending || this.connection.cleanupPending){await this.exec.retryCleanup();await this.connection.retry();if(!this.exec.counts().cleanupPending)this.scheduler.releaseHeld(this.viewId);this.changed();this.wake(5000);return;}
    this.scheduler.releaseHeld(this.viewId);
    if(!this.wanted()){this.scheduler.drop(this.viewId);this.admitted=false;return;}
    this.admitted=this.scheduler.want(this.viewId,()=>{this.admitted=true;this.changed();this.wake();});
    if(!this.admitted){this.wake(1000);return;}
    const now=this.clock.monotonicMs();
    if(!this.connection.lease){
      if(!this.connection.busy && now>=this.nextConnection){
        this.nextConnection=now+5000;const epoch=this.epoch;
        try{await this.connection.acquire(false,this.id());if(epoch===this.epoch&&this.connection.lease){this.collector.reset();this.model.reset(now);this.nextFast=0;this.nextInventory=0;this.error=undefined;}}
        catch(error){if(epoch===this.epoch)this.error=errorMessage(error);}
        this.changed();
      }
      this.wake(1000);return;
    }
    if(now>=this.nextFast)this.queue('fast');
    if(!this.once && (now>=this.nextInventory || this.model.capacityTargets(now).length))this.queue('slow');
    this.wake(250);
  }
  private queue(lane: Lane) {
    if(this.jobs.has(lane))return;
    const epoch=this.epoch;
    const accepted=this.scheduler.enqueue(this.viewId,lane,async()=>{
      try{if(epoch===this.epoch&&this.wanted())await this.sample(lane,epoch);}
      finally{this.jobs.delete(lane);this.changed();this.wake();}
    });
    if(accepted)this.jobs.add(lane);
  }
  private async sample(lane: Lane, epoch: number) {
    const lease=this.connection.lease;if(!lease)return;
    const target={scope:this.scope,leaseId:lease.leaseId,connectionGeneration:lease.generation,cancellation:this.token};
    const valid=()=>epoch===this.epoch&&this.connection.lease===lease&&this.wanted();
    const start=this.clock.monotonicMs();
    let failed=false;
    try {
      if(lane==='fast'){
        const data=await this.collector.fast(target,this.id(),this.model.inventory,start,this.model.selection);
        if(!valid())return;if(!await this.connection.verify())throw Object.assign(Error('连接路径已变化'),{code:'CONNECTION_NOT_READY'});if(!valid())return;
        const end=this.clock.monotonicMs();this.model.installFast(data,start,end,Date.now());this.nextFast=end+this.model.options.refreshSeconds*1000;
      }else if(start>=this.nextInventory){
        const inventory=await this.collector.inventory(target,this.id(),this.model.selection.mount);if(!valid())return;
        if(!await this.connection.verify())throw Object.assign(Error('连接路径已变化'),{code:'CONNECTION_NOT_READY'});if(!valid())return;
        this.model.installInventory(inventory,this.clock.monotonicMs());this.nextInventory=this.clock.monotonicMs()+60000;
      }else{
        const mount=this.model.capacityTargets(start)[0];if(!mount)return;
        this.model.capacityAttempt(mount,start);
        try{const value=await this.collector.capacity(target,this.id(),mount);if(valid())this.model.capacityResult(mount,this.clock.monotonicMs(),Date.now(),value);}
        catch(error){if(valid())this.model.capacityResult(mount,this.clock.monotonicMs(),Date.now(),undefined,errorMessage(error));throw error;}
      }
      if(!valid())return;
      this.failures[lane]=0;this.error=undefined;
      if(this.model.bytes()>4194304||!this.budget())throw Object.assign(Error('监控缓存达到上限，已暂停采样'),{code:'CACHE_CAPACITY'});
    }catch(error){
      failed=true;
      if(!valid())return;
      this.error=errorMessage(error);const code=errorCode(error),now=this.clock.monotonicMs();
      if(lane==='fast'){this.model.fail(this.error,now);this.nextFast=now+backoff(++this.failures.fast);}
      else this.nextInventory=Math.max(this.nextInventory,now+backoff(++this.failures.slow));
      if(code==='UNSUPPORTED_OS'){this.unsupported=true;await this.stopSampling();}
      else if(code==='CACHE_CAPACITY'){this.model.options.paused=true;await this.stopSampling();}
      // An uncertain exec close does not invalidate the SSH lease. Keep it while
      // tick reconciles the retained job; dropping the only lease would prevent
      // an otherwise healthy standalone monitor from resuming without a dial.
      else if(['CONNECTION_NOT_READY','STALE_HANDLE','STALE_WORKSPACE'].includes(code))await this.stopSampling();
    }finally{
      this.report?.(lane,this.clock.monotonicMs()-start,failed);
      if(this.exec.counts().cleanupPending)this.scheduler.hold(this.viewId,lane);
      if(lane==='fast'&&this.once){this.once=false;await this.stopSampling();}
      this.changed();
    }
  }
  private stopSampling() {
    if(this.stopping)return this.stopping;
    this.epoch++;this.token.cancel();clearTimeout(this.timer);this.scheduler.drop(this.viewId);this.admitted=false;
    // Pending intentions were removed by the scheduler; running closures still
    // own their lane until finally, but an invalid epoch cannot publish data.
    this.model.reset(this.clock.monotonicMs());this.changed();
    this.stopping=(async()=>{await this.exec.cancelAll();await this.connection.release();})().finally(()=>{
      this.stopping=undefined;this.token=new Cancellation();this.jobs.clear();this.changed();this.wake(250);
    });
    return this.stopping;
  }
  options(options: Partial<Options>) {
    const old=this.model.options;this.model.options={...old,...options};this.changed();
    if(old.refreshSeconds!==this.model.options.refreshSeconds){this.model.reset(this.clock.monotonicMs());this.nextFast=0;}
    if(this.model.options.paused){this.once=false;void this.stopSampling();}else this.wake();
  }
  action(id: string) {if(this.actions.has(id))return false;this.actions.add(id);if(this.actions.size>64)this.actions.delete(this.actions.values().next().value!);return true;}
  refresh(id: string) {
    if(!this.action(id))return;
    this.collector.reset();
    this.unsupported=false;this.once=this.model.options.paused;if(!this.failures.fast)this.nextFast=Math.min(this.nextFast,this.clock.monotonicMs());if(!this.failures.slow)this.nextInventory=Math.min(this.nextInventory,this.clock.monotonicMs());this.wake();this.changed();
  }
  async connect(id: string) {
    if(!this.action(id)||this.connection.busy||this.disposed)return;
    this.unsupported=false;
      if(this.stopping)await this.stopping;
      if(this.disposed)return;
      try{await this.connection.acquire(true,id);this.collector.reset();this.error=undefined;this.model.reset(this.clock.monotonicMs());}
      catch(error){this.error=isConnectionAuthenticationError(error)?'等待完成连接路径认证':errorMessage(error);this.actions.delete(id);throw error;}
      finally{if(!this.wanted())await this.stopSampling();this.changed();this.wake();}
  }
  select(kind: 'network'|'disk'|'mount', id: string|null, page?:number, allowCapacity=false, expanded?:boolean) {
    if(kind==='mount'&&expanded!==undefined)this.model.selection.mountExpanded=expanded;
    if(page!==undefined){const key=kind==='mount'?'mountPage':kind==='network'?'networkPage':'diskPage',count=kind==='mount'?this.model.inventory?.mounts.length??0:kind==='network'?this.model.networks().length:this.model.disk.value?.length??0;this.model.selection[key]=Math.min(page,Math.max(0,Math.ceil(count/(kind==='mount'?8:31))-1));}
    if(id!==null){const rows=kind==='mount'?this.model.inventory?.mounts:this.model[kind].value;if(!rows?.some(row=>row.id===id))throw Object.assign(Error('该对象已经变化，请刷新后重试'),{code:'STALE_OBJECT'});this.model.selection[kind]=id;if(kind==='mount'&&allowCapacity)this.model.allowCapacity(id);}
    this.changed();this.wake();
  }
  snapshot(known?:string): SnapshotResult {
    const revision=String(this.revision);if(known===revision)return {unchanged:true,revision};
    const now=this.clock.monotonicMs(),cleanupPending=!!this.stopping||this.exec.counts().cleanupPending>0||this.connection.cleanupPending;
    const snapshot:Snapshot={protocolVersion:1,revision,viewId:this.viewId,serverId:this.serverId,serverName:this.connection.name||this.serverId,workspaceGeneration:this.scope.workspaceGeneration,
      connection:this.connection.lease?'connected':this.connection.busy?'connecting':this.error?'disconnected':'waiting',
      status:cleanupPending?'正在回收采集资源':!this.visible?'已隐藏，采集停止':this.model.options.paused&&!this.once?'已暂停':this.unsupported?'当前远端不支持采集':!this.admitted?'等待采样名额':this.connection.lease?'当前 SSH 可见环境':this.connection.recentDiagnostic?diagnosticSummary(this.connection.recentDiagnostic):'等待已有连接',
      ...(this.error?{error:this.error}:{}),...(!this.connection.lease&&this.connection.diagnosticRef?{diagnosticRef:this.connection.diagnosticRef}:{}),visible:this.visible,cleanupPending,limited:this.wanted()&&!this.admitted,...this.model.fields(now),capturedAt:Date.now(),timelineMs:now,diagnostics:{activeJobs:this.exec.counts().active,cacheBytes:this.model.bytes()}};
    if(utf8(JSON.stringify(snapshot)).length>262144)throw Object.assign(Error('监控快照超过上限'),{code:'CAPACITY'});
    return snapshot;
  }
  releaseHistory() {this.model.releaseHistory();this.changed();}
  health(){return {state:this.exec.counts().cleanupPending||this.connection.cleanupPending?'cleanupPending':!this.visible?'hidden':this.model.options.paused?'paused':this.connection.lease?(this.model.partial?'partial':'collecting'):this.error?'disconnected':'waiting',activeJobs:this.exec.counts().active,cacheBytes:this.model.bytes(),cleanupPending:this.exec.counts().cleanupPending>0||this.connection.cleanupPending};}
  async cleanup(){await this.exec.retryCleanup();await this.connection.retry();if(!this.exec.counts().active&&!this.connection.cleanupPending){this.scheduler.releaseHeld(this.viewId);return true;}return false;}
  async dispose() {if(this.disposed)return;this.disposed=true;clearTimeout(this.feedTimer);await Promise.allSettled([this.feed?.dispose(),this.stopSampling()]);await this.exec.dispose();this.model.releaseHistory();if(!this.exec.counts().active)this.scheduler.releaseHeld(this.viewId);}
}
