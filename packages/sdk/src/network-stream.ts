import type {Attachment,Cursor,Handle} from './network';
import {Kind,WINDOW,MAX_OFFSET,frame,parse,type Frame} from './network-wire';

export interface BinaryTransport {readonly version:number;send(bytes:Uint8Array):boolean;onFrame(listener:(bytes:Uint8Array)=>void):()=>void}
type Wake=()=>void;
const error=(code:string,message:string)=>Object.assign(new Error(message),{code});
const sleep=(ms:number)=>new Promise<void>(resolve=>setTimeout(resolve,ms));

/** One manager per business worker, including frames arriving before RPC replies. */
export class NetworkStreams {
  private sessions=new Map<number,ByteSession>();
  private early=new Map<number,Uint8Array[]>();
  private retired=new Map<number,number>();
  private bytes=0;
  private unsubscribe:()=>void;
  constructor(readonly transport:BinaryTransport) {
    if(transport.version!==1)throw error('UNSUPPORTED_API','当前运行环境不支持字节流');
    this.unsubscribe=transport.onFrame(bytes=>{
      const f=parse(bytes),session=this.sessions.get(f.slot);
      if(this.retired.get(f.slot)===f.epoch)return;
      if(session){session.receive(f);return;}
      if(this.early.size>=512&&!this.early.has(f.slot))throw error('CAPACITY','待接入字节会话过多');
      this.reserve(bytes.byteLength);
      const list=this.early.get(f.slot)??[];list.push(bytes);this.early.set(f.slot,list);
    });
  }
  reserve(bytes:number) {if(this.bytes+bytes>16*1024*1024)throw error('CAPACITY','字节会话缓冲超过 16 MiB');this.bytes+=bytes;}
  release(bytes:number) {this.bytes-=bytes;if(this.bytes<0)throw Error('Unbalanced byte budget');}
  get bufferedBytes(){return this.bytes;}
  attach(stream:Handle,attachment:Attachment):ByteSession {
    if(this.sessions.has(attachment.slot))throw error('PROTOCOL_ERROR','字节会话重复接入');
    const session=new ByteSession(this,stream,attachment);this.sessions.set(attachment.slot,session);
    const early=this.early.get(attachment.slot)??[];this.early.delete(attachment.slot);
    for(const bytes of early){this.release(bytes.byteLength);session.receive(parse(bytes));}
    return session;
  }
  retire(session:ByteSession) {
    this.sessions.delete(session.attachment.slot);this.retired.set(session.attachment.slot,session.attachment.epoch);
    // Slot IDs never repeat in one process; late frames only need a bounded tail.
    if(this.retired.size>1024)this.retired.delete(this.retired.keys().next().value!);
  }
  dispose() {
    this.unsubscribe();for(const session of [...this.sessions.values()])session.dispose();
    for(const frames of this.early.values())for(const bytes of frames)this.release(bytes.byteLength);
    this.early.clear();this.retired.clear();
  }
}

