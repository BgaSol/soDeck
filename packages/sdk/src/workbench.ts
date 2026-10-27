import { workbenchSchema } from "./workbench.generated";
export type * from "./workbench.generated";
import { validate, type Shape } from "./schema";
const schema = workbenchSchema as unknown as { $defs: Record<string, Shape>; "x-methods": Record<string, string>; "x-results":Record<string,string>; "x-events":Record<string,string> };
export function validateWorkbenchRequest(method: string, value: unknown): string | undefined {
  const name = schema["x-methods"][method]; return name ? validate(schema, schema.$defs[name], value, method) : undefined;
}
export function validateWorkbenchResult(method:string,value:unknown):string|undefined {const name=schema['x-results'][method];return name?validate(schema, schema.$defs[name],value,method):undefined;}
export function validateWorkbenchEvent(method:string,value:unknown):string|undefined {const name=schema['x-events'][method];return name?validate(schema, schema.$defs[name],value,method):undefined;}
