import {qualitySchema} from './quality.generated';
import {validate,type Shape} from './schema';
import type {SummaryRequest,QualityState} from './quality.generated';
export type * from './quality.generated';
const schema=qualitySchema as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateQuality(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
  const name=schema[kind][method];
  return name?validate(schema,schema.$defs[name],value,method):`未知连接诊断接口 ${method}`;
}
/** Validate every boundary; generic core.call<T> alone cannot validate incoming data. */
export async function callQuality<T>(call:(method:string,params:unknown)=>Promise<unknown>,method:string,params:unknown):Promise<T> {
  const request=validateQuality('x-methods',method,params);if(request)throw Object.assign(new Error(request),{code:'INVALID_PARAMS'});
  const value=await call(method,params),result=validateQuality('x-results',method,value);
  if(result)throw Object.assign(new Error(result),{code:'PROTOCOL_ERROR'});return value as T;
}
export async function qualityObjectId(p:SummaryRequest):Promise<string> {
  const invalid=validateQuality('x-methods','quality.summary',p);if(invalid)throw new Error(invalid);
  const hash=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify([p.configEpoch,p.serverId??''])));
  return Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');
}
export function decodeQualityState(value:unknown):QualityState {
  const invalid=validate(schema,schema.$defs.QualityState,value,'communication.quality.v1');
  if(invalid)throw Object.assign(new Error(invalid),{code:'PROTOCOL_ERROR'});return value as QualityState;
}
