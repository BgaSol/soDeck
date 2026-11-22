export interface Context {workspaceId:string;serverId:string;connectionId:string;generation:number}
export interface ApiError extends Error {code:string|number;data?:{code?:string;dispatch?:'notSent'|'unknown';[key:string]:unknown}}
export interface Mount {surfaceId:string;viewId:string;viewType:string;context:Context;readOnly:boolean;connected:boolean;visible:boolean;data:unknown;tokens:Record<string,string>;settings:unknown}
export type PageMethod='data'|'event'|'state'|'clipboard'|'settings'|'localFiles'|'openView'|'closeView';
export interface PageClient {
  readonly channel:string;
  request<T=unknown>(method:PageMethod,params:Record<string,unknown>):Promise<T>;
  prepared(flush?:boolean,localFiles?:boolean):void;
  flushed(id:string,error?:Error|string):void;
  interaction():void;
}
declare global {interface Window {sodeck:PageClient} const sodeck:PageClient}
