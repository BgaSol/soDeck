import {schema as source} from './updates.generated';
import {validate, type Shape} from './schema';
import type {ActionRequest, ComponentList, ListRequest, Operation, Plan, Policy, PolicyCommit, PolicyValidate, Status} from './updates.generated';
export type * from './updates.generated';
const schema = source as unknown as {$defs:Record<string,Shape>; 'x-methods':Record<string,string>; 'x-results':Record<string,string>};
export function validateUpdates(kind:'x-methods'|'x-results', method:string, value:unknown):string|undefined {
  const name = schema[kind][method];
  if (!name) return;
  if (encodeURIComponent(JSON.stringify(value)).replace(/%[0-9A-F]{2}/g,'x').length > 64*1024) return 'Updates payload exceeds 64 KiB';
  return validate(schema, schema.$defs[name], value, method);
}
export interface UpdatesTransport {call<T>(method:string, params?:unknown):Promise<T>}
export function updatesClient(bridge:UpdatesTransport) { return {
  status:()=>bridge.call<Status>('updates.status.get'),
  list:(p:ListRequest={})=>bridge.call<ComponentList>('updates.components.list',p),
  plan:(componentIds:string[],expectedRevision?:string)=>bridge.call<Plan>('updates.plan.get',{componentIds,...(expectedRevision?{expectedRevision}:{})}),
  check:(operationId:string)=>bridge.call<Operation>('updates.check',{operationId}),
  download:(p:ActionRequest)=>bridge.call<Operation>('updates.download',p),
  install:(p:ActionRequest)=>bridge.call<Operation>('updates.install',p),
  cancel:(operationId:string)=>bridge.call<Operation>('updates.cancel',{operationId}),
  operation:(operationId:string)=>bridge.call<Operation>('updates.operation.get',{operationId}),
  validate:(p:PolicyValidate)=>bridge.call<{valid:true}>('updates.policy.validate',p),
  commit:(p:PolicyCommit)=>bridge.call<Policy>('updates.policy.commit',p),
  policyOperation:(generation:string,operationId:string)=>bridge.call<import('./updates.generated').PolicyOperation>('updates.policy.operation.get',{generation,operationId}),
  follow:(pluginId:string,expectedDigest:string,operationId:string)=>bridge.call<Operation>('updates.source.followOfficial',{pluginId,expectedDigest,operationId}),
  restart:(planRevision:string,operationId:string)=>bridge.call<Operation>('app.restartForUpdates',{planRevision,operationId}),
}; }
