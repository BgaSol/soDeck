import {expect,it} from 'vitest';
import {acceptPlugins,editPlugin,emptyPlugins,installPlugins,pluginChanges} from './plugin-model';
import {saveParticipants,type SettingsSaveParticipant} from './save-coordinator';
import type {EnablementSnapshot,ManagedPlugin} from '@sodeck/sdk/plugin-management';
const snapshot:EnablementSnapshot={storageStatus:'ready',generation:'generation',revision:'0',reason:null,diagnosticToken:null};
const row=(pluginId:string):ManagedPlugin=>({pluginId,displayName:pluginId,enabled:true,bootEnabled:true,selected:null,pending:null,error:null,restartRequired:false,runtimeStatus:'running'});
it('retains per-ID drafts across imports and detects saved-revision conflicts',()=>{
  let model=installPlugins(emptyPlugins(),[row('a')],snapshot);model=editPlugin(model,'a',false);
  model=installPlugins(model,[row('a'),row('b')],snapshot);expect(pluginChanges(model)).toEqual([{pluginId:'a',enabled:false}]);expect(model.conflict).toBe(false);
  model=installPlugins(model,[row('a'),row('b')],{...snapshot,revision:'1'});expect(model.conflict).toBe(true);expect(model.edits.a).toBe(false);
  model=acceptPlugins(model,pluginChanges(model));expect(pluginChanges(model)).toEqual([]);expect(model.rows[0].bootEnabled).toBe(true);expect(model.rows[0].enabled).toBe(false);
});
it('validates every group before writes and retains partial success',async()=>{
  const calls:string[]=[];const p=(id:string,fail:string):SettingsSaveParticipant=>({id,dirty:true,unresolvedOperations:[],discard:async()=>{},reconcile:async()=>true,validate:async()=>{calls.push('validate:'+id);if(fail==='validate')throw Error('invalid');},commit:async()=>{calls.push('commit:'+id);if(fail==='commit')throw Error('write');}});
  await expect(saveParticipants([p('appearance',''),p('plugins','validate')],()=>{})).rejects.toThrow('invalid');expect(calls).toEqual(['validate:appearance','validate:plugins']);calls.length=0;
  let saved=0;await expect(saveParticipants([p('appearance',''),p('plugins','commit')],()=>saved++)).rejects.toThrow('write');expect(saved).toBe(1);
});
