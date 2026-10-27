import {observeState,directory,presentation,type Directory} from './follow-contract';
import type {StateBridge,StateSubscription} from '@sodeck/sdk/state';
import type {Remote} from '@sodeck/sdk/remote-client';
import {verifyPathView} from '@sodeck/sdk/linux-path-view';
import {message} from './remote';
import type {Explorer} from './state';
export interface FollowSnapshot {enabled:boolean;paused:boolean;source:string;status:string;message:string;loading:boolean;navigation:number}
export class FollowController {
  enabled=true;
  paused=false;
  private state?:Explorer;
  private value?:Directory;
  private observed='';
  private generation=0;
  private disposed=false;
  private visible=false;
  private interaction=false;
  private pending=false;
  private once=false;
  private busy=false;
  private remote?:Remote;
  private directoryFeed?:StateSubscription<Directory>;
  private presentationFeed?:StateSubscription<ReturnType<typeof presentation>>;
  private subscriptionGeneration=0;
  private visibilityGeneration=0;
  private error='';
  private navigation=0;
  private timings:{mappingMs:number;locateMs:number}[]=[];
  private connectionQuery?:Promise<boolean>;
  private observedLease='';
  diagnostics(){return {timings:this.timings,attached:!!this.state,visible:this.visible};}
  constructor(private bridge:StateBridge){}
  snapshot():FollowSnapshot{return {enabled:this.enabled,paused:this.paused,source:this.value?.source?.title??'',status:this.value?.status??'unknown',message:this.error||(!this.value?.source?'等待终端目录':this.value.status==='ready'?'':this.value.status==='lastConfirmed'?'上次确认目录':({unmappable:'终端目录无法映射到文件浏览',unsupported:'此终端未提供目录集成',disconnected:'终端连接已断开',unknown:'尚未获取终端目录'}[this.value.status]??'')),loading:this.busy,navigation:this.navigation};}
  async attach(state:Explorer){
    await this.detach();this.disposed=false;this.state=state;
    const generation=++this.subscriptionGeneration;
    const feed=await observeState(this.bridge,{contract:'view.presentation.v1',target:{scope:state.scope,viewId:state.viewId}},presentation,value=>{
      if(generation!==this.subscriptionGeneration)return;
      void this.setVisible(value.visible,value.revision).catch(e=>{if(this.state!==state||generation!==this.subscriptionGeneration)return;this.error=message(e);this.value=undefined;this.pending=false;state.changed();});
    },{onError:e=>{this.error=message(e);state.changed();}});
    if(generation!==this.subscriptionGeneration){await feed.dispose();return;}this.presentationFeed=feed;
  }
  async setVisible(visible:boolean,revision:number){
    const state=this.state;if(!state||this.disposed)return;
    await state.presentation(visible,revision);
    if(state.visible!==visible)return;
    if(visible===this.visible)return;this.visible=visible;
    this.generation++;const generation=++this.visibilityGeneration;
    if(!visible){this.pending=false;await this.remote?.cancel();await this.directoryFeed?.dispose();this.directoryFeed=undefined;return;}
    this.observed='';
    const feed=await observeState(this.bridge,{contract:'workspace.directory.v1',target:state.scope},directory,value=>{
      if(!this.visible||this.state!==state||this.disposed)return;
      this.receive(value);
    },{onError:e=>{this.error=message(e);this.value=undefined;this.pending=false;state.changed();}});
    if(generation!==this.visibilityGeneration||!this.visible){await feed.dispose();return;}this.directoryFeed=feed;

  }
  private receive(value:Directory){
    this.value=value;
    const key=[value.scope.workspaceGeneration,value.source?.id,value.source?.generation,value.selectionRevision,value.pathRevision].join(':');
    const changed=key!==this.observed;this.observed=key;
    if(changed){this.generation++;this.error='';if(this.enabled&&!this.paused)this.pending=true;void this.remote?.cancel();}
    this.state?.changed();void this.tick();
  }
  manual(){this.generation++;this.pending=false;this.once=false;this.error='';void this.remote?.cancel();this.state?.changed();}
  interactionState(active:boolean,submitted=false){this.interaction=active;if(submitted)this.manual();else if(!active)void this.tick();}
  async setEnabled(value:boolean){this.enabled=value;this.paused=false;this.generation++;this.once=false;this.pending=value;this.error='';await this.remote?.cancel();this.state?.changed();void this.tick();}
  locateOnce(){this.generation++;this.once=true;this.pending=true;this.error='';void this.tick();}
  async cancel(){this.paused=true;this.pending=false;this.once=false;this.generation++;await this.remote?.cancel();this.state?.changed();}
  async tick(){
    const state=this.state,value=this.value;
    const lease=state?.remote.lease;
    const leaseKey=lease?`${lease.connectionId}:${lease.generation}`:'';
    if(leaseKey!==this.observedLease){this.observedLease=leaseKey;if(lease&&this.enabled&&!this.paused)this.pending=true;}
    if(this.busy||this.disposed||!state||!this.visible||this.interaction||!this.pending||(!this.once&&(!this.enabled||this.paused))||!state.remote.lease)return;
    if(!value?.physicalPath||!value.evidence||!['ready','lastConfirmed'].includes(value.status)){return;}
    const generation=this.generation;const valid=()=>!this.disposed&&this.visible&&this.state===state&&this.generation===generation&&!this.interaction;
    this.pending=false;this.busy=true;state.changed();const remote=state.remote.fork();this.remote=remote;
    try{
      const started=Date.now();
      this.connectionQuery??=this.bridge.call<{capabilities:string[]}>('capabilities.list').then(r=>r?.capabilities?.includes('workspace.directory.connection.v1')??false);
      if(await this.connectionQuery){
        if(!value.source)throw new Error('等待终端目录');
        const connection=await this.bridge.call<{connectionId:string;generation:number}>('directories.connection',{scope:state.scope,sourceId:value.source.id,sourceGeneration:value.source.generation});
        if(!valid())return;
        if(connection.connectionId!==remote.lease?.connectionId||connection.generation!==remote.lease?.generation){
          this.error='连接不同，暂不跟随';return;
        }
      }
      await verifyPathView(remote,value.evidence,value.physicalPath);
      if(!valid())return;
      const mapped=Date.now();
      await state.locateFollow(value.physicalPath,remote,valid);
      if(valid()){this.navigation++;this.error='';this.timings.push({mappingMs:mapped-started,locateMs:Date.now()-mapped});if(this.timings.length>64)this.timings.shift();}
    }catch(e){if(valid())this.error=message(e);}
    finally{await remote.cancel();if(this.remote===remote)this.remote=undefined;this.busy=false;if(generation===this.generation)this.once=false;state.changed();if(this.pending)void this.tick();}
  }
  async detach(){
    this.subscriptionGeneration++;this.visibilityGeneration++;this.generation++;this.visible=false;this.pending=false;this.once=false;this.interaction=false;
    await Promise.all([this.presentationFeed?.dispose(),this.directoryFeed?.dispose(),this.remote?.cancel()]);
    this.presentationFeed=undefined;this.directoryFeed=undefined;this.state=undefined;
  }
  async dispose(){this.disposed=true;await this.detach();}
}
