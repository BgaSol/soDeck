import {it,expect} from 'vitest';
import {verifyPathView,processIdentity} from './linux-path-view';
import {encode,decode,utf8} from './bytes';
import type {Remote} from './remote-client';
const path=(s:string)=>({bytesBase64:encode(utf8(s))});
const evidence={pid:'10',start:'123',uid:'1000',mountNamespace:'mnt:[1]',rootDevice:'65025',rootInode:'2'};
const stat=(pid:string,start:string,name='shell')=>`${pid} (${name}) `+Array.from({length:20},(_,i)=>i===0?'S':i===19?start:'0').join(' ')+'\n';
function remote(overrides:Record<string,string>={},wrongExec=false){
 const files:Record<string,string>={'/proc/10/stat':stat('10','123'),'/proc/self/stat':stat('20','124'),'/proc/10/status':'Uid:\t1000\t1000\t1000\t1000','/proc/self/status':'Uid:\t1000\t1000\t1000\t1000','/proc/self/mountinfo':'29 1 254:1 / / rw - ext4 /dev/vda1 rw\n','/proc/self/fdinfo/4':'pos:\t0\nflags:\t0100000\nmnt_id:\t29\nino:\t2\n',...overrides};
 return {async readFile(p:any){const key=new TextDecoder().decode(decode(p.bytesBase64));if(!(key in files))throw Error(key);return utf8(files[key]);},
  async call(_m:string,p:any){const key=new TextDecoder().decode(decode(p.path.bytesBase64));return {path:path(key.endsWith('/ns/mnt')?'mnt:[1]':key.endsWith('/cwd')?'/tmp':'/')};},
  async openDirectory(){return {directoryId:'d',cursor:0};},async page(){return {eof:true,entries:[{name:path('4')}]};},async close(){},
  async exec(){return utf8('65025:2\n\0'+stat('10','123')+'\0'+stat('20',wrongExec?'999':'124'));}} as unknown as Remote;
}
it('parses proc names with spaces, parentheses and newlines without truncating identity',()=>{expect(processIdentity(stat('10','123','a ) b\nc'))).toEqual({pid:'10',start:'123'});});
it('proves the SFTP root through its own descriptor without sibling access to non-dumpable proc root',async()=>{await expect(verifyPathView(remote(),evidence,path('/tmp'))).resolves.toBeUndefined();});
it('rejects roots, PID reuse and differing exec proc views even when paths have the same text',async()=>{
 await expect(verifyPathView(remote({'/proc/self/fdinfo/4':'mnt_id:\t29\nino:\t99\n'}),evidence,path('/tmp'))).rejects.toMatchObject({code:'PATH_UNMAPPABLE'});
 await expect(verifyPathView(remote({'/proc/10/stat':stat('10','999')}),evidence,path('/tmp'))).rejects.toMatchObject({code:'PATH_UNMAPPABLE'});
 await expect(verifyPathView(remote({},true),evidence,path('/tmp'))).rejects.toMatchObject({code:'PATH_UNMAPPABLE'});
});
