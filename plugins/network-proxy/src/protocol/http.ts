import {authority,fault,requestTarget,sameTarget} from './address';
import {ascii,Reader} from './io';
import {body,cleanHeaders,encodeHead,framing,headers,single,tokens,type Header,type Framing} from './http-body';
import {code,write,type Channel,type Connect} from './channel';

function validateConnection(head:Header[]) {
  if(tokens(head,'connection').some(t=>['host','content-length','transfer-encoding'].includes(t)))throw fault('BAD_REQUEST','Connection 不能移除消息边界字段');
}
function responseError(error:unknown) {
  const value=code(error);
  if(value==='HEADER_TOO_LARGE')return '431 Request Header Fields Too Large';
  if(value==='BAD_REQUEST'||value==='UNEXPECTED_EOF')return '400 Bad Request';
  if(['TIMEOUT','TARGET_TIMEOUT','TARGET_CONNECT_TIMEOUT'].includes(value))return '504 Gateway Timeout';
  if(value==='UNSUPPORTED_VERSION')return '505 HTTP Version Not Supported';
  return '502 Bad Gateway';
}
function upstreamError(error:unknown):never {
  if(['BAD_REQUEST','UNEXPECTED_EOF','HEADER_TOO_LARGE'].includes(code(error)))throw fault('BAD_GATEWAY','上游 HTTP 响应格式无效');
  throw error;
}
export async function http(client:Channel,connect:Connect,initialDeadline:number):Promise<boolean> {
  const reader=new Reader(client);let deadline=initialDeadline,responseStarted=false;
  try {
    while(true) {
      responseStarted=false;
      const available=await client.readable(deadline);if(!available.length)return false;
      deadline=Math.min(deadline,Date.now()+10000);
      const line=await reader.line(8192,deadline,true);if(line===undefined)return false;
      // Once a new request starts, the remaining header gets the handshake budget.
      deadline=Math.min(deadline,Date.now()+10000);
      const match=/^([!#$%&'*+.^_`|~0-9A-Za-z-]+) (\S+) HTTP\/(\d\.\d)$/.exec(line);
      if(!match)throw fault('BAD_REQUEST','HTTP 请求行无效');
      const [,method,url,version]=match;if(version!=='1.1')throw fault('UNSUPPORTED_VERSION','需要 HTTP/1.1');
      const head=await headers(reader,deadline,65536,line.length+2);
      validateConnection(head);
      const host=single(head,'host'),requestFrame=framing(head),connection=tokens(head,'connection');
      if(method==='CONNECT') {
        const target=authority(url);
        if(!host||!sameTarget(target,authority(host,target.port))||requestFrame.kind!=='length'||requestFrame.length!==0)throw fault('BAD_REQUEST','CONNECT 目标或正文无效');
        const upstream=await connect(target);
        try {responseStarted=true;await client.write(ascii('HTTP/1.1 200 Connection Established\r\n\r\n'));await client.splice(upstream);return true;}
        catch(error){await upstream.close();throw error;}
      }
      const {target,path}=requestTarget(url,host);
      const upgrade=connection.includes('upgrade')?single(head,'upgrade'):undefined;
      if(upgrade&&!/^[a-z0-9!#$%&'*+.^_`|~-]+(?:\/[a-z0-9!#$%&'*+.^_`|~-]+)?$/i.test(upgrade))throw fault('BAD_REQUEST','Upgrade 协议无效');
      const expect=single(head,'expect');if(expect&&expect.toLowerCase()!=='100-continue')throw fault('BAD_REQUEST','Expect 字段无效');
      const upstream=await connect(target),remote=new Reader(upstream);
      let alive=true,uploaded=requestFrame.kind==='length'&&requestFrame.length===0,spliced=false;
      try {
        const outgoing=cleanHeaders(head,!!upgrade);
        if(!upgrade)outgoing.push(['connection','close']);
        await write(upstream,encodeHead(`${method} ${path} HTTP/1.1`,outgoing));
        // Reading responses concurrently allows 100 Continue and early rejection
        // while a large upload is still backpressured by the target.
        const upload=body(reader,upstream,requestFrame,()=>alive).then(()=>{uploaded=true;});
        let uploadFailure:unknown;
        const failedUpload=new Promise<never>((_,reject)=>{void upload.catch(error=>{uploadFailure=error;reject(error);});});
        // Install a handler immediately; response reads below race this promise.
        void failedUpload.catch(()=>{});
        const responseDeadline=Date.now()+30000;
        let informational=0;
        while(true) {
          const statusLine=(await Promise.race([remote.line(8192,responseDeadline).catch(upstreamError),failedUpload]))!;
          const statusMatch=/^HTTP\/1\.[01] ([1-5][0-9]{2})(?: [\x20-\x7e\x80-\xff]*)?$/.exec(statusLine);
          if(!statusMatch)throw fault('BAD_GATEWAY','上游 HTTP 状态行无效');
          const status=Number(statusMatch[1]);
          const response=await Promise.race([headers(remote,responseDeadline,65536,statusLine.length+2).catch(upstreamError),failedUpload]);
          let frame:Framing,responseUpgrade:string|undefined;
          try{validateConnection(response);frame=framing(response,true,method==='HEAD'||status===204||status===304||status<200);responseUpgrade=single(response,'upgrade');}catch(error){upstreamError(error);}
          if(status===101) {
            if(!upgrade||!tokens(response,'connection').includes('upgrade')||responseUpgrade?.toLowerCase()!==upgrade.toLowerCase()||!uploaded)throw fault('BAD_GATEWAY','上游未正确协商协议升级');
            responseStarted=true;await write(client,encodeHead('HTTP/1.1 101 Switching Protocols',cleanHeaders(response,true)));
            await client.splice(upstream);spliced=true;return true;
          }
          if(status<200) {
            if(++informational>8)throw fault('BAD_GATEWAY','上游临时响应过多');
            await write(client,encodeHead(statusLine,cleanHeaders(response)));continue;
          }
          const early=!uploaded;
          alive=!early;
          const close=early||connection.includes('close')||frame.kind==='close';
          const output=cleanHeaders(response);output.push(['connection',close?'close':'keep-alive']);
          responseStarted=true;await write(client,encodeHead(statusLine.replace(/^HTTP\/1\.0/,'HTTP/1.1'),output));
          await body(remote,client,frame);
          if(uploadFailure&&!early)throw uploadFailure;
          if(close)return false;
          break;
        }
      }finally {alive=false;if(!spliced)await upstream.close();}
      deadline=Date.now()+60000;
    }
  }catch(error) {
    if(!responseStarted) {
      const status=responseError(error),message=ascii(status+'\n');
      try{await client.write(encodeHead(`HTTP/1.1 ${status}`,[['content-type','text/plain'],['content-length',String(message.length)],['connection','close']]));await client.write(message);}catch{/* Peer already gone. */}
    }
    throw error;
  }
}
