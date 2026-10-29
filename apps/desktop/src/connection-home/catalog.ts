import {useCallback,useEffect,useRef,useState} from 'react';
import {core,CoreError,errorMessage,observeState,onCoreNotification,type CatalogPage,type GroupsPage,type ProxyPage} from '@sodeck/sdk';
import {emptyCatalog,type Catalog} from './model';
export async function readCatalog():Promise<Catalog>{
  for(let attempt=0;attempt<3;attempt++){
    try{
      const result:Catalog={items:[],proxies:[],groups:[],revision:''};let bytes=0;
      const accept=(page:{catalogRevision:string})=>{if(result.revision&&result.revision!==page.catalogRevision)throw new CoreError({code:'STALE_CURSOR',message:'目录正在更新'});result.revision=page.catalogRevision;bytes+=JSON.stringify(page).length*2;if(bytes>32*1024*1024)throw Error('连接目录超过 32 MiB 容量');};
      let cursor:string|null=null;
      do{const page:CatalogPage=await core.call('servers.catalog.list',{limit:100,...(cursor?{cursor}:{})});accept(page);result.items.push(...page.items);cursor=page.nextCursor;}while(cursor);
      do{const page:GroupsPage=await core.call('serverGroups.list',{limit:100,...(cursor?{cursor}:{})});accept(page);result.groups.push(...page.groups);cursor=page.nextCursor;}while(cursor);
      do{const page:ProxyPage=await core.call('proxies.catalog.list',{limit:100,...(cursor?{cursor}:{})});accept(page);result.proxies.push(...page.items);cursor=page.nextCursor;}while(cursor);
      accept(await core.call<CatalogPage>('servers.catalog.list',{limit:1}));return result;
    }catch(e){if(!(e instanceof CoreError&&e.code==='STALE_CURSOR'&&attempt<2))throw e;}
  }
  throw Error('目录正在持续变化，请重试同步');
}
export function useConnectionCatalog(){
  const [catalog,setCatalog]=useState(emptyCatalog),[error,setError]=useState(''),[ready,setReady]=useState(false);
  const active=useRef(true),loading=useRef<Promise<void>|null>(null),again=useRef(false);
  const refresh=useCallback(()=>{
    if(loading.current){again.current=true;return loading.current;}
    loading.current=(async()=>{do{again.current=false;try{const next=await readCatalog();if(active.current){setCatalog(next);setReady(true);setError('');}}catch(e){if(active.current)setError(errorMessage(e));}}while(again.current&&active.current);})().finally(()=>{loading.current=null;});return loading.current;
  },[]);
  useEffect(()=>{active.current=true;const abort=new AbortController();void refresh();
    void observeState({call:core.call,onNotification:onCoreNotification},{contract:'servers.catalog.v2',target:{app:'current'}},v=>v,()=>void refresh(),{signal:abort.signal,onError:e=>{if(active.current)setError(errorMessage(e));}}).catch(e=>{if(active.current)setError(errorMessage(e));});
    return()=>{active.current=false;abort.abort();};
  },[refresh]);
  return {catalog,error,ready,refresh};
}
