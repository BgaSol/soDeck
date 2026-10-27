import {connectionRouteSchema as wire} from './connection-route.generated';
import {connectionRouteSchema as wireV2} from './connection-route-v2.generated';
import type {RouteStatusV2,RouteMeta} from './connection-route-v2.generated';
import type {RouteStatus} from './connection-route.generated';
import {validate,type Shape} from './schema';
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
const schemaV2=wireV2 as unknown as typeof schema;
export function validateConnectionRoute(kind:'x-methods'|'x-results',name:string,value:unknown){
  const selected=name.includes('.v2.')?schemaV2:schema;
  const key=selected[kind][name];
  const error=key?validate(selected,selected.$defs[key],value,name):undefined;
  if(error||kind==='x-methods'||!key||selected!==schemaV2)return error;
  const v=value as Record<string,unknown>;
  if(name==='auth.challenge.v2.get'&&((v.hopIndex as number)>=(v.hopCount as number)||(v.route as RouteMeta).nodeCount!==v.hopCount))return '认证节点索引与路径不一致';
  if(name==='connections.route.v2.get')return statusError(value as RouteStatusV2);
  return undefined;
}
function statusError(value:RouteStatusV2):string|undefined {
  if(value.attempt&&value.attempt.hopIndex>=value.attempt.route.nodeCount)return '连接阶段节点索引超出路径';
  const routes=[value.saved,...value.running.map(r=>r.route),...(value.attempt?[value.attempt.route]:[])];
  if(routes.some(r=>r.targetServerId!==r.reference.serverId||r.routeRevision!==r.reference.routeRevision))return '路径引用与修订不一致';
  return undefined;
}
export function decodeRouteStatusV2(value:unknown):RouteStatusV2 {
  const error=validate(schemaV2,schemaV2.$defs.RouteStatusV2,value,'connections.route.v2')||statusError(value as RouteStatusV2);
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as RouteStatusV2;
}
export function decodeRouteStatus(value:unknown):RouteStatus {
  const error=validate(schema,schema.$defs.RouteStatus,value,'connections.route.v1');
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as RouteStatus;
}
