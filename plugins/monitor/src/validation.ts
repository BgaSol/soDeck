import {diagnosticReference} from '@sodeck/sdk';
import type {Options, SnapshotResult} from './protocol';
export function invalid(message: string): never {throw Object.assign(Error(message),{code:'INVALID_PARAMS'});}
export function object(value: unknown): Record<string,unknown> {if(!value||typeof value!=='object'||Array.isArray(value))return invalid('需要对象参数');return value as Record<string,unknown>;}
export function keys(value: Record<string,unknown>, names: string[]) {for(const key of Object.keys(value))if(!names.includes(key))invalid('未知参数：'+key);}
export function string(value: unknown, label='身份', maximum=256): string {if(typeof value!=='string'||!value||value.length>maximum)return invalid(label+'无效');return value;}
export function options(value: unknown): Partial<Options> {
  const o=object(value);keys(o,['refreshSeconds','rangeSeconds','paused','showLoopback','showVirtual']);
  if(o.refreshSeconds!==undefined&&![2,5,10].includes(o.refreshSeconds as number))invalid('采样周期无效');
  if(o.rangeSeconds!==undefined&&![60,300,600].includes(o.rangeSeconds as number))invalid('历史范围无效');
  for(const key of ['paused','showLoopback','showVirtual'])if(o[key]!==undefined&&typeof o[key]!=='boolean')invalid(key+'无效');
  return o;
}
export function snapshot(value: unknown, viewId: string, generation: string): SnapshotResult {
  const v=object(value);
  if(typeof v.revision!=='string'||!/^\d{1,32}$/.test(v.revision))invalid('快照修订无效');
  if(v.unchanged===true){keys(v,['unchanged','revision']);return v as unknown as SnapshotResult;}
  if(v.diagnosticRef!==undefined&&!diagnosticReference({details:{diagnosticRef:v.diagnosticRef}}))throw Error('诊断引用无效');
  keys(v,['protocolVersion','revision','viewId','serverId','serverName','workspaceGeneration','connection','status','error','diagnosticRef','visible','cleanupPending','limited','options','selection','system','cpu','memory','network','disk','mounts','selectedMount','mountCount','networkCount','diskCount','partial','historyReleased','trends','capturedAt','timelineMs','diagnostics']);
  if(v.protocolVersion!==1||v.viewId!==viewId||v.workspaceGeneration!==generation)invalid('快照不属于当前视图');
  for(const key of ['capturedAt','timelineMs','mountCount'])if(typeof v[key]!=='number'||!Number.isFinite(v[key])||v[key]<0)invalid('快照数字无效');
  options(v.options);
  for(const key of ['visible','cleanupPending','limited','partial','historyReleased'])if(typeof v[key]!=='boolean')invalid('快照状态无效');
  if(!['waiting','connecting','connected','disconnected'].includes(String(v.connection)))invalid('连接状态无效');
  string(v.serverId);string(v.serverName,'服务器名称',4096);string(v.status,'状态',4096);
  if(v.error!==undefined)string(v.error,'错误',4096);
  const system=object(v.system);keys(system,['host','kernel','os','uptime']);
  for(const key of ['host','kernel','os'])if(typeof system[key]!=='string'||system[key].length>1024)invalid('主机信息无效');
  if(system.uptime!==null&&(typeof system.uptime!=='number'||!Number.isFinite(system.uptime)||system.uptime<0))invalid('在线时长无效');
  const diagnostics=object(v.diagnostics);keys(diagnostics,['activeJobs','cacheBytes']);
  for(const key of ['activeJobs','cacheBytes'])if(!Number.isInteger(diagnostics[key])||Number(diagnostics[key])<0)invalid('诊断数值无效');
  for(const [key,max]of [['mountCount',128],['networkCount',64],['diskCount',64]] as const)if(!Number.isInteger(v[key])||Number(v[key])<0||Number(v[key])>max)invalid('对象数量无效');
  const metric=(value:unknown)=>{
    const m=object(value);keys(m,['quality','value','updatedAt','ageMs','message']);
    if(!['warming','fresh','stale','unsupported','error'].includes(String(m.quality))||!('value'in m))invalid('指标质量无效');
    for(const key of ['updatedAt','ageMs'])if(m[key]!==null&&(typeof m[key]!=='number'||!Number.isFinite(m[key])||m[key]<0))invalid('样本时间无效');
    if(m.message!==undefined)string(m.message,'指标说明',4096);return m;
  };
  const selection=object(v.selection);keys(selection,['network','disk','mount','mountPage','mountExpanded','networkPage','diskPage']);
  for(const key of ['network','disk','mount'])if(selection[key]!==null)string(selection[key]);
  for(const key of ['mountPage','networkPage','diskPage'])if(!Number.isInteger(selection[key])||Number(selection[key])<0||Number(selection[key])>15)invalid('对象分页无效');
  if(typeof selection.mountExpanded!=='boolean')invalid('明细展开状态无效');
  for(const key of ['cpu','memory','network','disk']){
    metric(v[key]);
  }
  if(!Array.isArray(v.mounts)||v.mounts.length>32)invalid('挂载列表超限');
  const number=(value:unknown,maximum=Number.MAX_SAFE_INTEGER)=>{if(typeof value!=='number'||!Number.isFinite(value)||value<0||value>maximum)invalid('指标数字无效');};
  const decimal=(value:unknown)=>{if(typeof value!=='string'||!/^\d{1,30}$/.test(value))invalid('无损计数无效');};
  const cpu=object(v.cpu).value;if(cpu!==null){const c=object(cpu);keys(c,['percent','iowait','cores','load']);if(c.percent!==null)number(c.percent,100);if(c.iowait!==null)number(c.iowait,100);if(c.cores!==null)number(c.cores,65536);if(!Array.isArray(c.load)||c.load.length>3)invalid('负载无效');for(const n of c.load)number(n);}
  const memory=object(v.memory).value;if(memory!==null){const m=object(memory);keys(m,['total','used','available','percent','estimated','swapTotal','swapUsed']);for(const key of ['total','used','available','swapTotal','swapUsed'])decimal(m[key]);number(m.percent,100);if(typeof m.estimated!=='boolean')invalid('内存估算状态无效');}
  for(const key of ['network','disk']){const rows=object(v[key]).value;if(rows===null)continue;if(!Array.isArray(rows)||rows.length>32)invalid('对象明细超限');for(const row of rows){const r=object(row);keys(r,['id','name','first','second','virtual','partition','weakIdentity']);string(r.id);string(r.name);for(const value of [r.first,r.second])if(value!==null)number(value);}}
  for(const value of [...v.mounts,...(v.selectedMount===null?[]:[v.selectedMount])]){
    const row=object(value);keys(row,['id','identity','path','device','fsType','source','shared','network','temporary','capacity','nextReadMs','readAllowed']);string(row.id);if(typeof row.path!=='string'||row.path.length>65536)invalid('挂载显示路径无效');
    for(const key of ['shared','network','temporary','readAllowed'])if(typeof row[key]!=='boolean')invalid('挂载状态无效');number(row.nextReadMs);
    const capacity=metric(row.capacity).value;if(capacity!==null){const c=object(capacity);keys(c,['total','used','available','denominator','percent','source']);for(const key of ['total','used','available','denominator'])decimal(c[key]);if(c.percent!==null)number(c.percent,100);if(!['stat','df'].includes(String(c.source)))invalid('容量来源无效');}
  }
  const trends=object(v.trends);keys(trends,['cpu','memory','networkFirst','networkSecond','diskFirst','diskSecond','capacity']);
  for(const points of Object.values(trends)){
    if(!Array.isArray(points)||points.length>301)invalid('趋势点数超限');
    let last=-Infinity;
    for(const point of points){const p=object(point);keys(p,['time','value']);if(typeof p.time!=='number'||!Number.isFinite(p.time)||p.time<last||p.time>0)invalid('趋势时序无效');last=p.time;if(p.value!==null&&(typeof p.value!=='number'||!Number.isFinite(p.value)||p.value<0))invalid('趋势数值无效');}
  }
  return v as unknown as SnapshotResult;
}
