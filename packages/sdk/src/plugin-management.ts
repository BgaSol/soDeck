import {schema as source} from './plugin-management.generated';
import {validate,type Shape} from './schema';
export type * from './plugin-management.generated';
const schema=source as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>};
export function validatePluginManagement(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
    const name=schema[kind][method];if(!name)return;
    if(encodeURIComponent(JSON.stringify(value)).replace(/%[A-F\d]{2}/gi,'x').length>64*1024)return 'Plugin management payload exceeds 64 KiB';
    return validate(schema,schema.$defs[name],value,method);
}
