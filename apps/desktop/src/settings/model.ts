import {settingFields,acceptsSetting,type SettingsPageResult,type SettingsSnapshot,type SettingEntries,type SettingsCommitResult} from '@sodeck/sdk/settings';
export type DraftValue=string|boolean|number;
export interface PageDraft {page:SettingsPageResult;draft:Record<string,DraftValue>;errors:Record<string,string>;conflict:boolean;operation?:{id:string;generation:string;changes:SettingEntries};status:'clean'|'dirty'|'validating'|'saving'|'reconciling'|'unavailable'}
export const values=(snapshot:SettingsSnapshot)=>Object.fromEntries(snapshot.values.map(e=>[e.key,e.value]));
export function draftFor(page:SettingsPageResult):PageDraft {return {page,draft:values(page.snapshot),errors:{},conflict:false,status:'clean'};}
export function changesFor(record:PageDraft):{changes:SettingEntries;errors:Record<string,string>}{
  const baseline=values(record.page.snapshot),changes:SettingEntries=[],errors:Record<string,string>={};
  for(const f of settingFields(record.page.definition)){
    const raw=record.draft[f.key],value=f.type==='integer'&&typeof raw==='string'&&/^-?\d+$/.test(raw)?Number(raw):raw;
    if(!acceptsSetting(f,value)){errors[f.key]=f.type==='integer'?`请输入 ${f.minimum??''} 至 ${f.maximum??''} 的整数`:'请输入有效值';continue;}
    if(value!==baseline[f.key])changes.push({key:f.key,value});
  }return {changes,errors};
}
export function dirty(record:PageDraft){const c=changesFor(record);return c.changes.length>0||Object.keys(c.errors).length>0;}
export function installSnapshot(record:PageDraft,next:SettingsSnapshot):PageDraft {
  const old=record.page.snapshot;if(next.pageId!==old.pageId||next.providerEpoch===old.providerEpoch&&BigInt(next.settingsRevision)<BigInt(old.settingsRevision))return record;
  if(next.syncState==='unavailable')return {...record,status:'unavailable',page:{...record.page,snapshot:next}};
  if(next.providerEpoch===old.providerEpoch&&next.settingsRevision===old.settingsRevision)return {...record,status:dirty(record)?'dirty':'clean',page:{...record.page,snapshot:next}};
  if(dirty(record))return {...record,conflict:true};
  return draftFor({...record.page,snapshot:next});
}
// A durable receipt settles the operation even when a subsequent provider read fails.
// Keep the page read-only until that read confirms any provider normalization.
export function acceptSaved(record:PageDraft,result:SettingsCommitResult):PageDraft {
  const merged={...values(record.page.snapshot),...Object.fromEntries(record.operation!.changes.map(e=>[e.key,e.value]))};
  return {...draftFor({...record.page,snapshot:{...record.page.snapshot,settingsRevision:result.persistedRevision,values:Object.entries(merged).map(([key,value])=>({key,value})),syncState:'pending'}}),status:'unavailable'};
}
export function matchesSearch(text:string,query:string){const fold=(s:string)=>s.normalize('NFKC').toLocaleLowerCase();const haystack=fold(text);return fold(query).trim().split(/\s+/u).every(word=>haystack.includes(word));}
