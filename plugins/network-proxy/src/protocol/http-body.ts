import {Reader,ascii,type Bytes} from './io';
import {fault} from './address';
export type Header=[string,string];
export type Framing={kind:'length';length:number}|{kind:'chunked'}|{kind:'close'};
const token=/^[!#$%&'*+.^_`|~0-9a-z-]+$/i;
export function parseHeader(line:string):Header {
  const colon=line.indexOf(':');if(colon<=0)throw fault('BAD_REQUEST','HTTP 字段格式无效');
  const name=line.slice(0,colon).toLowerCase(),value=line.slice(colon+1).trim();
  if(!token.test(name)||/[^\t\x20-\x7e\x80-\xff]/.test(value))throw fault('BAD_REQUEST','HTTP 字段包含非法字符');
  return [name,value];
}
export function values(headers:Header[],name:string){return headers.filter(h=>h[0]===name).map(h=>h[1]);}
export function single(headers:Header[],name:string):string|undefined {
  const found=values(headers,name);if(found.length>1)throw fault('BAD_REQUEST',`HTTP ${name} 重复`);return found[0];
}
export function tokens(headers:Header[],name:string):string[] {
  const list=values(headers,name).flatMap(v=>v.toLowerCase().split(',').map(s=>s.trim()));
  if(list.some(s=>!token.test(s)))throw fault('BAD_REQUEST','HTTP 连接字段无效');return list;
}
export async function headers(reader:Reader,deadline?:number,limit=65536,initial=0):Promise<Header[]> {
  const result:Header[]=[];let bytes=initial;
  while(true){const line=(await reader.line(Math.min(65536,limit-bytes),deadline))!;bytes+=line.length+2;if(bytes>limit)throw fault('HEADER_TOO_LARGE','HTTP 字段超过上限');if(!line)return result;if(result.length>=100)throw fault('HEADER_TOO_LARGE','HTTP 字段超过 100 项');result.push(parseHeader(line));}
}
export function framing(head:Header[],response=false,noBody=false):Framing {
  const length=single(head,'content-length'),transfer=single(head,'transfer-encoding');
  if(length!==undefined&&transfer!==undefined)throw fault('BAD_REQUEST','Content-Length 与 Transfer-Encoding 不能同时出现');
  if(transfer!==undefined&&transfer.toLowerCase()!=='chunked')throw fault('BAD_REQUEST','仅支持 chunked 传输编码');
  let n=0;if(length!==undefined){if(!/^[0-9]+$/.test(length))throw fault('BAD_REQUEST','Content-Length 无效');n=Number(length);if(!Number.isSafeInteger(n))throw fault('BAD_REQUEST','Content-Length 超出范围');}
  if(noBody)return {kind:'length',length:0};
  return transfer!==undefined?{kind:'chunked'}:length!==undefined?{kind:'length',length:n}:response?{kind:'close'}:{kind:'length',length:0};
}
export function cleanHeaders(head:Header[],upgrade=false):Header[] {
  const remove=new Set(['connection','proxy-connection','proxy-authorization','proxy-authenticate','keep-alive','te','trailer','upgrade',...tokens(head,'connection')]);
  const result=head.filter(([name])=>!remove.has(name));
  if(upgrade){const value=single(head,'upgrade');if(value)result.push(['upgrade',value],['connection','Upgrade']);}
  return result;
}
export function encodeHead(line:string,headers:Header[]):Uint8Array {return ascii(line+'\r\n'+headers.map(([k,v])=>`${k}: ${v}\r\n`).join('')+'\r\n');}
async function copy(reader:Reader,dest:Bytes,count:number,alive:()=>boolean):Promise<void> {
  let batch=0;
  while(count>0) {
    if(!alive())throw fault('CANCELED','正文转发已结束');
    const chunk=await reader.stream.readable();if(!alive())throw fault('CANCELED','正文转发已结束');if(!chunk.length)throw fault('UNEXPECTED_EOF','HTTP 正文长度不足');
    const n=Math.min(chunk.length,count);await dest.write(chunk.subarray(0,n));reader.stream.consume(n);count-=n;batch+=n;
    if(batch>=262144){batch=0;await new Promise<void>(resolve=>setTimeout(resolve,0));}
  }
}
export async function body(reader:Reader,dest:Bytes,frame:Framing,alive:()=>boolean=()=>true):Promise<void> {
  if(frame.kind==='length'){await copy(reader,dest,frame.length,alive);return;}
  if(frame.kind==='close') {
    let batch=0;
    while(alive()){const chunk=await reader.stream.readable();if(!chunk.length)return;await dest.write(chunk);reader.stream.consume(chunk.length);batch+=chunk.length;if(batch>=262144){batch=0;await new Promise<void>(r=>setTimeout(r,0));}}
    throw fault('CANCELED','正文转发已结束');
  }
  while(alive()) {
    const line=(await reader.line(4096))!;
    const match=/^([0-9a-f]+)(?:;[\x20-\x7e]*)?$/i.exec(line);
    if(!match)throw fault('BAD_REQUEST','chunk 长度无效');
    const size=parseInt(match[1],16);if(!Number.isSafeInteger(size))throw fault('BAD_REQUEST','chunk 长度超出范围');
    await dest.write(ascii(size.toString(16)+'\r\n'));
    if(size===0){
      const trailers=await headers(reader,undefined,16384);
      const forbidden=new Set(['content-length','transfer-encoding','host','connection','trailer','authorization','cookie','set-cookie','proxy-authorization','proxy-connection','upgrade']);
      if(trailers.some(([name])=>forbidden.has(name)))throw fault('BAD_REQUEST','trailer 包含禁止字段');
      await dest.write(ascii(trailers.map(([k,v])=>`${k}: ${v}\r\n`).join('')+'\r\n'));return;
    }
    await copy(reader,dest,size,alive);const end=await reader.exact(2);if(end[0]!==13||end[1]!==10)throw fault('BAD_REQUEST','chunk 结束符无效');await dest.write(end);
  }
  throw fault('CANCELED','正文转发已结束');
}
