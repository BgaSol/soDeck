import source from '../settings/settings.json';
import {validateSettingsDescriptor,settingFields,type SettingsSnapshot} from '@sodeck/sdk/settings';
export const settingsDescriptor=validateSettingsDescriptor(source,'sodeck.terminal');
export const settingsPageId='plugin:sodeck.terminal:general';
const fields=settingFields(settingsDescriptor.pages[0]);
export const defaultFontSize=fields.find(f=>f.key==='fontSize')!.default as number;
export const defaultShellIntegration=fields.find(f=>f.key==='shellIntegration')?.default!==false;
export function terminalPreferences(snapshot:SettingsSnapshot){
  const values=new Map(snapshot.values.map(e=>[e.key,e.value]));
  return {fontSize:values.get('fontSize') as number,shellIntegration:(values.get('shellIntegration')??defaultShellIntegration)===true};
}
