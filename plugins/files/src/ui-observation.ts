import {core,observationTransport} from '@sodeck/sdk';
import {createObservation,observedAction} from '@sodeck/sdk/observation';
const observation=createObservation(observationTransport);
const operations:Record<string,string>={'files.refresh':'refresh','files.locate':'locate','files.more':'readMore'};
export function invoke<T>(command:string,args:Record<string,unknown>={}):Promise<T>{
  const operation=operations[command];if(!operation)return core.call<T>('plugin.invoke',{command,args});
  return observedAction(observation,'plugin.sodeck.files.directory.operation',{operation},client=>client.call<T>('plugin.invoke',{command,args}));
}
