import type {ObservationClient} from '@sodeck/sdk/observation';
import {observation,errorCode} from './observation';
import {workerClient} from '@sodeck/sdk/worker';
import {Remote,type Bridge,type Lease,type WorkspaceScope} from '@sodeck/sdk/remote-client';
import {decode} from '@sodeck/sdk/bytes';
import {verifyPathView} from '@sodeck/sdk/linux-path-view';
import type {Notification} from '@sodeck/sdk/state';
import type {Registered} from '@sodeck/sdk/directory';
import {DirectoryProtocol,type Report} from './directory-protocol';
export interface ObservedPty {ptyId:string;generation:number;observer?:{observerId:string;cursor:string}}
export interface ReporterBridge extends Bridge {onNotification(listener:(event:Notification)=>void):()=>void}
export class DirectoryReporter {
  private remote:Remote;
  private source?:Registered;
  private disposed=false;
  private drainRunning=false;
  private dirty=false;
  private publishing=false;
  private latest?:Report;
  private identity?:string;
  private cursor='0';
  private unsubscribe?:()=>void;
  private timer?:ReturnType<typeof setTimeout>;
  private timeout?:ReturnType<typeof setTimeout>;
  private publishSequence=0;
  private lastPublished=0;
  private reports:Promise<unknown>=Promise.resolve();
  private protocol:DirectoryProtocol;
  private received=0n;
  private observationState="starting";private directoryRevision="0";
  constructor(private bridge:ReporterBridge,private scope:WorkspaceScope,private serverId:string,private viewId:string,private pty:ObservedPty,tag:string,private diagnostic:ObservationClient=observation,private sessionRef?:string,private observed?:(state:string,revision:string,error?:string)=>void){
    this.bridge=workerClient(bridge);
    this.remote=new Remote(this.bridge,scope);
    this.protocol=new DirectoryProtocol(tag,r=>{
      if(BigInt(r.sequence)<=this.received)return;
      this.received=BigInt(r.sequence);clearTimeout(this.timeout);
      if(r.status==='ready'){
        const identity=r.evidence.pid+':'+r.evidence.start;
        if(this.identity&&identity!==this.identity){this.latest=undefined;void this.report('unmappable');return;}
        this.identity=identity;
      }
      this.latest=r;void this.publish();
    },()=>{this.latest=undefined;void this.report('unknown');});
  }
  async start(){
    this.source=await this.bridge.call<Registered>('directories.register',{scope:this.scope,viewId:this.viewId,pty:{id:this.pty.ptyId,generation:this.pty.generation}});
    if(this.disposed){await this.disposeSource();return;}
    if(!this.pty.observer){await this.report('unsupported');return;}
    this.cursor=this.pty.observer.cursor;
    this.unsubscribe=this.bridge.onNotification(event=>{
      const p=event.payload as {observerId?:string}|undefined;
      if(event.type==='stream.readable'&&p?.observerId===this.pty.observer?.observerId){this.dirty=true;void this.drain();}
    });
    this.timeout=setTimeout(()=>{void this.report('unsupported');},5000);
    this.dirty=true;void this.drain();
  }
  private async report(status:string,report?:Extract<Report,{status:'ready'}>){
    // All publications share one ordering lane, including timeout, gap and EOF.
    const source=this.source;
    this.reports=this.reports.catch(()=>{}).then(async()=>{
      if(this.disposed||!source||this.source!==source)return;
      const payload={...source,reportSequence:String(++this.publishSequence),status,...(report?{logicalPath:report.logical,physicalPath:report.physical,evidence:report.evidence}:{})};
      while(!this.disposed&&this.source===source){
        try{const receipt=await this.bridge.call<{sourceRevision:string}>('directories.report',payload);this.directoryRevision=receipt.sourceRevision;this.observed?.(this.observationState,this.directoryRevision);const next=['ready','lastConfirmed'].includes(status)?'ready':status==='unsupported'?'unsupported':status==='disconnected'?'stopped':'degraded';if(next!==this.observationState){this.observationState=next;this.observed?.(next,this.directoryRevision);void this.diagnostic.emit('plugin.sodeck.terminal.integration.state.changed',{state:next,directoryRevision:this.directoryRevision,...(this.sessionRef?{sessionRef:this.sessionRef}:{})});}break;}catch(e){
          if((e as {code?:string}).code!=='RATE_LIMITED'){if(this.observationState!=='degraded'){this.observationState='degraded';this.observed?.('degraded',this.directoryRevision,errorCode(e));void this.diagnostic.emit('plugin.sodeck.terminal.integration.report.failed',{reasonCode:errorCode(e),...(this.sessionRef?{sessionRef:this.sessionRef}:{})},{outcome:'failed'});}break;}
          await new Promise<void>(r=>setTimeout(r,150));
        }
      }
    });
    await this.reports;
  }

