import {core,observationTransport} from '@sodeck/sdk';
import {createObservation,observedAction} from '@sodeck/sdk/observation';
const observation=createObservation(observationTransport);
const types:Record<string,string>={'network.check':'check','network.stop':'run.stop','network.delete':'rule.edit','network.save':'rule.edit','network.editor.save':'rule.edit'};
export function invoke<T>(command:string,args:Record<string,unknown>={}):Promise<T>{
  const type=types[command];if(!type)return core.call<T>('plugin.invoke',{command,args});
  return observedAction(observation,`plugin.sodeck.network-proxy.${type}`,{},client=>client.call<T>('plugin.invoke',{command,args}));
}
