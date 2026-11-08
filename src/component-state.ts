import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import type {Context} from './workspaces';
type Call = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
type Binding = {context:Context;post:(message:Record<string,unknown>)=>void;clients:Set<string>};
export class ComponentState {
  private bindings=new Map<string,Binding>();
  private unlisten?:UnlistenFn;
  private disposed=false;
  constructor(private call:Call){
    void listen<{surfaceId:string;data:{context:Context;clientSubscriptionId:string}}>('component-state-changed',event=>{
      const binding=this.bindings.get(event.payload.surfaceId),data=event.payload.data;
      if(binding && JSON.stringify(binding.context)===JSON.stringify(data.context) && binding.clients.has(data.clientSubscriptionId))binding.post({type:'stateChanged',data});
    }).then(fn=>{if(this.disposed)fn();else this.unlisten=fn;}).catch(()=>{});
  }
  attach(token:string,context:Context,post:Binding['post']){this.bindings.set(token,{context,post,clients:new Set()});}
  async request(token:string,input:Record<string,unknown>){
    const binding=this.bindings.get(token);if(!binding)throw new Error('共享状态挂载已失效');
    if(input.op==='subscribe' && typeof input.clientSubscriptionId==='string')binding.clients.add(input.clientSubscriptionId);
    const result=await this.call('component_ui',{operation:'state',args:{surfaceId:token,input:{...input,expectedContext:binding.context}}});
    if(this.disposed || this.bindings.get(token)!==binding)throw new Error('共享状态返回时挂载已改变');
    if(input.op==='unsubscribe' && typeof input.clientSubscriptionId==='string')binding.clients.delete(input.clientSubscriptionId);
    return result;
  }
  detach(token?:string){
    const binding=token?this.bindings.get(token):undefined;if(!token || !binding)return;
    this.bindings.delete(token);
    for(const clientSubscriptionId of binding.clients)void this.call('component_ui',{operation:'state',args:{surfaceId:token,input:{op:'unsubscribe',clientSubscriptionId,expectedContext:binding.context}}}).catch(()=>{});
  }
  dispose(){this.disposed=true;this.unlisten?.();for(const token of this.bindings.keys())this.detach(token);}
}
