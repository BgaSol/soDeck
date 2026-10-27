import {validateDirectory} from './directory';
import {validateConnectionRoute} from './connection-route';
import {validatePty} from './pty';
import {validateState,type Notification,type StateBridge} from './state';
import {validateRemoteRead} from './remote-read';
import {validateNetwork} from './network';
import {validateWorkbenchRequest,validateWorkbenchResult} from './workbench';

export interface WorkerTransport {
  call<T=unknown>(method:string,params?:unknown):Promise<T>;
  onNotification(listener:(event:Notification)=>void):()=>void;
}
/** No window/DOM assumptions: the same validators serve QuickJS and WebViews. */
export function workerClient(transport:WorkerTransport):StateBridge {
  return {
    async call<T>(method:string,params:unknown={}) {
      const error=validateNetwork('x-methods',method,params)??validateConnectionRoute('x-methods',method,params) ?? validateState('x-methods',method,params) ?? validateDirectory('x-methods',method,params) ?? validatePty('x-methods',method,params)??validateRemoteRead('x-methods',method,params)??validateWorkbenchRequest(method,params);
      if(error)throw Object.assign(new Error(error),{code:'INVALID_PARAMS'});
      const result=await transport.call<T>(method,params);
      const invalid=validateNetwork('x-results',method,result)??validateConnectionRoute('x-results',method,result) ?? validateState('x-results',method,result) ?? validateDirectory('x-results',method,result) ?? validatePty('x-results',method,result)??validateRemoteRead('x-results',method,result)??validateWorkbenchResult(method,result);
      if(invalid)throw Object.assign(new Error(invalid),{code:'PROTOCOL_ERROR'});
      return result;
    },
    onNotification(listener){return transport.onNotification(event=>{const error=validateState('x-events',event.type,event.payload)??validatePty('x-events',event.type,event.payload);if(!error)listener(event);});},
  };
}
