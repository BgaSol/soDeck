import {fault,validHost,type Target} from './address';
import {Reader} from './io';
import {code,type Channel,type Connect} from './channel';

function reply(status:number){return Uint8Array.of(5,status,0,1,0,0,0,0,0,0);}
function status(error:unknown) {
  const value=code(error);
  if(['TIMEOUT','TARGET_TIMEOUT','TARGET_CONNECT_TIMEOUT'].includes(value))return 6;
  if(value==='CONNECTION_REFUSED'||value==='TARGET_REFUSED')return 5;
  if(value==='DNS_FAILED')return 4;
  return 1;
}
export async function socks(client:Channel,connect:Connect,deadline:number):Promise<boolean> {
  const reader=new Reader(client),hello=await reader.exact(2,deadline);
  if(hello[0]!==5||!hello[1])throw fault('BAD_REQUEST','SOCKS5 协商无效');
  const methods=await reader.exact(hello[1],deadline);
  if(!methods.includes(0)){await client.write(Uint8Array.of(5,255));return false;}
  await client.write(Uint8Array.of(5,0));
  const head=await reader.exact(4,deadline);
  if(head[0]!==5||head[2]!==0){await client.write(reply(1));return false;}
  if(head[1]!==1){await client.write(reply(7));return false;}
  let host:string;
  if(head[3]===1)host=[...await reader.exact(4,deadline)].join('.');
  else if(head[3]===4){const bytes=await reader.exact(16,deadline);host=Array.from({length:8},(_,i)=>((bytes[2*i]<<8)|bytes[2*i+1]).toString(16)).join(':');}
  else if(head[3]===3){const size=(await reader.exact(1,deadline))[0];host=String.fromCharCode(...await reader.exact(size,deadline));}
  else {await client.write(reply(8));return false;}
  const bytes=await reader.exact(2,deadline),target:Target={host,port:(bytes[0]<<8)|bytes[1]};
  if(!validHost(host)||!target.port){await client.write(reply(8));return false;}
  let upstream:Channel;
  try{upstream=await connect(target);}catch(error){await client.write(reply(status(error)));throw error;}
  try {
    await client.write(reply(0));
    await client.splice(upstream);
    return true;
  }catch(error){await upstream.close();throw error;}
}
