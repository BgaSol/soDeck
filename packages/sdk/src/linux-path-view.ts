// Optional Linux/proc adapter; no native host dependency or plugin-private API.
import {Remote,fault} from './remote-client';
import {encode,utf8,type RemotePath} from './bytes';
import type {Evidence} from './directory';
export const byteText=(bytes:Uint8Array)=>{let result='';for(let i=0;i<bytes.length;i+=8192)result+=String.fromCharCode(...bytes.subarray(i,i+8192));return result;};
const path=(text:string):RemotePath=>({bytesBase64:encode(utf8(text))});
export function processIdentity(text:string){
  const end=text.lastIndexOf(') '),pid=text.slice(0,text.indexOf(' (')),start=text.slice(end+2).trim().split(/\s+/)[19];
  if(end<0||!/^\d+$/.test(pid)||!/^\d+$/.test(start??''))throw fault('PATH_UNMAPPABLE','无法核对远端进程身份');
  return {pid,start};
}
export async function verifyPathView(remote:Remote,evidence:Evidence,physical:RemotePath):Promise<void>{
  if(!/^[1-9][0-9]{0,9}$/.test(evidence.pid))throw fault('PATH_UNMAPPABLE','无效 shell 进程');
  const prefix='/proc/'+evidence.pid;
  const read=async(p:string)=>byteText(await remote.readFile(path(p),65536));
  const before=processIdentity(await read(prefix+'/stat'));
  const self=processIdentity(await read('/proc/self/stat'));
  if(before.start!==evidence.start)throw fault('PATH_UNMAPPABLE','Shell 进程已换代');
  const ns=byteTextFromPath((await remote.call<{path:RemotePath}>('files.readLink',{path:path(prefix+'/ns/mnt')})).path);
  const ownNs=byteTextFromPath((await remote.call<{path:RemotePath}>('files.readLink',{path:path('/proc/self/ns/mnt')})).path);
  const uid=(await read(prefix+'/status')).match(/^Uid:\s+(\d+)\s+(\d+)/m)?.[2];
  const ownUid=(await read('/proc/self/status')).match(/^Uid:\s+(\d+)\s+(\d+)/m)?.[2];
  const cwd=(await remote.call<{path:RemotePath}>('files.readLink',{path:path(prefix+'/cwd')})).path;
  if(ns!==evidence.mountNamespace||ownNs!==ns||uid!==evidence.uid||ownUid!==uid||cwd.bytesBase64!==physical.bytesBase64)throw fault('PATH_UNMAPPABLE','Shell 与文件浏览的目录或身份不同');
  // Verify actual /proc roots independently; never trust hostname or reported
  // root alone. The SFTP pid is identified before and after this separate exec.
  // OpenSSH marks sftp-server non-dumpable: a sibling exec cannot stat its
  // /proc/PID/root even for the same UID. Prove its root using its own open
  // directory descriptor and fdinfo, read through that exact SFTP session.
  const root = await remote.openDirectory(path('/'));
  try {
    const info = await sftpRoot(remote);
    const dev = BigInt(evidence.rootDevice);
    const device = (((dev >> 8n) & 0xfffn) | ((dev >> 32n) & 0xfffff000n)) + ':' +
      ((dev & 0xffn) | ((dev >> 12n) & 0xffffff00n));
    if(info.inode!==evidence.rootInode||info.device!==device)throw fault('PATH_UNMAPPABLE','Shell 与 SFTP 可见根不同');
    // NUL separators preserve arbitrary newlines in Linux process names.
    const command=`LC_ALL=C; export LC_ALL; stat -Lc '%d:%i' -- /proc/${evidence.pid}/root && printf '\\0' && cat /proc/${evidence.pid}/stat && printf '\\0' && cat /proc/${self.pid}/stat`;
    const pieces=byteText(await remote.exec(command)).split('\0');
    if(pieces.length!==3||pieces[0]!==`${evidence.rootDevice}:${evidence.rootInode}\n`)throw fault('PATH_UNMAPPABLE','Shell 根目录已变化');
    const identities=pieces.slice(1).map(processIdentity);
    if(identities[0].pid!==before.pid||identities[0].start!==before.start||identities[1].pid!==self.pid||identities[1].start!==self.start)throw fault('PATH_UNMAPPABLE','执行环境的进程视图不同');
    const after=await read(info.path);
    if(after.match(/^ino:\s*(\d+)$/m)?.[1]!==info.inode||after.match(/^mnt_id:\s*(\d+)$/m)?.[1]!==info.mountId)throw fault('PATH_UNMAPPABLE','核对期间根目录已变化');
  } finally {await remote.close(root.directoryId);}
  const after=processIdentity(await read(prefix+'/stat'));
  const ownAfter=processIdentity(await read('/proc/self/stat'));
  if(after.pid!==before.pid||after.start!==before.start||ownAfter.pid!==self.pid||ownAfter.start!==self.start)throw fault('PATH_UNMAPPABLE','核对期间进程已变化');
  const finalCwd=(await remote.call<{path:RemotePath}>('files.readLink',{path:path(prefix+'/cwd')})).path;
  if(finalCwd.bytesBase64!==physical.bytesBase64)throw fault('PATH_UNMAPPABLE','核对期间目录已变化');
}
import {decode} from './bytes';
function byteTextFromPath(p:RemotePath){return byteText(decode(p.bytesBase64));}

async function sftpRoot(remote:Remote){
  const mounts=byteText(await remote.readFile(path('/proc/self/mountinfo')));
  const handle=await remote.openDirectory(path('/proc/self/fd'));
  try {
    const page=await remote.page(handle.directoryId,0);
    if(!page.eof)throw fault('PATH_UNMAPPABLE','SFTP 描述符超过核对范围');
    for(const entry of page.entries){
      const fd=byteTextFromPath(entry.name);if(!/^\d+$/.test(fd))continue;
      const link=await remote.call<{path:RemotePath}>('files.readLink',{path:path('/proc/self/fd/'+fd)}).catch(()=>null);
      if(!link||byteTextFromPath(link.path)!=='/')continue;
      const p='/proc/self/fdinfo/'+fd,raw=byteText(await remote.readFile(path(p),65536));
      const inode=raw.match(/^ino:\s*(\d+)$/m)?.[1],id=raw.match(/^mnt_id:\s*(\d+)$/m)?.[1];
      const mount=mounts.split('\n').map(line=>line.split(' ')).find(fields=>fields[0]===id&&fields[4]==='/');
      if(inode&&mount)return {inode,device:mount[2],path:p,mountId:id};
    }
    throw fault('PATH_UNMAPPABLE','无法核对 SFTP 根目录句柄');
  } finally {await remote.close(handle.directoryId);}
}
