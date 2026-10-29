import {schema as source} from './settings.generated';
import {validate, type Shape} from './schema';
import type {SettingField, SettingPage, SettingsDescriptor, SettingEntries, SettingsSnapshot, SettingsPageResult, SettingsCatalogResult, SettingsChangeRequest, SettingsCommitRequest, SettingsCommitResult, SettingsOperationRequest, SettingsOperationResult, SettingsContext} from './settings.generated';
export type * from './settings.generated';
const schema=source as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>;'x-events':Record<string,string>};
export const settingsBytes=(value:unknown)=>encodeURIComponent(JSON.stringify(value)).replace(/%[0-9A-F]{2}/g,'x').length;
export function validateSettings(kind:'x-methods'|'x-results'|'x-events',method:string,value:unknown):string|undefined {
  if(schema[kind][method]&&settingsBytes(value)>64*1024)return 'Settings payload exceeds 64 KiB';
  const name=schema[kind][method];return name?validate(schema,schema.$defs[name],value,method):undefined;
}
export function acceptsSetting(field:SettingField,value:unknown):boolean {
  switch(field.type){
    case 'boolean':return typeof value==='boolean';
    case 'integer':return typeof value==='number'&&Number.isSafeInteger(value)&&value>=(field.minimum??-Number.MAX_SAFE_INTEGER)&&value<=(field.maximum??Number.MAX_SAFE_INTEGER);
    case 'text':return typeof value==='string'&&[...value].length<=(field.maxLength??4096);
    case 'enum':return typeof value==='string'&&!!field.options?.some(o=>o.value===value);
  }
}
export const settingFields=(page:SettingPage)=>page.sections.flatMap(s=>s.fields);
export function validateSettingsDescriptor(input:unknown,pluginId:string):SettingsDescriptor {
  const error=validate(schema,schema.$defs.SettingsDescriptor,input,'settings');if(error)throw Error(error);
  const descriptor=input as SettingsDescriptor;
  if(descriptor.pluginId!==pluginId||settingsBytes(input)>128*1024)throw Error('设置描述身份或容量无效');
  const pages=new Set<string>(),keys=new Set<string>();
  for(const page of descriptor.pages){
    const fields=settingFields(page),byKey=new Map(fields.map(f=>[f.key,f]));
    if(settingsBytes(page)>48*1024||settingsBytes(fields.map(f=>({key:f.key,value:f.default})))>16*1024)throw Error('设置页超出容量');
    if(!page.title.trim()||page.sections.some(s=>!s.title.trim()||s.fields.some(f=>!f.label.trim()||f.keywords?.some(k=>!k.trim())||f.options?.some(o=>!o.value.trim()||!o.label.trim()))))throw Error('设置文案不能为空白');
    if(pages.has(page.id)||fields.length>64||new Set(page.sections.map(s=>s.id)).size!==page.sections.length)throw Error('设置页重复或超出容量');pages.add(page.id);
    for(const field of fields){
      if(keys.has(field.key)||!acceptsSetting(field,field.default)||field.type!=='integer'&&(field.minimum!==undefined||field.maximum!==undefined)||field.type!=='text'&&field.maxLength!==undefined||field.type!=='enum'&&field.options!==undefined||(field.minimum??-Infinity)>(field.maximum??Infinity)||field.options&&new Set(field.options.map(o=>o.value)).size!==field.options.length)throw Error('设置字段无效：'+field.key);
      keys.add(field.key);
    }
    const done=new Set<string>();
    const visit=(key:string,active:Set<string>)=>{
      if(done.has(key))return;if(active.has(key))throw Error('设置条件存在循环');active.add(key);
      const field=byKey.get(key)!;
      for(const c of [field.visibleWhen,field.enabledWhen])if(c){const target=byKey.get(c.key);if(!target||!acceptsSetting(target,c.equals))throw Error('设置条件引用无效');visit(c.key,active);}
      active.delete(key);done.add(key);
    };
    fields.forEach(f=>visit(f.key,new Set()));
  }
  return descriptor;
}
export function validateSettingChanges(page:SettingPage,changes:SettingEntries){
  const fields=new Map(settingFields(page).map(f=>[f.key,f])),seen=new Set<string>();
  return changes.flatMap(e=>{const field=fields.get(e.key),invalid=seen.has(e.key)||!field||!acceptsSetting(field,e.value);seen.add(e.key);return invalid?[{key:e.key,message:'请输入有效值'}]:[];});
}
export interface SettingsTransport {call<T>(method:string,params?:unknown):Promise<T>}
export function settingsClient(bridge:SettingsTransport){return {
  catalog:(params:{cursor?:string;limit?:number}={})=>bridge.call<SettingsCatalogResult>('settings.catalog.list',params),
  page:(pageId:string)=>bridge.call<SettingsPageResult>('settings.page.get',{pageId}),
  validate:(request:SettingsChangeRequest)=>bridge.call<{errors:{key:string;message:string}[]}>('settings.validate',request),
  commit:(request:SettingsCommitRequest)=>bridge.call<SettingsCommitResult>('settings.commit',request),
  operation:(request:SettingsOperationRequest)=>bridge.call<SettingsOperationResult>('settings.operation.get',request),
  sync:(pageId:string,settingsRevision:string)=>bridge.call<SettingsSnapshot>('settings.sync.retry',{pageId,settingsRevision}),
  open:(operationId:string,pageId?:string)=>bridge.call<SettingsContext>('ui.settings.open',{operationId,...(pageId?{pageId}:{})}),
};}
