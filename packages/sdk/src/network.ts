import {networkSchema} from './network.generated';
import {validate,type Shape} from './schema';
export type * from './network.generated';
const schema=networkSchema as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>;'x-events':Record<string,string>};
export function validateNetwork(kind:'x-methods'|'x-results'|'x-events',method:string,value:unknown):string|undefined {
  const name=schema[kind][method];
  if(!name)return method.startsWith('net.')?'未知网络接口':undefined;
  return validate(schema,schema.$defs[name],value,method);
}
