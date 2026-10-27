import {decode,encode,type RemotePath} from '@sodeck/sdk/bytes';
import type {Evidence} from '@sodeck/sdk/directory';
export type Report={sequence:string;status:'busy'}|{sequence:string;status:'ready';logical:RemotePath;physical:RemotePath;evidence:Evidence};
function path(encoded:string,delimiter=false):RemotePath {
  let bytes=decode(encoded);
  if(delimiter){if(bytes.at(-1)!==10)throw Error('Missing pwd delimiter');bytes=bytes.subarray(0,-1);}
  if(bytes[0]!==47||bytes.length>4096||bytes.includes(0))throw Error('Invalid byte directory');
  return {bytesBase64:encode(bytes)};
}
export class DirectoryProtocol {
  private frame:number[]=[];
  private state:'text'|'escape'|'osc'|'end'='text';
  constructor(private tag:string,private accept:(r:Report)=>void,private invalid:()=>void){}
  reset(){this.frame=[];this.state='text';}
  push(bytes:Uint8Array){
    for(const b of bytes){
      if(this.state==='text'){if(b===27)this.state='escape';continue;}
      if(this.state==='escape'){this.state=b===93?'osc':b===27?'escape':'text';this.frame=[];continue;}
      if(this.state==='end'){
        if(b===92){this.complete();this.reset();continue;}
        this.reset();if(b===27)this.state='escape';continue;
      }
      if(b===27){this.state='end';continue;}
      if(b===7){this.complete();this.reset();continue;}
      if(this.frame.length>=16384){this.invalid();this.reset();continue;}
      this.frame.push(b);
    }
  }
  private complete(){
    const fields=String.fromCharCode(...this.frame).split(';');
    if(fields[0]!=='633'||fields[1]!=='SoDeck'||fields[2]!=='1'||fields[3]!==this.tag)return;
    try{
      const sequence=fields[4];if(!/^[1-9][0-9]{0,18}$/.test(sequence))throw Error('Bad sequence');
      if(fields.length===6&&fields[5]==='busy'){this.accept({sequence,status:'busy'});return;}
      if(fields.length!==14||fields[5]!=='ready')throw Error('Bad report');
      const [pid,start,uid,mountNamespace,rootDevice,rootInode]=fields.slice(6,12);
      if(!/^[1-9][0-9]{0,9}$/.test(pid)||![start,uid,rootDevice,rootInode].every(s=>/^[0-9]{1,20}$/.test(s))||!/^mnt:\[\d+\]$/.test(mountNamespace))throw Error('Bad evidence');
      this.accept({sequence,status:'ready',logical:path(fields[12]),physical:path(fields[13],true),evidence:{pid,start,uid,mountNamespace,rootDevice,rootInode}});
    }catch{this.invalid();}
  }
}
