import type {Handle} from '@sodeck/sdk/network';
import {networkOperation,type NetworkRpc,type NetworkClient} from '@sodeck/sdk/network-client';
import type {ByteSession} from '@sodeck/sdk/network-stream';
import type {Channel} from './protocol/channel';

export class CoreChannel implements Channel {
  private released=false;
  constructor(readonly net:NetworkClient,readonly group:Handle,readonly bytes:ByteSession,private onRelease:()=>void=()=>{},private rpc:NetworkRpc=net.rpc){}
  get eof(){return this.bytes.eof;}
  readable(deadline?:number){return this.bytes.readable(deadline);}
  peek(max?:number){return this.bytes.peek(max);}
  consume(n:number){this.bytes.consume(n);}
  write(bytes:Uint8Array){return this.bytes.write(bytes);}
  async close() {
    if(this.released)return;this.released=true;this.onRelease();
    this.bytes.dispose();
    try {await this.net.execute('net.streams.close',{group:this.group,stream:this.bytes.stream},networkOperation(),this.rpc);}
    catch(error){if(!['STALE_HANDLE','STALE_SCOPE','OPERATION_EXPIRED'].includes(String((error as {code?:string}).code)))throw error;}
  }
  async finish() {
    if(this.released)return;
    try {
      await this.bytes.endWrite();
      const deadline=Date.now()+5000;
      while(!this.bytes.eof){const bytes=await this.bytes.readable(deadline);this.bytes.consume(bytes.length);}
    }catch{/* Close/reset/peer timeout all converge on native cleanup. */}
    await this.close();
  }
  async splice(other:Channel) {
    if(!(other instanceof CoreChannel)||other.group.id!==this.group.id)throw Error('Unrelated network streams');
    await this.net.splice(this.group,this.bytes,other.bytes,this.rpc);this.released=true;other.released=true;this.onRelease();other.onRelease();
  }
}
