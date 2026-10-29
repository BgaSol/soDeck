import {schema as proxy} from './proxy-catalog.generated';
import {schema as credentials} from './credential-configuration.generated';
import {schema as home} from './connection-home.generated';
import {validate,type Shape} from './schema';
export function validateConnectionConfiguration(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
  for(const schema of [proxy,credentials,home]){
    const name=(schema[kind] as Record<string,string>)[method];
    if(name)return validate(schema as unknown as {$defs:Record<string,Shape>},(schema.$defs as unknown as Record<string,Shape>)[name],value,method);
  }
}
