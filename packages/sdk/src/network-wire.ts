/** Private framing exposed only to the native worker byte transport. */
export const WINDOW=65536,HEADER=24,MAX_OFFSET=Number.MAX_SAFE_INTEGER;
export const Kind={Data:1,Ack:2,Credit:3,Fin:4,Reset:5} as const;
export interface Frame {kind:number;slot:number;epoch:number;offset:number;payload:Uint8Array}
export function frame(kind:number,slot:number,epoch:number,offset:number,payload=new Uint8Array()):Uint8Array {
  if(!Number.isInteger(kind)||kind<1||kind>5||![slot,epoch].every(v=>Number.isInteger(v)&&v>0&&v<=0xffffffff)
    ||!Number.isSafeInteger(offset)||offset<0||offset>MAX_OFFSET-payload.length||payload.length>WINDOW
    ||(kind===Kind.Data?payload.length===0:payload.length!==0)||(kind===Kind.Reset&&offset!==0))throw Error('Invalid binary frame');
  const bytes=new Uint8Array(HEADER+payload.length),view=new DataView(bytes.buffer);
  bytes.set([0,1,kind,0]);view.setUint32(4,slot);view.setUint32(8,epoch);
  view.setUint32(12,Math.floor(offset/4294967296));view.setUint32(16,offset%4294967296);view.setUint32(20,payload.length);
  bytes.set(payload,HEADER);return bytes;
}
export function parse(bytes:Uint8Array):Frame {
  if(bytes.length<HEADER||bytes[0]!==0||bytes[1]!==1||bytes[3]!==0)throw Error('Invalid binary frame header');
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  const kind=bytes[2],slot=view.getUint32(4),epoch=view.getUint32(8),offset=view.getUint32(12)*4294967296+view.getUint32(16),size=view.getUint32(20);
  if(kind<1||kind>5||slot===0||epoch===0||!Number.isSafeInteger(offset)||offset>MAX_OFFSET-size||size>WINDOW||bytes.length!==HEADER+size
    ||(kind===Kind.Data?size===0:size!==0)||(kind===Kind.Reset&&offset!==0))throw Error('Invalid binary frame');
  return {kind,slot,epoch,offset,payload:bytes.subarray(HEADER)};
}
