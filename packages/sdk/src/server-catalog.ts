import {catalogSchema} from './server-catalog.generated';
import {validate,type Shape} from './schema';
export type {Group,CatalogItem,CatalogPage,GroupsPage,CatalogSave,OperationResult} from './server-catalog.generated';
export function validateCatalog(kind:'x-methods'|'x-results',method:string,value:unknown):string|undefined {
  const name=(catalogSchema[kind] as Record<string,string>)[method];
  return name?validate(catalogSchema as unknown as {$defs:Record<string,Shape>},(catalogSchema.$defs as unknown as Record<string,Shape>)[name],value,method):undefined;
}
