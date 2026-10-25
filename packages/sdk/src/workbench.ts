import { workbenchSchema } from "./workbench.generated";
export type * from "./workbench.generated";
type Shape = { $ref?: string; type?: string; enum?: readonly unknown[]; properties?: Record<string, Shape>; required?: readonly string[]; additionalProperties?: boolean; items?: Shape; minimum?: number; maximum?: number; minLength?: number; maxLength?: number; minItems?: number; maxItems?: number; pattern?: string; allOf?:Shape[]; anyOf?:Shape[]; if?:Shape; then?:Shape };
const schema = workbenchSchema as unknown as { $defs: Record<string, Shape>; "x-methods": Record<string, string>; "x-results":Record<string,string>; "x-events":Record<string,string> };
export function validateWorkbenchRequest(method: string, value: unknown): string | undefined {
  const name = schema["x-methods"][method]; return name ? validate(schema.$defs[name], value, method) : undefined;
}
export function validateWorkbenchResult(method:string,value:unknown):string|undefined {const name=schema['x-results'][method];return name?validate(schema.$defs[name],value,method):undefined;}
export function validateWorkbenchEvent(method:string,value:unknown):string|undefined {const name=schema['x-events'][method];return name?validate(schema.$defs[name],value,method):undefined;}
function validate(s: Shape, value: unknown, path: string): string | undefined {
  if (s.$ref) return validate(schema.$defs[s.$ref.split("/").at(-1)!], value, path);
  for(const part of s.allOf??[]) {const error=validate(part,value,path);if(error)return error;}
  if(s.if && !validate(s.if,value,path) && s.then){const error=validate(s.then,value,path);if(error)return error;}
  const error = `${path} 不符合工作台契约`;
  if(s.anyOf&&!s.anyOf.some(part=>!validate(part,value,path)))return error;
  if(s.type==='null'&&value!==null)return error;
  if (s.enum && !s.enum.includes(value)) return error;
  if (s.type === "object") {
    if (value === null || typeof value !== "object" || Array.isArray(value)) return error;
    const data = value as Record<string, unknown>;
    for (const key of s.required ?? []) if (data[key] === undefined) return error;
    for (const [key, item] of Object.entries(data)) {
      if (item === undefined) continue;
      const field = s.properties?.[key]; if (!field) { if (s.additionalProperties === false) return error; continue; }
      const failure = validate(field, item, `${path}.${key}`); if (failure) return failure;
    }
  } else if (s.type === "array") {
    if (!Array.isArray(value) || value.length < (s.minItems ?? 0) || value.length > (s.maxItems ?? Infinity)) return error;
    for (let i = 0; i < value.length; i++) { const failure = validate(s.items ?? {}, value[i], `${path}[${i}]`); if (failure) return failure; }
  } else if (s.type === "string") {
    if (typeof value !== "string" || [...value].length < (s.minLength ?? 0) || [...value].length > (s.maxLength ?? Infinity) || (s.pattern !== undefined && !new RegExp(s.pattern).test(value))) return error;
  } else if (s.type === "number" || s.type === "integer") {
    if (typeof value !== "number" || !Number.isFinite(value) || (s.type === "integer" && !Number.isSafeInteger(value)) || value < (s.minimum ?? -Infinity) || value > (s.maximum ?? Infinity)) return error;
  } else if (s.type === "boolean" && typeof value !== "boolean") return error;
}
