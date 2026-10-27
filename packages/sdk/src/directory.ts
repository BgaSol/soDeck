import {schema as wire} from './directory-context.generated';
import {validate,type Shape} from './schema';
import type {Directory} from './directory-context.generated';
export type * from './directory-context.generated';
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validateDirectory(kind:'x-methods'|'x-results',name:string,value:unknown){
  const definition=schema[kind][name];return definition?validate(schema,schema.$defs[definition],value,name):undefined;
}
export function directory(value:unknown):Directory {
  const error=validate(schema,schema.$defs.Directory,value,'workspace.directory.v1');
  if(error)throw Object.assign(new Error(error),{code:'PROTOCOL_ERROR'});
  return value as Directory;
}