export class ByteSession {
  private chunks:{bytes:Uint8Array;start:number;charge:number}[]=[];
  private consumed:number;private received:number;private written:number;private acknowledged:number;private credit:number;
  private final:number|undefined;private failed:Error|undefined;private frozen=false;private disposed=false;private writing=false;private ended=false;
  private wakes=new Set<Wake>();private controls=new Map<number,number>();private retry:ReturnType<typeof setTimeout>|undefined;
  constructor(private manager:NetworkStreams,readonly stream:Handle,readonly attachment:Attachment) {
    this.consumed=this.received=attachment.readOffset;this.written=this.acknowledged=attachment.writeOffset;this.credit=attachment.writeCredit;
  }
  get available(){return this.received-this.consumed;}
  get eof(){return this.final===this.received&&this.available===0;}
  private check(){if(this.failed)throw this.failed;if(this.disposed||this.frozen)throw error('STALE_HANDLE','字节会话已移交或结束');}
  private wake(){for(const wake of [...this.wakes])wake();}
  private fail(message:string){this.failed=error('PROTOCOL_ERROR',message);this.wake();}
  receive(f:Frame) {
    if(this.disposed||f.epoch!==this.attachment.epoch)return;
    if(f.kind===Kind.Reset){this.failed=error('TRANSPORT_LOST','字节通道已结束');this.clearChunks();this.wake();return;}
    if(f.kind===Kind.Data) {
      if(f.offset!==this.received||f.offset+f.payload.length>this.consumed+WINDOW||this.final!==undefined&&f.offset+f.payload.length>this.final){this.fail('接收偏移或窗口无效');return;}
      this.manager.reserve(f.payload.byteLength);
      this.chunks.push({bytes:f.payload,start:0,charge:f.payload.byteLength});this.received+=f.payload.length;
    } else if(f.kind===Kind.Credit) {
      if(f.offset>this.acknowledged+WINDOW){this.fail('发送额度超出已确认窗口');return;}
      this.credit=Math.max(this.credit,f.offset);
    } else if(f.kind===Kind.Ack) {
      if(f.offset>this.written){this.fail('发送确认超出已提交字节');return;}
      this.acknowledged=Math.max(this.acknowledged,f.offset);
    } else if(f.kind===Kind.Fin) {
      if(f.offset<this.received||f.offset>this.consumed+WINDOW||this.final!==undefined&&this.final!==f.offset){this.fail('读取 EOF 偏移无效');return;}
      this.final=f.offset;
    }
    this.wake();
  }
  peek(max=WINDOW):Uint8Array {
    this.check();const chunk=this.chunks[0];return chunk?chunk.bytes.subarray(chunk.start,chunk.start+max):new Uint8Array();
  }
  consume(count:number) {
    this.check();if(!Number.isSafeInteger(count)||count<0||count>this.available)throw error('INVALID_PARAMS','消费长度超出读取窗口');
    this.consumed+=count;
    while(count>0){const chunk=this.chunks[0],n=Math.min(count,chunk.bytes.length-chunk.start);chunk.start+=n;count-=n;if(chunk.start===chunk.bytes.length){this.chunks.shift();this.manager.release(chunk.charge);}}
    this.controls.set(Kind.Ack,this.consumed);this.controls.set(Kind.Credit,Math.min(MAX_OFFSET,this.consumed+WINDOW));this.flush();
  }
  private flush() {
    if(this.disposed)return;
    for(const kind of [Kind.Ack,Kind.Credit,Kind.Fin]) {
      const offset=this.controls.get(kind);if(offset===undefined)continue;
      if(!this.manager.transport.send(frame(kind,this.attachment.slot,this.attachment.epoch,offset))) {
        if(this.retry===undefined)this.retry=setTimeout(()=>{this.retry=undefined;this.flush();},1);
        return;
      }
      this.controls.delete(kind);
    }
  }
  private async changed(deadline?:number):Promise<void> {
    this.check();
    if(deadline!==undefined&&Date.now()>=deadline)throw error('TIMEOUT','等待字节通道超时');
    await new Promise<void>((resolve,reject)=>{
      let timer:ReturnType<typeof setTimeout>|undefined;
      const wake=()=>{this.wakes.delete(wake);if(timer!==undefined)clearTimeout(timer);resolve();};this.wakes.add(wake);
      if(deadline!==undefined)timer=setTimeout(()=>{this.wakes.delete(wake);reject(error('TIMEOUT','等待字节通道超时'));},Math.max(0,deadline-Date.now()));
    });
    this.check();
  }
  async readable(deadline?:number):Promise<Uint8Array> {
    while(!this.available&&!this.eof)await this.changed(deadline);
    return this.peek();
  }
  async write(bytes:Uint8Array):Promise<void> {
    this.check();if(this.writing||this.ended)throw error('INVALID_STATE','字节流写入需要串行且不能在 EOF 之后继续');
    if(bytes.length>WINDOW)throw error('INVALID_PARAMS','单次写入最多 64 KiB');
    if(!bytes.length)return;
    this.manager.reserve(bytes.length*2+24);this.writing=true;
    const copy=bytes.slice();
    try {
      let offset=0;
      while(offset<copy.length) {
        while(this.credit===this.written)await this.changed();
        this.check();const n=Math.min(copy.length-offset,this.credit-this.written);
        const encoded=frame(Kind.Data,this.attachment.slot,this.attachment.epoch,this.written,copy.subarray(offset,offset+n));
        while(!this.manager.transport.send(encoded)){this.check();await sleep(1);}
        this.written+=n;offset+=n;
        while(this.acknowledged<this.written)await this.changed();
      }
    } finally {this.writing=false;this.manager.release(bytes.length*2+24);}
  }
  async endWrite():Promise<void> {
    this.check();if(this.writing)throw error('INVALID_STATE','请等待写入结束');
    this.ended=true;this.controls.set(Kind.Fin,this.written);this.flush();
    while(this.controls.has(Kind.Fin)){this.check();await sleep(1);}
  }
  freeze():Cursor {
    this.check();if(this.writing)throw error('INVALID_STATE','写入尚未确认，不能移交');
    this.frozen=true;return {stream:this.stream,epoch:this.attachment.epoch,consumed:this.consumed,written:this.written};
  }
  private clearChunks(){for(const chunk of this.chunks)this.manager.release(chunk.charge);this.chunks=[];}
  /** Native close/splice owns socket cleanup. Disposal only releases worker copies. */
  dispose() {
    if(this.disposed)return;this.disposed=true;this.clearChunks();this.controls.clear();
    if(this.retry!==undefined)clearTimeout(this.retry);this.manager.retire(this);this.wake();
  }
}
