import {NetworkStreams,ByteSession} from '../../../packages/sdk/src/network-stream';
import {NetworkClient} from '../../../packages/sdk/src/network-client';
import type {Handle,GroupSummary} from '../../../packages/sdk/src/network';
declare const sodeck:ConstructorParameters<typeof NetworkClient>[0]&{binary:ConstructorParameters<typeof NetworkStreams>[0]};
const scope={workspaceId:'binary-test',workspaceGeneration:'one'};
let streams:NetworkStreams,client:NetworkClient,group:Handle|undefined,bytes=0,failure='',paused=false,running=false;
const sleep=(ms:number)=>new Promise<void>(resolve=>setTimeout(resolve,ms));
async function accept(listener:Handle) {
  while(running) {
    const summary=await sodeck.call<GroupSummary>('net.groups.get',{scope,group});
    if(summary.pendingAccept>0){const r=await client.execute<{items:{stream:Handle}[]}>('net.listeners.accept',{group,listener,maxItems:1});if(r.items[0])return r.items[0].stream;}
    await sleep(5);
  }
  throw Error('stopped');
}
async function echo(listener:Handle) {
  const stream=await accept(listener),session=await client.exchange(group!,stream);
  try {
    while(running) {
      while(paused&&running)await sleep(5);
      const chunk=await session.readable();if(!chunk.length)break;
      await session.write(chunk);bytes+=chunk.length;session.consume(chunk.length);
      if(bytes%262144===0)await sleep(0);
    }
    if(running)await session.endWrite();
  } finally {session.dispose();}
}
async function splice(listener:Handle) {
  const left=await accept(listener),a=await client.exchange(group!,left);
  let header='';
  while(header.length<5){const chunk=await a.readable(),n=Math.min(5-header.length,chunk.length);for(const byte of chunk.subarray(0,n))header+=String.fromCharCode(byte);a.consume(n);}
  if(header!=='HELLO')throw Error('wrong handshake');
  const right=await accept(listener);await a.write(new Uint8Array([82,69,65,68,89]));
  await client.splice(group!,a,right);
}
Object.assign(globalThis,{sodeckPlugin:{
  prepare(){streams=new NetworkStreams(sodeck.binary);client=new NetworkClient(sodeck,scope,streams);},
  async invoke(command:string,p:{mode?:string;paused?:boolean}) {
    if(command==='start') {
      group=(await client.execute<{group:Handle}>('net.groups.create',{viewId:'binary-view'})).group;
      const l=await client.execute<{listener:Handle;address:{port:number}}>('net.listeners.open',{group,host:'127.0.0.1',port:0});
      await client.execute('net.listeners.setAccepting',{group,listener:l.listener,enabled:true});running=true;
      void (p.mode==='splice'?splice(l.listener):echo(l.listener)).catch(error=>{if(running)failure=String(error.message);});
      return {port:l.address.port};
    }
    if(command==='pause'){paused=!!p.paused;return true;}
    if(command==='stats')return {bytes,failure,buffered:streams.bufferedBytes};
    throw Error('unknown command');
  },
  async stop(){running=false;if(group)await client.execute('net.groups.close',{group});streams.dispose();}
}});
