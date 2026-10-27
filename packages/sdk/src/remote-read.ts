import { remoteReadSchema } from './remote-read.generated';
import { validate, type Shape } from './schema';
export type * from './remote-read.generated';
const schema = remoteReadSchema as unknown as { $defs: Record<string, Shape>; 'x-methods': Record<string,string>; 'x-results': Record<string,string> };
export function validateRemoteRead(kind:'x-methods'|'x-results', method:string, value:unknown):string|undefined {
  const name=schema[kind][method];
  return name ? validate(schema,schema.$defs[name],value,method) : undefined;
}
