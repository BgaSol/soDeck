const yieldTask=()=>new Promise(resolve=>setTimeout(resolve,0));
export const fold=text=>{const lower=text.toLowerCase();return lower.includes('\u03c2')?lower.replaceAll('\u03c2','\u03c3'):lower;};
export function originalOffset(text,offset,end=false){
  if(text.toLowerCase().length===text.length)return offset;
  let folded=0,original=0;
  for(const char of text){const next=folded+char.toLowerCase().length;if(offset<next)return original+(end?char.length:0);if(offset===next)return original+char.length;folded=next;original+=char.length;}
  return text.length;
}
const blankState=()=>({carry:'',segments:[],offset:0,lastEnd:0});
const cloneState=s=>({...s,segments:s.segments.map(p=>({...p}))});
const segmentAt=(segments,offset)=>segments.findLast(p=>p.start<=offset);
function compactColors(colors){const runs=[];for(let x=0;x<32;){const code=colors[x];let end=x+1;while(end<32&&colors[end]===code)end++;runs.push(end,code);x=end;}return runs.length<32?Uint32Array.from(runs):colors;}
function mergeColors(target,colors){let run=0;for(let x=0;x<32;x++){while(colors.length!==32&&x>=colors[run])run+=2;const code=colors.length===32?colors[x]:colors[run+1];if(code&&(code>>>24)!==3||!target[x])target[x]=code;}}
export class HistoryIndex {
  constructor(emit=()=>{}){this.emit=emit;this.rows=new Map();this.blocks=new Map();this.checkpoints=new Map();this.epoch=0;this.head=0;this.length=0;this.revision=0;this.serial=0;this.queryRevision=0;this.query='';this.caseSensitive=false;this.total=0;this.dirty=Infinity;this.fresh=true;this.anchorRow=0;}
  reset(epoch){this.serial++;this.epoch=epoch;this.rows.clear();this.blocks.clear();this.checkpoints.clear();this.head=0;this.length=0;this.revision=0;this.total=0;this.dirty=0;this.fresh=true;this.pendingNavigation=undefined;this.visibleRequest=undefined;}
  patch(message){
    if(message.epoch!==this.epoch)return;
    this.serial++;const blocks=new Set();
    for(const [id,row]of this.rows){if(id>=message.head)break;this.total-=row.count||0;this.rows.delete(id);blocks.add(Math.floor(id/64));}
    if(message.head!==this.head)this.trimmed=true;
    this.head=message.head;this.length=message.length;
    for(const row of message.rows){if(row.id<this.head)continue;const previous=this.rows.get(row.id);this.rows.set(row.id,{...row,colors:compactColors(row.colors),count:previous?.count||0});this.dirty=Math.min(this.dirty,row.id);blocks.add(Math.floor(row.id/64));}
    for(const block of blocks){const colors=new Uint32Array(32);let present=false;for(let id=block*64;id<(block+1)*64;id++){const row=this.rows.get(id);if(!row)continue;present=true;mergeColors(colors,row.colors);}if(present)this.blocks.set(block,colors);else this.blocks.delete(block);}
  }
  async queryFor(message){this.serial++;this.queryRevision=message.queryRevision;this.query=message.caseSensitive?message.query:fold(message.query);this.caseSensitive=message.caseSensitive;this.anchorRow=message.anchorRow;this.fresh=true;this.dirty=this.head;return this.search();}
  point(segment,offset,end=false){const row=this.rows.get(segment.id);const local=offset-segment.start;return {row:segment.id,offset:this.caseSensitive?local:originalOffset(row?.text||'',local,end)};}
  walk(row,state,hit,aggregate=false){
    if(!row.wrapped){state.carry='';state.segments=[];state.offset=state.lastEnd=0;}
    const text=this.caseSensitive?row.text:fold(row.text),start=state.offset,combined=state.carry+text,base=start-state.carry.length;
    const segments=[...state.segments,{id:row.id,start,end:start+text.length}];
    let position=combined.indexOf(this.query,Math.max(0,state.lastEnd-base)),amount=0,firstLocal;
    while(position>=0){
      const from=base+position,to=from+this.query.length;
      if(aggregate&&from>=start){amount++;firstLocal??=from;}
      else{const a=segmentAt(segments,from),b=segmentAt(segments,to-1);if(a&&b)hit(a,from,b,to,1);}
      state.lastEnd=to;position=combined.indexOf(this.query,position+this.query.length);
    }
    if(amount)hit(segments.at(-1),firstLocal,segments.at(-1),firstLocal+this.query.length,amount);
    state.offset=start+text.length;const keep=Math.min(this.query.length-1,combined.length);state.carry=keep?combined.slice(-keep):'';
    const before=state.offset-keep;state.segments=segments.filter(p=>p.end>p.start&&p.end>before);
  }
  async search(){
    if(!this.query){for(const row of this.rows.values())row.count=0;this.total=0;this.fresh=false;this.trimmed=false;this.dirty=Infinity;this.emitResult({complete:true});return;}
    const serial=++this.serial;let begin=this.head,state=blankState();
    if(!this.fresh){const checkpointId=Math.floor(this.dirty/256)*256,checkpoint=this.checkpoints.get(checkpointId);if(checkpoint&&checkpointId>=this.head&&checkpoint.state.segments.every(p=>p.id>=this.head)){begin=checkpointId;state=cloneState(checkpoint.state);}else if(this.dirty===Infinity)begin=this.head+this.length;}
    if(this.trimmed&&!this.fresh&&this.rows.get(this.head)?.wrapped){
      let end=this.head+1;while(end<this.head+this.length&&this.rows.get(end)?.wrapped)end++;
      if(end>=begin){begin=this.head;state=blankState();}
      else{const prefix=blankState();for(let id=this.head;id<end;id++){const row=this.rows.get(id);if(row){this.total-=row.count;row.count=0;}}for(let id=this.head;id<end;id++){const row=this.rows.get(id);if(row)this.walk(row,prefix,(a,from,b,to,n)=>{const match=this.rows.get(a.id);if(match){match.count+=n;this.total+=n;}},true);if(id%64===0){await yieldTask();if(serial!==this.serial)return;}}}
    }
    const clearFrom=Math.min(begin,state.segments[0]?.id??begin);
    for(let id=clearFrom;id<this.head+this.length;id++){const row=this.rows.get(id);if(row){this.total-=row.count;row.count=0;}}
    if(begin!==this.head){for(const saved of this.checkpoints.get(begin)?.prefix||[]){const row=this.rows.get(saved.id);if(row){row.count=saved.count;this.total+=saved.count;}}}
    for(const id of this.checkpoints.keys())if(id>=begin)this.checkpoints.delete(id);
    let first,firstNear,lastYield=performance.now();
    for(let id=begin;id<this.head+this.length;id++){
      if(serial!==this.serial)return;
      const row=this.rows.get(id);if(!row)continue;
      if(id%256===0)this.checkpoints.set(id,{state:cloneState(state),prefix:state.segments.map(p=>({id:p.id,count:this.rows.get(p.id)?.count||0}))});
      this.walk(row,state,(a,from,b,to,n)=>{const matched=this.rows.get(a.id);if(!matched)return;matched.count+=n;this.total+=n;if(!first||(!firstNear&&a.id>=this.anchorRow)){const match={start:this.point(a,from),end:this.point(b,to,true)};first??=match;if(a.id>=this.anchorRow)firstNear??=match;}},true);
      if(performance.now()-lastYield>4){await yieldTask();lastYield=performance.now();}
    }
    if(serial!==this.serial)return;
    this.fresh=false;this.trimmed=false;this.dirty=Infinity;this.emitResult({complete:true,first:firstNear||first});
    const navigation=this.pendingNavigation;this.pendingNavigation=undefined;if(navigation&&navigation.message.queryRevision===this.queryRevision)await this[navigation.method](navigation.message);
  }
  emitResult(extra){this.emit({type:'result',epoch:this.epoch,revision:this.revision,queryRevision:this.queryRevision,total:this.total,...extra});}
  async locate(message){
    if(message.queryRevision!==this.queryRevision||!this.query)return;
    if(this.dirty!==Infinity){this.pendingNavigation={method:'locate',message};return;}if(!this.total)return;
    let ordinal=((message.ordinal%this.total)+this.total)%this.total,target,local=ordinal;
    for(const row of this.rows.values()){if(local<row.count){target=row.id;break;}local-=row.count;}
    if(target==null)return;
    let begin=this.head,state=blankState();const saved=this.checkpoints.get(Math.floor(target/256)*256);
    if(saved){begin=Math.floor(target/256)*256;state=cloneState(saved.state);local-=saved.prefix.find(p=>p.id===target)?.count||0;}
    let match,lastYield=performance.now();const serial=this.serial;
    for(let id=begin;id<this.head+this.length&&!match;id++){
      if(serial!==this.serial)return;const row=this.rows.get(id);if(!row)continue;
      this.walk(row,state,(a,from,b,to)=>{if(a.id===target&&local--===0)match={start:this.point(a,from),end:this.point(b,to,true)};});
      if(performance.now()-lastYield>4){await yieldTask();lastYield=performance.now();}
    }
    if(match&&serial===this.serial)this.emitResult({complete:true,match,ordinal,requestId:message.requestId});
  }
  async visible(message){
    this.visibleRequest=message.requestId;
    if(message.queryRevision!==this.queryRevision||!this.query||this.dirty!==Infinity)return;
    const start=Math.max(this.head,message.start-this.query.length),end=Math.min(this.head+this.length,message.end+this.query.length);
    let begin=start;while(begin>this.head&&this.rows.get(begin)?.wrapped)begin--;
    const saved=this.checkpoints.get(Math.floor(begin/256)*256);let state=blankState();if(saved){begin=Math.floor(begin/256)*256;state=cloneState(saved.state);}
    let matches=[],first=true,lastYield=performance.now();const serial=this.serial;
    const belongs=()=>serial===this.serial&&message.requestId===this.visibleRequest;
    const context={type:'visible',epoch:this.epoch,revision:this.revision,queryRevision:this.queryRevision,requestId:message.requestId,head:this.head,start:begin,end:end-1};
    const send=complete=>{if(belongs())this.emit({...context,matches,begin:first,complete});first=false;matches=[];};
    for(let id=begin;id<end;id++){if(!belongs())return;const row=this.rows.get(id);if(!row)continue;this.walk(row,state,(a,from,b,to)=>{if(a.id<=message.end&&b.id>=message.start){matches.push({start:this.point(a,from),end:this.point(b,to,true)});if(matches.length===128)send(false);}});if(id%64===0||performance.now()-lastYield>4){await yieldTask();lastYield=performance.now();if(!belongs())return;}}
    send(true);
  }
  async near(message){
    if(message.queryRevision!==this.queryRevision||!this.query)return;
    if(this.dirty!==Infinity){this.pendingNavigation={method:'near',message};return;}if(!this.total)return;
    let target,prefix=0,first;
    for(const row of this.rows.values()){if(row.count){first??={id:row.id,prefix};if(row.id>=message.row){target={id:row.id,prefix};break;}}prefix+=row.count;}
    target??=first;if(!target)return;
    const checkpointId=Math.floor(target.id/256)*256,saved=this.checkpoints.get(checkpointId);let state=saved?cloneState(saved.state):blankState(),ordinal=target.prefix,match;
    const serial=this.serial;
    for(let id=saved?checkpointId:this.head;id<this.head+this.length&&!match;id++){
      if(serial!==this.serial)return;const row=this.rows.get(id);if(!row)continue;
      this.walk(row,state,(a,from,b,to)=>{if(a.id!==target.id||match)return;const start=this.point(a,from);if(start.row!==message.row||start.offset>=(message.offset||0))match={start,end:this.point(b,to,true)};else ordinal++;});
      if(id%64===0)await yieldTask();
    }
    if(match&&serial===this.serial){if(message.delta)await this.locate({...message,ordinal:ordinal+message.delta});else this.emitResult({complete:true,match,ordinal,requestId:message.requestId});}
    else await this.locate({...message,ordinal:target.prefix});
  }
  map(height,message={}){
    if(this.query&&this.dirty!==Infinity)return;
    height=Math.max(1,Math.min(1200,height));const colors=new Uint32Array(height*32),density=new Uint32Array(height),length=Math.max(1,this.length);
    if(length<4096){for(const row of this.rows.values()){const y=Math.min(height-1,Math.floor((row.id-this.head)/length*height));mergeColors(colors.subarray(y*32,(y+1)*32),row.colors);}}
    else for(const [block,summary]of this.blocks){const y=Math.min(height-1,Math.max(0,Math.floor((block*64-this.head)/length*height)));for(let x=0;x<32;x++){const code=summary[x],at=y*32+x;if(code&&(code>>>24)!==3||!colors[at])colors[at]=code;}}
    for(const row of this.rows.values()){if(row.count)density[Math.min(height-1,Math.floor((row.id-this.head)/length*height))]+=row.count;}
    this.emit({type:'map',epoch:this.epoch,revision:this.revision,queryRevision:this.queryRevision,requestId:message.requestId,colors,density,height,head:this.head,length:this.length},[colors.buffer,density.buffer]);
  }
}
if(typeof WorkerGlobalScope!=='undefined'&&self instanceof WorkerGlobalScope){
  const index=new HistoryIndex((message,transfer)=>postMessage(message,transfer||[]));
  self.onmessage=event=>{const message=event.data;let job;
    const context={epoch:message.epoch,revision:message.revision??index.revision,queryRevision:message.queryRevision??index.queryRevision,requestId:message.requestId,operation:message.type};
    const fail=error=>postMessage({type:'error',...context,message:String(error)});
    try{
    if(message.type==='reset')index.reset(message.epoch);
    if(message.type==='rows'){index.patch(message);postMessage({type:'ack',id:message.id});}
    if(message.epoch!==index.epoch)return;
    if(message.type==='done'&&message.epoch===index.epoch){index.revision=message.revision;job=index.search();}
    if(message.type==='query')job=index.queryFor(message);
    if(message.type==='locate')job=index.locate(message);
    if(message.type==='near')job=index.near(message);
    if(message.type==='visible')job=index.visible(message);
    if(message.type==='map')index.map(message.height,message);
    Promise.resolve(job).catch(fail);
    }catch(error){fail(error);}
  };
}
