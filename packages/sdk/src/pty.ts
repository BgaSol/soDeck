import {schema as wire} from './pty-observe.generated';
import {validate,type Shape} from './schema';
import type {Open,Opened,StartReceipt} from './pty-observe.generated';
export type * from './pty-observe.generated';
const schema=wire as unknown as {$defs:Record<string,Shape>;'x-methods':Record<string,string>;'x-results':Record<string,string>;'x-events':Record<string,string>};
export function validatePty(kind:'x-methods'|'x-results'|'x-events',name:string,value:unknown){
  const definition=schema[kind]?.[name];return definition?validate(schema,schema.$defs[definition],value,name):undefined;
}
/** Recover an ambiguous startup by querying its receipt; never replay exec. */
export async function openPtyCommand(bridge:{call<T>(method:string,p:unknown):Promise<T>},params:Open):Promise<Opened>{
  const call=async<T>(method:string,p:unknown)=>{
    const error=validatePty('x-methods',method,p);if(error)throw Object.assign(new Error(error),{code:'INVALID_PARAMS'});
    const result=await bridge.call<T>(method,p);const invalid=validatePty('x-results',method,result);
    if(invalid)throw Object.assign(new Error(invalid),{code:'PROTOCOL_ERROR'});return result;
  };
  try{return await call<Opened>('pty.openCommand',params);}catch(error){
    if((error as {code?:string}).code!=='RESULT_UNKNOWN')throw error;
    const deadline=Date.now()+30000;
    while(Date.now()<deadline){
      const receipt=await call<StartReceipt>('pty.start.get',{operationId:params.operationId});
      if(receipt.state==='completed'&&receipt.result)return receipt.result;
      if(receipt.state==='failed')throw receipt.error;
      await new Promise<void>(r=>setTimeout(r,50));
    }
    throw error;
  }
}
