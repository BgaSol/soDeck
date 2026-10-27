import {workbenchUiSchema} from './workbench-ui.generated';
import {validate,type Shape} from './schema';
export type {WorkbenchContributionsV2,PluginMetadata,ToolContribution,TabCreateContribution,DialogResult} from './workbench-ui.generated';
export function validateWorkbenchUi(kind:'x-methods'|'x-results'|'x-events',method:string,value:unknown):string|undefined {
  const name=(workbenchUiSchema[kind] as Record<string,string>)[method];
  return name?validate(workbenchUiSchema as unknown as {$defs:Record<string,Shape>},(workbenchUiSchema.$defs as unknown as Record<string,Shape>)[name],value,method):undefined;
}
