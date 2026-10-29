import {accessPathSchema as wire,type RouteStatus,type RouteMeta} from './connection-route.generated';
import {validate,type Shape} from './schema';
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateAccessPath(kind:'x-methods'|'x-results',name:string,value:unknown){
  const key=schema[kind][name];
  const error=key?validate(schema,schema.$defs[key],value,name):undefined;
  if(error||kind==='x-methods'||!key)return error;
  const v=value as Record<string,unknown>;
  if(name==='auth.challenge.get'&&((v.stepIndex as number)>=(v.stepCount as number)||(v.route as RouteMeta).stepCount!==v.stepCount))return '认证步骤索引与路径不一致';
  if(name==='connections.route.get')return statusError(value as RouteStatus);
  return undefined;
}
function statusError(value:RouteStatus):string|undefined {
  if(value.attempt&&value.attempt.stepIndex>=value.attempt.route.stepCount)return '连接阶段步骤索引超出路径';
  const routes=[value.saved,...value.running.map(r=>r.route),...(value.attempt?[value.attempt.route]:[])];
  if(routes.some(r=>r.targetServerId!==r.reference.serverId||r.routeRevision!==r.reference.routeRevision))return '路径引用与修订不一致';
  return undefined;
}
export function decodeRouteStatus(value:unknown):RouteStatus {
  const error=validate(schema,schema.$defs.RouteStatus,value,'connections.route.v2')||statusError(value as RouteStatus);
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as RouteStatus;
}
