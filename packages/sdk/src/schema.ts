export type Shape = { $ref?: string; type?: string; format?: string; enum?: readonly unknown[]; properties?: Record<string, Shape>; required?: readonly string[]; additionalProperties?: boolean; items?: Shape; minimum?: number; maximum?: number; minLength?: number; maxLength?: number; minItems?: number; maxItems?: number; pattern?: string; allOf?:Shape[]; anyOf?:Shape[]; if?:Shape; then?:Shape };
type Validator=(value:unknown,path:string)=>string|undefined;
const schemas=new WeakMap<object,WeakMap<Shape,Validator>>();
function compile(schema:{$defs:Record<string,Shape>},shape:Shape):Validator{
  let cache=schemas.get(schema);if(!cache){cache=new WeakMap();schemas.set(schema,cache);}const known=cache.get(shape);if(known)return known;
  // Placeholder permits referenced recursive shapes without compiling forever.
  let check:Validator;const validator:Validator=(value,path)=>check(value,path);cache.set(shape,validator);
  if(shape.$ref){check=compile(schema,schema.$defs[shape.$ref.split('/').at(-1)!]);return validator;}
  const all=shape.allOf?.map(s=>compile(schema,s)),any=shape.anyOf?.map(s=>compile(schema,s)),condition=shape.if&&compile(schema,shape.if),then=shape.then&&compile(schema,shape.then);
  const fields=Object.fromEntries(Object.entries(shape.properties??{}).map(([key,s])=>[key,compile(schema,s)])),item=shape.items?compile(schema,shape.items):undefined,pattern=shape.pattern===undefined?undefined:new RegExp(shape.pattern);
  const invalid=(path:string)=>path+' 不符合工作台契约';
  check=(value,path)=>{
    if(all)for(const part of all){const failure=part(value,path);if(failure)return failure;}
    if(condition&&!condition(value,path)&&then){const failure=then(value,path);if(failure)return failure;}
    if(any&&!any.some(part=>!part(value,path)))return invalid(path);
    if(shape.enum&&!shape.enum.includes(value))return invalid(path);
    switch(shape.type){
      case 'null':if(value!==null)return invalid(path);break;
      case 'object':{
        if(value===null||typeof value!=='object'||Array.isArray(value))return invalid(path);const data=value as Record<string,unknown>;
        for(const key of shape.required??[])if(data[key]===undefined)return invalid(path);
        for(const key in data){if(!Object.prototype.hasOwnProperty.call(data,key)||data[key]===undefined)continue;const field=Object.prototype.hasOwnProperty.call(fields,key)?fields[key]:undefined;
          if(!field){if(shape.additionalProperties===false)return invalid(path);continue;}
          const failure=field(data[key],path+'.'+key);if(failure)return failure;
        }break;
      }
      case 'array':{
        if(!Array.isArray(value)||value.length<(shape.minItems??0)||value.length>(shape.maxItems??Infinity))return invalid(path);
        if(item)for(let i=0;i<value.length;i++){const failure=item(value[i],path+'['+i+']');if(failure)return failure;}break;
      }
      case 'string':{
        if(typeof value!=='string')return invalid(path);
        if(shape.minLength!==undefined||shape.maxLength!==undefined){let length=value.length;for(let i=0;i<value.length-1;i++){const c=value.charCodeAt(i);if(c>=0xd800&&c<=0xdbff){const next=value.charCodeAt(i+1);if(next>=0xdc00&&next<=0xdfff){length--;i++;}}}if(length<(shape.minLength??0)||length>(shape.maxLength??Infinity))return invalid(path);}
        if(pattern&&!pattern.test(value))return invalid(path);
        if(shape.format==='uint64'&&(!/^(0|[1-9][0-9]{0,19})$/.test(value)||value.length===20&&value>'18446744073709551615'))return invalid(path);break;
      }
      case 'number':case 'integer':if(typeof value!=='number'||!Number.isFinite(value)||(shape.type==='integer'&&!Number.isSafeInteger(value))||value<(shape.minimum??-Infinity)||value>(shape.maximum??Infinity))return invalid(path);break;
      case 'boolean':if(typeof value!=='boolean')return invalid(path);break;
    }
  };return validator;
}
export function validate(schema:{$defs:Record<string,Shape>},shape:Shape,value:unknown,path:string):string|undefined{return compile(schema,shape)(value,path);}
