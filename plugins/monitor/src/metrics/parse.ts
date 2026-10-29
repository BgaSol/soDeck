import type {Counter, Cpu, Memory, FastData, Inventory, Mount, Capacity} from '../protocol';
import type {Sections} from '../collectors/framing';
import {displayText} from '../collectors/framing';
const unsigned = (value: string | undefined): bigint => {if(!value||!/^\d{1,30}$/.test(value))throw Error('无效整数');return BigInt(value);};
const percent = (used: bigint, total: bigint) => total>0n ? Number(used*1000n/total)/10 : null;
export function parseCpu(text: string): bigint[] {const fields=text.trim().split(/\s+/);if(fields.shift()!=='cpu'||fields.length<4)throw Error('缺少CPU计数');return Array.from({length:8},(_,i)=>unsigned(fields[i]??'0'));}
export function cpuDifference(current: bigint[], previous: bigint[]): Pick<Cpu,'percent'|'iowait'> {
  const delta=current.map((v,i)=>v-previous[i]);
  if(delta.some(v=>v<0n))return {percent:null,iowait:null};
  const total=delta.reduce((a,b)=>a+b,0n);if(total<=0n)return {percent:null,iowait:null};
  return {percent:percent(total-delta[3]-delta[4],total),iowait:percent(delta[4],total)};
}
export function parseMemory(text: string): Memory {
  const values: Record<string,bigint>={};
  const expected=new Set(['MemTotal','MemFree','MemAvailable','Buffers','Cached','SReclaimable','Shmem','SwapTotal','SwapFree']);
  for(const row of text.split('\n')){const key=row.split(':',1)[0];if(!expected.has(key))continue;const m=/^([A-Za-z_()]+):\s+(\d+)\s+kB\s*$/.exec(row);if(!m||values[key]!==undefined)throw Error('内存字段无效');values[key]=unsigned(m[2])*1024n;}
  const total=values.MemTotal;if(!total||values.MemFree===undefined)throw Error('缺少内存总量/空闲量');
  const estimated=values.MemAvailable===undefined;
  let available=values.MemAvailable??values.MemFree+(values.Buffers??0n)+(values.Cached??0n)+(values.SReclaimable??0n)-(values.Shmem??0n);
  if(estimated)available=available<0n?0n:available>total?total:available;
  if(available<0n||available>total)throw Error('可用内存无效');
  const swapTotal=values.SwapTotal??0n,swapFree=values.SwapFree??0n;if(swapFree>swapTotal)throw Error('Swap计数无效');
  return {total:String(total),used:String(total-available),available:String(available),percent:percent(total-available,total)!,estimated,swapTotal:String(swapTotal),swapUsed:String(swapTotal-swapFree)};
}
export function parseNet(text: string, inventory?: Inventory): Counter[] {
  const rows: Counter[]=[];
  for(const line of text.split('\n')){if(!line.includes(':'))continue;const at=line.lastIndexOf(':'),name=line.slice(0,at).trim(),fields=line.slice(at+1).trim().split(/\s+/);if(!name||name.length>128||fields.length<16)throw Error('无效网卡计数');const meta=inventory?.interfaces[name];rows.push({id:meta?.id??'name:'+name,name,first:String(unsigned(fields[0])),second:String(unsigned(fields[8])),weakIdentity:!meta,virtual:meta?.virtual??true});}
  return rows;
}
export function parseDisk(text: string, inventory?: Inventory): Counter[] {
  const rows:Counter[]=[];
  for(const line of text.trim().split('\n')){if(!line.trim())continue;const f=line.trim().split(/\s+/);if(f.length<14)throw Error('无效磁盘计数');const key=String(unsigned(f[0]))+':'+String(unsigned(f[1])),name=f[2],meta=inventory?.devices[name];rows.push({id:meta?.id??key+':'+name,name,first:String(unsigned(f[5])*512n),second:String(unsigned(f[9])*512n),weakIdentity:!meta,partition:meta?.partition});}
  return rows;
}
export function parseFast(sections: Sections, inventory?: Inventory, selected?: {network:string|null;disk:string|null}): FastData {
  const errors={...sections.errors};
  const read=<T>(name:string,fn:(value:string)=>T,fallback:T):T=>{try{if(sections.values[name]===undefined)return fallback;return fn(sections.values[name]);}catch(error){errors[name]=String((error as Error).message);return fallback;}};
  const before=read('before',t=>Number(t.trim().split(/\s+/)[0]),null as number|null),after=read('after',t=>Number(t.trim().split(/\s+/)[0]),null as number|null);
  const uptime=before!==null&&after!==null&&Number.isFinite(before)&&Number.isFinite(after)&&before>=0&&after>=before&&after-before<=2?(before+after)/2:null;
  const net=read('net',v=>parseNet(v,inventory),[]),disk=read('disk',v=>parseDisk(v,inventory),[]);
  return {cpu:read('cpu',parseCpu,null as bigint[]|null),memory:read('memory',parseMemory,null as Memory|null),load:read('load',v=>{const f=v.trim().split(/\s+/).slice(0,3).map(Number);if(f.length!==3||f.some(n=>!Number.isFinite(n)||n<0))throw Error('无效负载');return f;},[]),uptime,net:prioritize(net,selected?.network,64),disk:prioritize(disk,selected?.disk,64),errors,partial:sections.partial||net.length>64||disk.length>64||Object.keys(errors).length>0};
}
export function prioritize<T extends {id:string}>(rows:T[], id:string|null|undefined, maximum:number):T[]{const selected=rows.find(r=>r.id===id),first=rows.slice(0,maximum);if(selected&&!first.includes(selected))first[maximum-1]=selected;return first;}
const pseudo=new Set(['proc','sysfs','devpts','cgroup','cgroup2','securityfs','debugfs','tracefs','pstore','configfs','mqueue','hugetlbfs','fusectl','binfmt_misc','autofs']);
const unescape=(s:string)=>s.replace(/\\(040|011|012|134)/g,(_,oct)=>String.fromCharCode(parseInt(oct,8)));
export function parseMounts(text: string): Mount[] {
  const result:Mount[]=[],filesystems=new Set<string>();
  for(const line of text.split('\n')){
    if(!line)continue;const separator=line.indexOf(' - ');if(separator<0)throw Error('无效挂载信息');const a=line.slice(0,separator).split(' '),b=line.slice(separator+3).split(' ');
    if(a.length<6||b.length<3||!/^\d+$/.test(a[0])||!/^\d+:\d+$/.test(a[2]))throw Error('无效挂载字段');
    const rawPath=unescape(a[4]),fsType=b[0],source=unescape(b[1]);if(pseudo.has(fsType))continue;
    if(!rawPath.startsWith('/')||rawPath.includes('\0'))throw Error('无效挂载路径');
    const fs=a[2]+':'+fsType+':'+source,shared=filesystems.has(fs);filesystems.add(fs);
    result.push({id:a[0],identity:[a[0],a[2],a[3],a[4],fsType,b[1]].join('|'),rawPath,path:displayText(rawPath),device:a[2],fsType,source:displayText(source),shared,network:/^(nfs|nfs4|cifs|smb3|fuse\.sshfs|ceph|glusterfs)$/.test(fsType),temporary:['tmpfs','ramfs','devtmpfs'].includes(fsType)});
  }
  return result.sort((a,b)=>a.rawPath==='/'?-1:b.rawPath==='/'?1:a.path.localeCompare(b.path));
}
export function parseCores(text: string): number {
  const ranges=text.trim().split(','),seen=new Set<number>();if(ranges.length>4096)throw Error('CPU列表过大');
  for(const range of ranges){const m=/^(\d+)(?:-(\d+))?$/.exec(range);if(!m)throw Error('CPU列表无效');const a=Number(m[1]),b=Number(m[2]??m[1]);if(a>b||b>65535)throw Error('CPU列表无效');for(let n=a;n<=b;n++)seen.add(n);}
  return seen.size;
}
export function parseInventory(sections: Sections, selected?: string|null): Inventory {
  const values=sections.values;if(values.os===undefined)throw Error('未取得系统信息');if(values.os.trim()!=='Linux')throw Object.assign(Error('当前远端系统未支持；首版支持 Linux'),{code:'UNSUPPORTED_OS'});
  const mounts=values.mounts===undefined?[]:parseMounts(values.mounts),interfaces:Inventory['interfaces']={},devices:Inventory['devices']={};
  const netRows=(values.interfaces??'').trim().split('\n'),diskRows=(values.devices??'').trim().split('\n');
  for(const row of netRows.slice(0,64)){const m=/^(\S{1,128}) (\d+) ([01])$/.exec(row);if(m)interfaces[m[1]]={id:m[2]+':'+m[1],virtual:m[3]==='0'};}
  for(const row of diskRows.slice(0,64)){const m=/^(\S{1,128}) (\d+:\d+) ([01])$/.exec(row);if(m)devices[m[1]]={id:m[2]+':'+m[1],partition:m[3]==='1'};}
  let cores:number|null=null;try{cores=parseCores(values.online??'');}catch{/* Optional topology does not suppress CPU totals. */}
  return {os:'Linux',host:displayText((values.host??'').trim()),kernel:displayText((values.kernel??'').trim()),boot:(values.boot??'').trim(),cores,mounts:prioritize(mounts,selected,128),interfaces,devices,partial:sections.partial||mounts.length>128||netRows.length>64||diskRows.length>64};
}
export function parseCapacity(sections: Sections, mount: Mount): Capacity {
  const before=parseMounts(sections.values.before??''),after=parseMounts(sections.values.after??'');
  if(before[0]?.identity!==mount.identity||after[0]?.identity!==mount.identity)throw Error('挂载已经变化，请等待刷新');
  const text=sections.values.capacity;if(!text)throw Error(sections.errors.capacity??'容量不可用');
  let total:bigint,used:bigint,available:bigint,source:'stat'|'df';
  const m=/^STAT\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*$/.exec(text);
  if(m){const size=unsigned(m[1]),blocks=unsigned(m[2]),free=unsigned(m[3]),avail=unsigned(m[4]);if(size===0n||free>blocks||avail>free)throw Error('文件系统容量无效');total=size*blocks;used=size*(blocks-free);available=size*avail;source='stat';}
  else {
    if(!text.startsWith('DF\n'))throw Error('容量格式不支持');
    const matches=[...text.matchAll(/(?:^|\n)([^\n]+?)\s+(\d+)\s+(\d+)\s+(\d+)\s+\d+%\s+([^\n]+)(?=\n|$)/g)];
    if(matches.length!==1||matches[0][5]!==mount.rawPath)throw Error('无法无损核对df容量路径');
    total=unsigned(matches[0][2])*1024n;used=unsigned(matches[0][3])*1024n;available=unsigned(matches[0][4])*1024n;source='df';
    if(used>total||available>total-used)throw Error('文件系统容量无效');
  }
  return {total:String(total),used:String(used),available:String(available),denominator:String(used+available),percent:percent(used,used+available),source};
}
