import {useCallback,useEffect,useState} from 'react';
import {core,CoreError,errorMessage,observeState,onCoreNotification,type Group,type GroupsPage,type CatalogPage,type CatalogItem,type CatalogSave,type OperationResult} from '@sodeck/sdk';
import type {Server} from './types';
export const catalogServer=(item:CatalogItem):Server=>({...item.server,connectionRoute:item.connectionRoute,groupId:item.groupId,membershipRevision:item.membershipRevision});
export async function loadCatalog():Promise<{servers:Server[]}> {
  for(let attempt=0;;attempt++) {
    try {const servers:Server[]=[];let cursor:string|null=null;
      do {const page:CatalogPage=await core.call('servers.catalog.list',{cursor});servers.push(...page.items.map(catalogServer));cursor=page.nextCursor;}while(cursor);
      return {servers};
    }catch(e){if(!(e instanceof CoreError&&e.code==='STALE_CURSOR'&&attempt<2))throw e;}
  }
}
export function useServerGroups(enabled:boolean) {
  const [groups,setGroups]=useState<Group[]>([]),[revision,setRevision]=useState('0'),[error,setError]=useState('');
  const load=useCallback(async()=>{
    const items:Group[]=[];let cursor:string|null=null,last='0';
    do {const page:GroupsPage=await core.call('serverGroups.list',{cursor});items.push(...page.groups);last=page.catalogRevision;cursor=page.nextCursor;}while(cursor);
    return {items,last};
  },[]);
  const refresh=useCallback(async()=>{if(!enabled)return;try{const result=await load();setGroups(result.items);setRevision(result.last);setError('');}catch(e){setError(errorMessage(e));}},[enabled,load]);
  useEffect(()=>{if(!enabled)return;let alive=true;let sequence=0;
    const reload=()=>{const current=++sequence;void load().then(r=>{if(alive&&current===sequence){setGroups(r.items);setRevision(r.last);setError('');}},e=>{if(alive&&current===sequence)setError(errorMessage(e));});};
    reload();
    const subscription=observeState({call:core.call,onNotification:onCoreNotification},{contract:'servers.catalog.v1',target:{app:'current'}},value=>{
      if(!value||typeof value!=='object'||!('catalogRevision' in value)||typeof value.catalogRevision!=='string')throw new Error('目录状态无效');return value;
    },reload,{onError:e=>{if(alive)setError(errorMessage(e));}});
    return()=>{alive=false;void subscription.then(s=>s.dispose()).catch(()=>{});};
  },[enabled,load]);
  return {groups,revision,error,refresh};
}
export async function saveCatalog(request:CatalogSave):Promise<CatalogItem> {
  try {return await core.call('servers.catalog.save',request);}
  catch(error) {
    if(error instanceof CoreError&&['RESULT_UNKNOWN','TIMEOUT'].includes(error.code)) {
      try {const result=await core.call<OperationResult>('servers.catalog.operation',{operationId:request.operationId});
        if(result.status==='committed'&&!result.result.postCommitPending)return result.result;
      }catch{/* Keep the original operation identity while reconciliation is unavailable. */}
      throw new CoreError({code:'RESULT_UNKNOWN',message:'保存回执尚未确认，请重试核对。'});
    }
    throw error;
  }
}
