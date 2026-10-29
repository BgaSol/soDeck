import {validateSettings} from './settings';
import {validateUpdates} from './updates';
import {validateDirectory} from './directory';
import {validateObservation,type DiagnosticMetadata} from './observation';
import {validateAccessPath} from './connection-route';
import {validatePty} from './pty';
import {validateState,type Notification,type StateBridge} from './state';
import {validateRemoteRead} from './remote-read';
import {validateNetwork} from './network';
import {validateWorkbenchRequest,validateWorkbenchResult} from './workbench';

export interface WorkerTransport {
  readonly observationCreditBytes?:number;
  call<T=unknown>(method:string,params?:unknown,metadata?:DiagnosticMetadata):Promise<T>;
  onNotification(listener:(event:Notification)=>void):()=>void;
}
/** No window/DOM assumptions: the same validators serve QuickJS and WebViews. */
export function workerClient(transport:WorkerTransport):WorkerTransport {
  return {
    get observationCreditBytes(){return transport.observationCreditBytes;},
    async call<T>(method:string,params:unknown={},metadata?:DiagnosticMetadata) {
      const updatesInvalid=validateUpdates('x-methods',method,params);if(updatesInvalid)throw Object.assign(new Error(updatesInvalid),{code:'INVALID_PARAMS'});
      const settingsInvalid=validateSettings('x-methods',method,params);if(settingsInvalid)throw Object.assign(new Error(settingsInvalid),{code:'INVALID_PARAMS'});
      const observationInvalid=validateObservation('x-methods',method,params);if(observationInvalid)throw Object.assign(new Error(observationInvalid),{code:'INVALID_PARAMS'});
      const error=validateNetwork('x-methods',method,params)??validateAccessPath('x-methods',method,params) ?? validateState('x-methods',method,params) ?? validateDirectory('x-methods',method,params) ?? validatePty('x-methods',method,params)??validateRemoteRead('x-methods',method,params)??validateWorkbenchRequest(method,params);
      if(error)throw Object.assign(new Error(error),{code:'INVALID_PARAMS'});
      const result=await transport.call<T>(method,params,metadata);
      const updatesResult=validateUpdates('x-results',method,result);if(updatesResult)throw Object.assign(new Error(updatesResult),{code:'PROTOCOL_ERROR'});
      const settingsInvalidResult=validateSettings('x-results',method,result);if(settingsInvalidResult)throw Object.assign(new Error(settingsInvalidResult),{code:'PROTOCOL_ERROR'});
      const observationResult=validateObservation('x-results',method,result);if(observationResult)throw Object.assign(new Error(observationResult),{code:'PROTOCOL_ERROR'});
      const invalid=validateNetwork('x-results',method,result)??validateAccessPath('x-results',method,result) ?? validateState('x-results',method,result) ?? validateDirectory('x-results',method,result) ?? validatePty('x-results',method,result)??validateRemoteRead('x-results',method,result)??validateWorkbenchResult(method,result);
      if(invalid)throw Object.assign(new Error(invalid),{code:'PROTOCOL_ERROR'});
      return result;
    },
    onNotification(listener){return transport.onNotification(event=>{const error=validateState('x-events',event.type,event.payload)??validatePty('x-events',event.type,event.payload);if(!error)listener(event);});},
  };
}
