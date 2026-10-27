import type {Scope} from '@sodeck/sdk/network';
import type {NetworkStreams} from '@sodeck/sdk/network-stream';
import type {StateBridge} from '@sodeck/sdk/state';
import {utf8} from '@sodeck/sdk/bytes';
import {Rules} from './rules';
import {Run} from './run';
import {fault} from './protocol/address';

export class Manager {
  private badgeCount=-1;
  private publishBadge(){
    const count=[...this.runs.values()].filter(r=>['ready','starting','reconnecting'].includes(r.status)).length;
    if(count===this.badgeCount)return;this.badgeCount=count;
    void this.bridge.call('views.badge.set',{viewId:this.viewId,count}).catch(()=>{if(this.badgeCount===count)this.badgeCount=-1;});
  }
  readonly runs=new Map<string,Run>();private flights=new Map<string,Promise<unknown>>();private generations=new Map<string,number>();closed=false;
  constructor(readonly rules:Rules,readonly scope:Scope,readonly serverId:string,readonly viewId:string,readonly bridge:StateBridge,readonly streams:NetworkStreams){}
  snapshot(start=0,selected?:string,locate=false) {
    this.publishBadge();
    const all=this.rules.all(this.serverId);if(locate&&selected){const index=all.findIndex(r=>r.id===selected);if(index>=0)start=Math.floor(index/20)*20;}
    const offset=Math.min(Math.max(0,all.length-1),Math.max(0,Math.floor(start)||0)),items=all.slice(offset,offset+20).map(rule=>({rule,run:this.runs.get(rule.id)?.snapshot()}));
    const selectedRule=all.find(r=>r.id===selected);
    const result={items,runningCount:[...this.runs.values()].filter(r=>["ready","starting","reconnecting"].includes(r.status)).length,total:all.length,start:offset,nextStart:offset+items.length,selected:selectedRule?{rule:selectedRule,run:this.runs.get(selectedRule.id)?.snapshot()}:undefined};
    while(utf8(JSON.stringify(result)).length>32768&&items.length>1){items.pop();result.nextStart=offset+items.length;}
    if(utf8(JSON.stringify(result)).length>32768)throw fault('CAPACITY','单条规则快照超过 32 KiB');return result;
  }
  async start(id:string,commandId:string,restart=false):Promise<unknown> {
    if(this.closed)throw fault('VIEW_CLOSED','视图已关闭');
    const current=this.flights.get(id);if(current){if(!restart)return current;await this.stop(id);return this.start(id,commandId);}
    const old=this.runs.get(id);
    if(old&&['ready','reconnecting'].includes(old.status)&&!restart)return old.snapshot();
    const generation=(this.generations.get(id)??0)+1;this.generations.set(id,generation);
    const work=(async()=>{
      if(old)await old.stop();
      if(this.closed)throw fault('VIEW_CLOSED','视图已关闭');
      if(this.generations.get(id)!==generation)throw fault('CANCELED','启动已取消');
      const run=new Run(this.rules.get(this.serverId,id),this.scope,this.viewId,this.bridge,this.streams,()=>this.publishBadge());this.runs.set(id,run);this.publishBadge();
      await run.start(commandId);return run.snapshot();
    })();this.flights.set(id,work);
    try{return await work;}finally{if(this.flights.get(id)===work)this.flights.delete(id);}
  }
  async reconnect(id:string,commandId:string) {
    const run=this.runs.get(id);if(!run)throw fault('INVALID_STATE','规则尚未启动');
    const current=this.flights.get(id);if(current)return current;
    if(!run.route)return this.start(id,commandId);
    const work=run.reconnect(commandId);this.flights.set(id,work);try{await work;return run.snapshot();}finally{if(this.flights.get(id)===work)this.flights.delete(id);}
  }
  async stop(id:string){this.generations.set(id,(this.generations.get(id)??0)+1);this.flights.delete(id);await this.runs.get(id)?.stop();}
  async cancel(operation:string){await Promise.all([...this.runs.entries()].filter(([,r])=>r.commandId===operation).map(([id])=>this.stop(id)));}
  async dispose(){this.closed=true;await Promise.all([...this.runs.values()].map(r=>r.stop()));this.runs.clear();}
}
export type Snapshot=ReturnType<Manager['snapshot']>;
