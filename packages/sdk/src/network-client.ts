import type {Scope,Handle,Receipt,Attachment,Cursor} from './network';
import {validateNetwork} from './network';
import {NetworkStreams,type ByteSession} from './network-stream';

export interface NetworkRpc {call<T>(method:string,params:unknown):Promise<T>}
export interface NetworkOperation {id:string;canceled:boolean}
let sequence=0;
export function networkOperation():NetworkOperation{return {id:`net-${Date.now().toString(36)}-${(++sequence).toString(36)}-${Math.random().toString(36).slice(2)}`,canceled:false};}
const pause=(ms:number)=>new Promise<void>(resolve=>setTimeout(resolve,ms));
export class NetworkClient {
  constructor(readonly rpc:NetworkRpc,readonly scope?:Scope,readonly streams?:NetworkStreams){}
  async execute<T>(method:string,params:Record<string,unknown>,operation=networkOperation(),rpc:NetworkRpc=this.rpc):Promise<T> {
    const request={...(this.scope?{scope:this.scope}:{}),...params,operationId:operation.id};
    const bad=validateNetwork('x-methods',method,request);if(bad)throw Object.assign(Error(bad),{code:'INVALID_PARAMS'});
    let receipt:Receipt;
    try {receipt=await rpc.call<Receipt>(method,request);}
    catch(error) {
      if(!['RESULT_UNKNOWN','TIMEOUT'].includes(String((error as {code?:string}).code)))throw error;
      receipt=await rpc.call<Receipt>('net.operations.get',{scope:this.scope,operationId:operation.id});
    }
    while(receipt.state==='pending') {
      if(operation.canceled){await rpc.call('net.operations.cancel',{scope:this.scope,operationId:operation.id});throw Object.assign(Error('操作已取消'),{code:'CANCELED'});}
      await pause(20);
      receipt=await rpc.call<Receipt>('net.operations.get',{scope:this.scope,operationId:operation.id});
    }
    const invalid=validateNetwork('x-results',method,receipt);if(invalid)throw Object.assign(Error(invalid),{code:'PROTOCOL_ERROR'});
    if(receipt.state!=='succeeded')throw Object.assign(Error(receipt.error?.message??'网络操作未完成'),receipt.error,{operationId:operation.id});
    if(operation.canceled){await rpc.call('net.operations.cancel',{scope:this.scope,operationId:operation.id});throw Object.assign(Error('操作已取消'),{code:'CANCELED'});}
    await rpc.call('net.operations.claim',{scope:this.scope,operationId:operation.id});
    return receipt.result as T;
  }
  async exchange(group:Handle,stream:Handle,rpc:NetworkRpc=this.rpc):Promise<ByteSession> {
    if(!this.streams)throw Object.assign(Error('字节流只能在插件业务进程使用'),{code:'UNSUPPORTED_API'});
    const {attachment}=await this.execute<{attachment:Attachment}>('net.streams.exchange',{group,stream},networkOperation(),rpc);
    return this.streams.attach(stream,attachment);
  }
  async splice(group:Handle,left:Handle|ByteSession,right:Handle|ByteSession,rpc:NetworkRpc=this.rpc):Promise<void> {
    const cursor=(value:Handle|ByteSession):Cursor=>'freeze' in value?value.freeze():{stream:value,epoch:0,consumed:0,written:0};
    try {await this.execute('net.streams.splice',{group,left:cursor(left),right:cursor(right)},networkOperation(),rpc);}
    catch(error) {
      // Freeze failures cannot leave a JS reader and native owner racing.
      await Promise.allSettled([left,right].map(v=>this.execute('net.streams.close',{group,stream:'stream' in v?v.stream:v},networkOperation(),rpc)));
      throw error;
    } finally {if('dispose' in left)left.dispose();if('dispose' in right)right.dispose();}
  }
}
