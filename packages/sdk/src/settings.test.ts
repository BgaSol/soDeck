import {describe,it,expect} from 'vitest';
import {validateSettingsDescriptor,validateSettings,validateSettingChanges,type SettingsDescriptor} from './settings';
import {createSettingsProvider} from './settings-provider';
const descriptor:SettingsDescriptor={descriptorVersion:1,pluginId:'sample',pages:[{id:'general',title:'Settings',order:0,schemaVersion:1,sections:[{id:'display',title:'Display',fields:[{key:'size',label:'Size',type:'integer',default:14,minimum:10,maximum:24,effect:'immediate'},{key:'enabled',label:'Enabled',type:'boolean',default:true,effect:'newSession'}]}]}]};
describe('settings boundaries',()=>{
  it('rejects duplicate groups, mistyped defaults, cyclic and foreign conditions',()=>{
    expect(validateSettingsDescriptor(descriptor,'sample')).toEqual(descriptor);
    const invalid=structuredClone(descriptor);invalid.pages[0].sections[0].fields[0].default='14';expect(()=>validateSettingsDescriptor(invalid,'sample')).toThrow();
    invalid.pages[0].sections[0].fields[0].default=14;invalid.pages[0].sections[0].fields[0].visibleWhen={key:'enabled',equals:true};invalid.pages[0].sections[0].fields[1].enabledWhen={key:'size',equals:14};expect(()=>validateSettingsDescriptor(invalid,'sample')).toThrow(/循环/);
    const overlap=structuredClone(descriptor);overlap.pages.push({...overlap.pages[0],id:'another'});expect(()=>validateSettingsDescriptor(overlap,'sample')).toThrow();
    expect(()=>validateSettingsDescriptor(descriptor,'foreign')).toThrow();
  });
  it('validates all four controls, unique changes and strict wire fields',()=>{
    const page=structuredClone(descriptor.pages[0]);page.sections[0].fields.push({key:'name',label:'Name',type:'text',default:'',maxLength:3,effect:'restart'},{key:'mode',label:'Mode',type:'enum',default:'a',options:[{value:'a',label:'A'}],effect:'immediate'});
    expect(validateSettingChanges(page,[{key:'size',value:15},{key:'name',value:'中文名'},{key:'mode',value:'a'},{key:'enabled',value:false}])).toEqual([]);
    expect(validateSettingChanges(page,[{key:'size',value:14},{key:'size',value:15},{key:'name',value:'long'},{key:'mode',value:'b'}])).toHaveLength(3);
    expect(validateSettings('x-methods','settings.page.get',{pageId:'core:appearance',extra:1})).toBeDefined();
  });
  it('does not acknowledge synchronization when runtime application fails',async()=>{
    const calls:string[]=[];const snapshot={pageId:'plugin:sample:general',providerEpoch:'epoch',storageGeneration:'generation',schemaVersion:1,settingsRevision:'1',values:[{key:'size',value:16},{key:'enabled',value:true}],syncState:'current',warnings:[]};
    const provider=createSettingsProvider({pluginId:'sample',descriptor,bridge:{async call<T>(method:string){calls.push(method);return snapshot as T;}},apply:()=>{throw Error('application failed');}});
    await expect(provider.invoke('settings.provider.commit',{groupId:'general',ticket:'ticket',changes:[{key:'size',value:16}]})).rejects.toThrow('application failed');
    expect(calls).toEqual(['storage.preferences.commit','storage.preferences.read']);
  });
});
