import type {AccessPath,RouteStep,CatalogItem,Proxy,Group} from '@sodeck/sdk';
import type {DependencyIdentity} from '@sodeck/sdk';
export type {DependencyIdentity};
export type Phase='editing'|'validating'|'savingConfiguration'|'savingCredential'|'saved'|'opening'|'configurationError'|'credentialError'|'reconciling'|'conflict';
export type Fields={host:string;port:string;username:string;authType:'password'|'key'|'none'|'usernamePassword';name:string;groupId:string|null;pathEnabled:boolean;steps:RouteStep[];remember:boolean;dependencies:DependencyIdentity[]};
export type GroupEdit={name:string;group?:Group;operationId?:string;busy?:boolean;unknown?:boolean;error?:string};
export type InlineTarget={parentDraftId:string;targetStepId:string};
export type Draft={id:string;kind:'ssh'|'proxy';fields:Fields;baseline:string;server?:CatalogItem;proxy?:Proxy;inlineTarget?:InlineTarget;inlineId?:string;groupEdit?:GroupEdit;phase:Phase;error:string;errors:Record<string,string>;scroll:number;focus?:string;message:string};
export type Mode={kind:'guide'|'proxies'|'groups'}|{kind:'draft';id:string};
export type Catalog={items:CatalogItem[];proxies:{proxy:Proxy;referenceCount:number}[];groups:Group[];revision:string};
export const emptyCatalog:Catalog={items:[],proxies:[],groups:[],revision:'0'};
export function fields(kind:Draft['kind']):Fields{return {host:'',port:kind==='ssh'?'22':'1080',username:'',authType:kind==='ssh'?'password':'none',name:'',groupId:null,pathEnabled:false,steps:[],remember:true,dependencies:[]};}
export function accessPath(f:Fields):AccessPath {return {steps:f.pathEnabled?f.steps:[]};}
export function dependencies(f:Fields,c:Catalog):DependencyIdentity[]{
  if(!f.pathEnabled)return [];
  return f.steps.flatMap((step):DependencyIdentity[]=>{
    const id=step.kind==='ssh'?step.serverId:step.proxyId;
    const existing=f.dependencies.find(d=>d.stepId===step.stepId&&d.kind===step.kind&&d.id===id);
    if(existing)return [existing];
    const owner=step.kind==='ssh'?c.items.find(i=>i.server.id===id)?.server:c.proxies.find(i=>i.proxy.id===id)?.proxy;
    return owner?[{stepId:step.stepId,kind:step.kind,id,host:owner.host,port:owner.port,username:owner.username,authType:owner.authType}]:[];
  });
}
export function serverFields(s:CatalogItem,c:Catalog):Fields {const f:Fields={...fields('ssh'),...s.server,port:String(s.server.port),groupId:s.groupId,pathEnabled:s.accessPath.steps.length>0,steps:s.accessPath.steps.map(s=>({...s}))};f.dependencies=dependencies(f,c);return f;}
export function proxyFields(p:Proxy):Fields{return {...fields('proxy'),...p,port:String(p.port)};}
export const busy=(d:Draft)=>['validating','savingConfiguration','savingCredential','opening'].includes(d.phase);
export const locked=(d:Draft)=>busy(d)||d.phase==='reconciling';
export function endpoint(host:string,port:string){return `${host.includes(':')&&!host.startsWith('[')?`[${host}]`:host}:${port}`;}
export function generatedName(d:Pick<Draft,'kind'|'fields'>){const f=d.fields;return d.kind==='ssh'&&f.port==='22'?f.host.trim():endpoint(f.host.trim(),f.port);}
