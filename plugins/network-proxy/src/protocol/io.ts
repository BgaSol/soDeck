import {fault} from './address';
export interface Bytes {
  readonly eof:boolean;
  readable(deadline?:number):Promise<Uint8Array>;
  peek(max?:number):Uint8Array;
  consume(count:number):void;
  write(bytes:Uint8Array):Promise<void>;
}
export const ascii=(text:string)=>Uint8Array.from(text,c=>c.charCodeAt(0));
export class Reader {
  constructor(readonly stream:Bytes){}
  async exact(count:number,deadline?:number):Promise<Uint8Array> {
    if(count>65536||count<0)throw fault('PROTOCOL_ERROR','读取长度超过窗口');
    const bytes=new Uint8Array(count);let used=0;
    while(used<count){const chunk=await this.stream.readable(deadline);if(!chunk.length)throw fault('UNEXPECTED_EOF','握手数据不完整');const n=Math.min(count-used,chunk.length);bytes.set(chunk.subarray(0,n),used);this.stream.consume(n);used+=n;}
    return bytes;
  }
  async line(limit:number,deadline?:number,allowEof=false):Promise<string|undefined> {
    let text='',cr=false;
    while(true) {
      const chunk=await this.stream.readable(deadline);
      if(!chunk.length){if(allowEof&&!text&&!cr)return;throw fault('UNEXPECTED_EOF','HTTP 行不完整');}
      let used=0;
      for(const byte of chunk) {
        used++;
        if(cr){if(byte!==10)throw fault('BAD_REQUEST','HTTP 行必须使用 CRLF');this.stream.consume(used);return text;}
        if(byte===13){cr=true;continue;}
        if(byte===10||byte===0)throw fault('BAD_REQUEST','HTTP 行包含非法换行');
        text+=String.fromCharCode(byte);if(text.length>limit)throw fault('HEADER_TOO_LARGE','HTTP 行超过限制');
      }
      this.stream.consume(used);
    }
  }
}
