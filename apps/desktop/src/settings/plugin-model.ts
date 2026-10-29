import type {EnablementChange,EnablementSnapshot,ManagedPlugin} from '@sodeck/sdk/plugin-management';
export type PluginDraft={rows:ManagedPlugin[];snapshot?:EnablementSnapshot;edits:Record<string,boolean>;conflict:boolean};
export const emptyPlugins=():PluginDraft=>({rows:[],edits:{},conflict:false});
export function pluginChanges(model:PluginDraft):EnablementChange[]{
  return Object.entries(model.edits).filter(([id,value])=>model.rows.find(r=>r.pluginId===id)?.enabled!==value).map(([pluginId,enabled])=>({pluginId,enabled}));
}
export function editPlugin(model:PluginDraft,id:string,value:boolean):PluginDraft{
  const edits={...model.edits};if(model.rows.find(r=>r.pluginId===id)?.enabled===value)delete edits[id];else edits[id]=value;
  return {...model,edits};
}
export function installPlugins(model:PluginDraft,rows:ManagedPlugin[],snapshot:EnablementSnapshot,discard=false):PluginDraft{
  if(discard)return {rows,snapshot,edits:{},conflict:false};
  const changed=model.snapshot&&(model.snapshot.generation!==snapshot.generation||model.snapshot.revision!==snapshot.revision);
  return {rows,snapshot,edits:model.edits,conflict:model.conflict||!!changed&&Object.keys(model.edits).length>0};
}
export function acceptPlugins(model:PluginDraft,changes:EnablementChange[]):PluginDraft{
  const values=new Map(changes.map(c=>[c.pluginId,c.enabled]));
  return {...model,rows:model.rows.map(r=>values.has(r.pluginId)?{...r,enabled:values.get(r.pluginId)!}:r),edits:{},conflict:false};
}
