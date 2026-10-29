import {observabilitySchema} from './observability.generated';
import {validate,type Shape} from './schema';
export type * from './observability.generated';
const schema=observabilitySchema as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateObservability(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
  const name=schema[kind][method];return name?validate(schema,schema.$defs[name],value,method):`未知观测接口 ${method}`;
}
export async function callObservability<T>(call:(method:string,params:unknown)=>Promise<unknown>,method:string,params:unknown):Promise<T> {
  const request=validateObservability('x-methods',method,params);if(request)throw Object.assign(new Error(request),{code:'INVALID_PARAMS'});
  const value=await call(method,params),result=validateObservability('x-results',method,value);
  if(result)throw Object.assign(new Error(result),{code:'PROTOCOL_ERROR'});return value as T;
}

export {createObservation,observedAction,observeHistory,callObservation,validateObservation,ObservationClient,ObservationSampler,DiagnosticEntities} from './observation';
export type {DiagnosticMetadata,Invocation,ObservationTransport,ObservationRuntime,ProducerHealth} from './observation';
export * as observationContract from './observation.generated';