  private async drain(){
    if(this.drainRunning||this.disposed||!this.pty.observer)return;this.drainRunning=true;
    try{
      do{
        this.dirty=false;
        const r=await this.bridge.call<{bytes:string;nextCursor:string;gap:boolean;closed:boolean}>('pty.observe.read',{ptyId:this.pty.ptyId,generation:this.pty.generation,observerId:this.pty.observer.observerId,after:this.cursor});
        if(this.disposed)return;this.cursor=r.nextCursor;
        if(r.gap){this.protocol.reset();this.latest=undefined;await this.report('unknown');}
        const bytes=decode(r.bytes);this.protocol.push(bytes);
        if(r.closed){this.latest=undefined;await this.report('disconnected');return;}
        if(bytes.length>0)this.dirty=true;
      }while(this.dirty&&!this.disposed);
    }catch{if(!this.disposed)await this.report('disconnected');}
    finally{this.drainRunning=false;}
  }
  private async publish(){
    if(this.publishing||this.disposed)return;this.publishing=true;
    try{
      while(this.latest&&!this.disposed){
        const report=this.latest;
        const wait=120-(Date.now()-this.lastPublished);
        if(wait>0)await new Promise<void>(r=>setTimeout(r,wait));
        if(report!==this.latest)continue;
        if(report.status==='busy'){await this.report('lastConfirmed');}
        else{
          try{
            if(!this.remote.lease){
              const server=await this.bridge.call<{revision:number}>('servers.get',{serverId:this.serverId});
              this.remote.lease=await this.bridge.call<Lease>('connections.acquireExisting',{scope:this.scope,serverId:this.serverId,configRevision:server.revision,operationId:`directory-${this.viewId}-${Date.now()}`});
            }
            await verifyPathView(this.remote,report.evidence,report.physical);
            if(report!==this.latest||this.disposed)continue;
            await this.report('ready',report);
          }catch{if(report===this.latest&&!this.disposed)await this.report('unmappable');}
        }
        this.lastPublished=Date.now();
        if(this.latest===report)this.latest=undefined;
      }
    }finally{this.publishing=false;}
  }
  private async disposeSource(){if(this.source)await this.bridge.call('directories.unregister',this.source).catch(()=>{});this.source=undefined;}
  async dispose(){
    if(this.disposed)return;this.disposed=true;clearTimeout(this.timeout);clearTimeout(this.timer);this.unsubscribe?.();
    this.protocol.reset();this.latest=undefined;
    await this.disposeSource();
    if(this.pty.observer)await this.bridge.call('pty.observe.close',{ptyId:this.pty.ptyId,generation:this.pty.generation,observerId:this.pty.observer.observerId}).catch(()=>{});
    await this.remote.dispose();
    if(this.observationState!=="stopped"){this.observationState="stopped";this.observed?.("stopped",this.directoryRevision);void this.diagnostic.emit("plugin.sodeck.terminal.integration.state.changed",{state:"stopped",directoryRevision:this.publishSequence});}
  }
}
