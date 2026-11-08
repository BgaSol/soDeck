let surfaceReady=false;
addEventListener('sodeck:mount',()=>{surfaceReady=false;});
addEventListener('sodeck:surfaceReady',()=>{surfaceReady=true;});
const bytes=value=>new TextEncoder().encode(JSON.stringify(value)).length;
const revision=value=>typeof value==='string'&&/^(0|[1-9]\d{0,19})$/.test(value)&&BigInt(value)<=18446744073709551615n;
const identity=record=>JSON.stringify([record.topic,record.producer,record.key]);
function validRecord(r){
  if(!r||typeof r.topic!=='string'||typeof r.key!=='string'||!revision(r.revision)||typeof r.present!=='boolean'||!['core','plugin'].includes(r.producer?.kind)||r.present!==Object.hasOwn(r,'value'))throw new Error('共享记录格式无效');
}
export class SharedState {
  constructor(mount){
    this.context=mount.context;this.available=mount.capabilities?.sharedState===1;this.watches=new Map();this.owned=new Set();this.waiters=[];this.disposed=false;
    this.ready=()=>{for(const resolve of this.waiters.splice(0))resolve();};
    this.changed=event=>{const frame=event.detail.data;this.watches.get(frame?.clientSubscriptionId)?.receive(frame);};
    addEventListener('sodeck:surfaceReady',this.ready);addEventListener('sodeck:stateChanged',this.changed);
  }
  async request(op,input={}){
    if(this.disposed||!this.available)throw Object.assign(new Error('共享状态不可用'),{code:'unsupported'});
    if(!surfaceReady)await new Promise(resolve=>this.waiters.push(resolve));
    if(this.disposed)throw new Error('共享状态挂载已结束');
    const envelope=await sodeck.request('state',{op,...input,expectedContext:this.context});
    if(envelope.error)throw Object.assign(new Error(envelope.error.message),envelope.error);
    return envelope.result;
  }
  watch(filter,change){const watch=new Watch(this,filter,change);this.owned.add(watch);watch.start();return watch;}
  dispose(){for(const watch of [...this.owned])watch.dispose();this.disposed=true;this.ready();removeEventListener('sodeck:surfaceReady',this.ready);removeEventListener('sodeck:stateChanged',this.changed);}
}
class Watch {
  constructor(client,filter,change){this.client=client;this.filter=filter;this.change=change;this.records=new Map();this.active=false;this.disposed=false;this.epoch=0;this.retry=0;}
  get valid(){return this.active&&performance.now()<this.expires;}
  async start(){
    const epoch=++this.epoch;this.clientId=crypto.randomUUID();this.client.watches.set(this.clientId,this);
    try{
      const initial=await this.client.request('subscribe',{clientSubscriptionId:this.clientId,filter:this.filter});
      if(epoch!==this.epoch||this.disposed)return;
      this.id=initial.subscriptionId;this.scope=initial.scope;this.revision=initial.revision;
      const records=[...initial.records];let cursor=initial.nextCursor;
      while(cursor){const page=await this.client.request('list',{cursor});if(epoch!==this.epoch||this.disposed)return;if(page.revision!==this.revision)throw new Error('共享快照修订失步');records.push(...page.records);cursor=page.nextCursor;if(bytes(records)>4*1024*1024)throw new Error('共享快照超限');}
      for(const record of records)validRecord(record);
      this.records=new Map(records.map(r=>[identity(r),r]));this.expires=performance.now()+initial.leaseMs;this.active=true;
      await this.client.request('subscribe',{clientSubscriptionId:this.clientId,filter:this.filter,acknowledgedRevision:this.revision});
      if(epoch!==this.epoch||this.disposed)return;
      this.retry=0;this.notify();this.arm();
    }catch(error){if(epoch===this.epoch&&!this.disposed)this.reset(error);}
  }
  notify(){const epoch=this.epoch;queueMicrotask(()=>{if(epoch===this.epoch&&!this.disposed)this.change(this.valid?{scope:this.scope,revision:this.revision,records:[...this.records.values()]}:null);});}
  arm(){
    clearTimeout(this.leaseTimer);clearTimeout(this.renewTimer);
    this.leaseTimer=setTimeout(()=>this.reset(new Error('共享状态租约已到期')),Math.max(0,this.expires-performance.now()));
    this.renewTimer=setTimeout(async()=>{
      const epoch=this.epoch;try{const result=await this.client.request('subscribe',{clientSubscriptionId:this.clientId,filter:this.filter,acknowledgedRevision:this.revision});if(epoch===this.epoch&&!this.disposed){this.expires=performance.now()+result.leaseMs;this.arm();}}
      catch(error){if(epoch===this.epoch&&!this.disposed)this.reset(error);}
    },30000);
  }
  receive(frame){
    if(this.disposed)return;
    if(this.active&&performance.now()>=this.expires){this.reset(new Error('共享状态租约已到期'));return;}
    try{
      if(frame.resetRequired){this.reset(new Error('共享状态需要重新同步'));return;}
      if(!this.valid||frame.subscriptionId!==this.id||JSON.stringify(frame.context)!==JSON.stringify(this.client.context)||!revision(frame.fromRevision)||!revision(frame.toRevision)||!Number.isInteger(frame.part)||!Number.isInteger(frame.parts)||frame.parts<1||frame.parts>4096||frame.part<0||frame.part>=frame.parts)throw new Error('共享批次格式或归属无效');
      const final=frame.part+1===frame.parts;
      let applied;
      if(this.lastBatch===frame.batchId && frame.toRevision===this.revision){if(final)applied=this.revision;}
      else{
        if(frame.fromRevision!==this.revision||BigInt(frame.toRevision)<=BigInt(frame.fromRevision))throw new Error('共享批次游标失步');
        if(!this.batch)this.batch={id:frame.batchId,to:frame.toRevision,parts:frame.parts,frames:[],bytes:0};
        const batch=this.batch;
        if(batch.id!==frame.batchId||batch.to!==frame.toRevision||batch.parts!==frame.parts||frame.part>batch.frames.length)throw new Error('共享批次分帧失步');
        if(frame.part<batch.frames.length){if(JSON.stringify(batch.frames[frame.part])!==JSON.stringify(frame.changes))throw new Error('重试帧内容已改变');}
        else{batch.bytes+=bytes(frame);if(batch.bytes>1024*1024)throw new Error('共享变化积压超限');batch.frames.push(frame.changes);}
        if(final){
          const next=new Map(this.records);let scope=this.scope;
          for(const changes of batch.frames)for(const change of changes){
            if(change.op==='scope')scope=change.scope;
            else{validRecord(change.record);if(change.op==='put'&&change.record.present)next.set(identity(change.record),change.record);else if(change.op==='remove'&&!change.record.present)next.delete(identity(change.record));else throw new Error('共享变化无效');}
          }
          this.records=next;this.scope=scope;this.revision=batch.to;this.lastBatch=batch.id;this.batch=undefined;applied=this.revision;this.notify();
        }
      }
      const epoch=this.epoch;
      void this.client.request('ack',{subscriptionId:this.id,batchId:frame.batchId,part:frame.part,...(applied?{appliedRevision:applied}:{})}).catch(error=>{if(epoch===this.epoch&&!this.disposed)this.reset(error);});
    }catch(error){this.reset(error);}
  }
  reset(error){
    if(this.disposed)return;
    ++this.epoch;this.active=false;this.batch=undefined;this.records.clear();this.notify();clearTimeout(this.leaseTimer);clearTimeout(this.renewTimer);clearTimeout(this.retryTimer);
    const id=this.clientId;this.client.watches.delete(id);void this.client.request('unsubscribe',{clientSubscriptionId:id}).catch(()=>{});
    if(!['staleContext','scopeClosed','sourceStopped','unsupported'].includes(error.code))this.retryTimer=setTimeout(()=>this.start(),Math.min(5000,1000*2**this.retry++));
  }
  dispose(){if(this.disposed)return;this.disposed=true;++this.epoch;clearTimeout(this.leaseTimer);clearTimeout(this.renewTimer);clearTimeout(this.retryTimer);this.client.owned.delete(this);this.client.watches.delete(this.clientId);void this.client.request('unsubscribe',{clientSubscriptionId:this.clientId}).catch(()=>{});}
}
