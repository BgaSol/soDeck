import {trafficSchema} from './traffic.generated';
import {historySchema} from './history.generated';
import {validate,type Shape} from './schema';
import type {Summary,SummaryRequest} from './traffic.generated';
export type * from './traffic.generated';
export type {HistoryRequest,HistoryResult,HistoryPoint,LiveHistoryResult,StoredHistoryResult,CompareRequest,CompareResult,Policy} from './history.generated';
const schema=trafficSchema as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateTraffic(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
  const selected=schema[kind][method]?schema:historySchema as unknown as typeof schema;
  const name=selected[kind][method];
  return name?validate(selected,selected.$defs[name],value,method):undefined;
}
export function decodeTrafficSummary(value:unknown):Summary {
  const error=validateTraffic('x-results','communication.summary',value);
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as Summary;
}
/** Same canonical tuple as Core::communication_object_id. No caller identity comes from this key. */
export async function communicationObjectId(p:SummaryRequest):Promise<string> {
  const error=validateTraffic('x-methods','communication.summary',p);if(error)throw new Error(error);
  const t=p.target;
  const tuple=[p.layer??'channel',t?.configEpoch??'current',t?.kind??'all',t?.kind==='server'?t.serverId:'',t?.kind==='resource'?t.bootId:'',t?.kind==='resource'?t.resourceId:'',t?.kind==='resource'?t.generation:''];
  const hash=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(tuple)));
  return Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');
}
/** Raw byte values never pass through Number, including totals beyond 2^53. */
export function byteCount(value:string):bigint {
  if(!/^(0|[1-9][0-9]{0,19})$/.test(value))throw new RangeError('无效字节计数');
  const result=BigInt(value);
  if(result>18446744073709551615n)throw new RangeError('字节计数超出范围');
  return result;
}
export function formatByteCount(value:string,locale='zh-CN'):string {
  let bytes=byteCount(value),unit=0;
  const units=['B','KiB','MiB','GiB','TiB','PiB','EiB'];
  let divisor=1n;
  while(unit<units.length-1&&bytes>=divisor*1024n){divisor*=1024n;unit++;}
  const tenths=(bytes*10n+divisor/2n)/divisor;
  return unit===0?`${bytes.toLocaleString(locale)} B`:`${(tenths/10n).toLocaleString(locale)}.${tenths%10n} ${units[unit]}`;
}
export function formatTrafficRate(value:number|null,unit:'bytes'|'bits'='bytes'):string {
  if(value===null||!Number.isFinite(value)||value<0)return '—';
  const base=unit==='bits'?1000:1024,units=unit==='bits'?['bit/s','kbit/s','Mbit/s','Gbit/s','Tbit/s']:['B/s','KiB/s','MiB/s','GiB/s','TiB/s'];
  let rate=value*(unit==='bits'?8:1),index=0;
  while(rate>=base&&index<units.length-1){rate/=base;index++;}
  return `${rate.toLocaleString('zh-CN',{maximumFractionDigits:index?1:0})} ${units[index]}`;
}
