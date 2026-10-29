import {validateSettingsDescriptor,validateSettingChanges,settingFields,type SettingsTransport,type SettingsSnapshot,type SettingEntries,type SettingErrors} from './settings';

export interface SettingsProviderOptions {
  pluginId:string;descriptor:unknown;bridge:SettingsTransport;
  apply:(groupId:string,snapshot:SettingsSnapshot)=>void|Promise<void>;
  validate?:(groupId:string,values:SettingEntries)=>SettingErrors|Promise<SettingErrors>;
}
/** Host-only callbacks. Storage accepts only the ticket issued for this invocation. */
export function createSettingsProvider(options:SettingsProviderOptions){
  const descriptor=validateSettingsDescriptor(options.descriptor,options.pluginId);
  const pages=new Map(descriptor.pages.map(p=>[p.id,p]));
  const read=(groupId:string)=>options.bridge.call<SettingsSnapshot>('storage.preferences.read',{groupId});
  return {
    descriptor,
    async initialize(){for(const page of descriptor.pages)await options.apply(page.id,await read(page.id));},
    async invoke(command:string,input:unknown):Promise<unknown>{
      if(!command.startsWith('settings.provider.'))throw Error('未知设置回调');
      const p=input as {groupId:string;changes?:SettingEntries;ticket?:string;descriptor?:unknown};
      const page=pages.get(p.groupId);if(!page)throw Error('设置组不存在');
      if(p.descriptor&&canonical(p.descriptor)!==canonical(descriptor))throw Error('设置描述与已签名包不一致');
      if(command==='settings.provider.read'){const snapshot=await read(p.groupId);await options.apply(p.groupId,snapshot);return snapshot;}
      if(command==='settings.provider.validate'){
        const changes=p.changes??[],errors=validateSettingChanges(page,changes);if(errors.length)return {errors};
        const current=await read(p.groupId),merged=new Map(current.values.map(e=>[e.key,e.value]));changes.forEach(e=>merged.set(e.key,e.value));
        return {errors:await options.validate?.(p.groupId,settingFields(page).map(f=>({key:f.key,value:merged.get(f.key)??f.default})))??[]};
      }
      if(!p.ticket)throw Error('缺少设置操作凭据');
      if(command==='settings.provider.commit')await options.bridge.call('storage.preferences.commit',{commitTicket:p.ticket,changes:p.changes??[]});
      else if(command!=='settings.provider.sync')throw Error('未知设置回调');
      const snapshot=await read(p.groupId);
      await options.apply(p.groupId,snapshot);
      await options.bridge.call('settings.provider.applied',{ticket:p.ticket,settingsRevision:snapshot.settingsRevision});
      return snapshot;
    },
  };
}

function canonical(value:unknown):string {return JSON.stringify(value,(_key,item)=>item&&typeof item==='object'&&!Array.isArray(item)?Object.fromEntries(Object.keys(item).sort().filter(key=>key!=='keywords'||item[key].length).map(key=>[key,item[key]])):item);}
