import {observationTransport} from '@sodeck/sdk';
import {createObservation,observedAction} from '@sodeck/sdk/observation';
import {type SettingsSnapshot} from '@sodeck/sdk/settings';
import {defaultFontSize} from './settings';
const observation=createObservation(observationTransport);
export function changeSetting(command:string,args:Record<string,unknown>,snapshot:SettingsSnapshot):Promise<unknown>{
  const key=command==='settings.shellIntegration'?'shellIntegration':'fontSize';
  const value=key==='shellIntegration'?args.enabled:command==='settings.reset'?defaultFontSize:args.fontSize;
  return observedAction(observation,'plugin.sodeck.terminal.settings.change',{},client=>client.call('settings.commit',{pageId:snapshot.pageId,providerEpoch:snapshot.providerEpoch,schemaVersion:snapshot.schemaVersion,expectedRevision:snapshot.settingsRevision,operationId:crypto.randomUUID(),changes:[{key,value}]}));
}
