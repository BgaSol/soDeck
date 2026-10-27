import type {Bytes} from './io';
import type {Target} from './address';

/** The adapter owns core handles; protocols only see bounded bytes. */
export interface Channel extends Bytes {
  close():Promise<void>;
  splice(other:Channel):Promise<void>;
}
export type Connect=(target:Target)=>Promise<Channel>;
export function code(error:unknown):string {return String((error as {code?:string})?.code??'TARGET_FAILED');}
export async function write(channel:Bytes,bytes:Uint8Array) {
  for(let offset=0;offset<bytes.length;offset+=65536)await channel.write(bytes.subarray(offset,offset+65536));
}
