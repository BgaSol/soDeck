import {useEffect,useRef,useState,useSyncExternalStore} from 'react';
import {core,errorMessage,onCoreNotification,type HomeIntent,type HomeAttached} from '@sodeck/sdk';
import {Drafts} from './drafts';
import {Saves} from './save';
import {busy,type Catalog,type Draft} from './model';
export function useConnectionHome(catalog:Catalog,ready:boolean,refresh:()=>Promise<void>,selected:boolean,navigationRevision:string,onOpen:(id:string,operation:string)=>Promise<void>,enabled=true){
  const [store]=useState(()=>new Drafts());const revision=useSyncExternalStore(store.subscribe,store.snapshot);
  const values=useRef({catalog,refresh,selected,navigationRevision,onOpen});values.current={catalog,refresh,selected,navigationRevision,onOpen};
  const [saves]=useState(()=>new Saves(store,()=>values.current.catalog,()=>values.current.refresh(),(id,operation)=>values.current.onOpen(id,operation)));
  const [error,setError]=useState(''),[discard,setDiscard]=useState<string>(),[closing,setClosing]=useState<{ticket:string;since:number;waiting:boolean}>(),[elapsed,setElapsed]=useState(0);
  const session=useRef(''),closeTicket=useRef<string|undefined>(undefined),renderer=useRef(crypto.randomUUID()),initialized=useRef(false),queued=useRef<HomeIntent|undefined>(undefined),intentSequence=useRef(0),lastNavigation=useRef(0n);
  const report=async(ticket?:string)=>{if(!session.current)return;await core.call('ui.connectionHome.session.report',{pageSession:session.current,draftRevision:String(store.revision),dirtyCount:store.dirtyCount(),operations:saves.operations(),...(ticket?{closeTicket:ticket}:{})});};
  const act=(fn:()=>void)=>{try{setError('');fn();}catch(e){setError(errorMessage(e));}};
  const navigate=(intent:HomeIntent)=>act(()=>{
    if(!ready){queued.current=intent;return;}intentSequence.current++;
    if(intent.kind==='newSsh')store.create('ssh',values.current.catalog);
    if(intent.kind==='newProxy')store.create('proxy',values.current.catalog);
    if(intent.kind==='editServer'){const server=values.current.catalog.items.find(i=>i.server.id===intent.serverId);if(!server)throw Error('服务器已删除或目录尚未同步');store.create('ssh',values.current.catalog,{server});}
    if(intent.kind==='manageProxies')store.select({kind:'proxies'});
    if(intent.kind==='editProxy'){const proxy=values.current.catalog.proxies.find(p=>p.proxy.id===intent.proxyId)?.proxy;if(!proxy)throw Error('代理已删除或目录尚未同步');store.create('proxy',values.current.catalog,{proxy});}
    if(intent.kind==='manageGroups')store.select({kind:'groups'});
    if(intent.kind==='home'&&store.mode.kind==='guide'&&!values.current.catalog.items.length)store.create('ssh',values.current.catalog);
  });
  const navigation=useRef(navigate);navigation.current=navigate;
  const closeReply=async(decision:'discard'|'cancel'|'abandonWait',ticket=closing?.ticket)=>{
    if(!ticket)return;try{await report(ticket);await core.call('ui.connectionHome.close.respond',{pageSession:session.current,ticket,draftRevision:String(store.revision),decision});setClosing(old=>old?.ticket===ticket?{...old,waiting:true}:old);}catch(e){setError(errorMessage(e));}
  };
  const reply=useRef(closeReply);reply.current=closeReply;
  useEffect(()=>{
    if(!enabled)return;
    let alive=true;void core.call<HomeAttached>('ui.connectionHome.session.attach',{rendererId:renderer.current}).then(result=>{if(alive){session.current=result.pageSession;if(result.replacedDirty)setError('页面已重新载入，未保存的内存草稿无法恢复。已保存配置仍在列表中。');void report();}}).catch(e=>setError(errorMessage(e)));
    const off=onCoreNotification(e=>{
      if(e.type==='connectionHome.navigate'){const p=e.payload as {intent:HomeIntent;navigationRevision:string};if(BigInt(p.navigationRevision)<lastNavigation.current)return;lastNavigation.current=BigInt(p.navigationRevision);navigation.current(p.intent);}
      if(e.type==='connectionHome.closeRequested'){const p=e.payload as {pageSession:string;ticket:string};if(p.pageSession!==session.current)return;closeTicket.current=p.ticket;store.frozen=true;store.changed();setElapsed(0);setClosing({ticket:p.ticket,since:Date.now(),waiting:false});void report(p.ticket).then(()=>{if(closeTicket.current===p.ticket&&!store.dirtyCount()&&!saves.operations().length)void reply.current('discard',p.ticket);}).catch(e=>setError(errorMessage(e)));}
      if(e.type==='connectionHome.closeFinished'){const p=e.payload as {ticket:string;committed:boolean};if(closeTicket.current!==p.ticket)return;closeTicket.current=undefined;setClosing(undefined);store.frozen=false;if(p.committed){store.clear();saves.pending.clear();initialized.current=false;}store.changed();}
    });return()=>{alive=false;off();};
  },[store,saves,enabled]);
  useEffect(()=>{void report().catch(e=>setError(errorMessage(e)));},[revision]);
  useEffect(()=>{if(!ready)return;if(queued.current){const intent=queued.current;queued.current=undefined;navigation.current(intent);initialized.current=true;}else if(selected&&!initialized.current){initialized.current=true;navigation.current({kind:'home'});}},[ready,selected]);
  useEffect(()=>{if(!closing)return;const timer=setInterval(()=>{setElapsed(Date.now()-closing.since);for(const d of store.items.values())if(d.phase==='reconciling')void saves.save(d.id);for(const g of [store.groupEdit,...[...store.items.values()].map(d=>d.groupEdit)])if(g?.unknown)void saves.saveGroup(g);},1000);return()=>clearInterval(timer);},[closing?.ticket,store,saves]);
  const submit=(d:Draft,connect=false)=>{const sequence=intentSequence.current,nav=values.current.navigationRevision,local=store.navigation;void saves.save(d.id,connect,()=>!store.frozen&&sequence===intentSequence.current&&local===store.navigation&&values.current.selected&&values.current.navigationRevision===nav&&store.mode.kind==='draft'&&store.mode.id===d.id);};
  return {store,saves,error,setError,discard,closing,elapsed,session,act,submit,closeReply,
    cancel:(d:Draft)=>{if(busy(d))return;if(store.dirty(d))setDiscard(d.id);else store.remove(d.id);},
    discardNow:()=>{if(discard){store.remove(discard);saves.forget(discard);setDiscard(undefined);}},keep:()=>setDiscard(undefined),
    navigate,
    pick:async(d:Draft)=>{try{const result=await core.call<{status:'selected';path:string}|{status:'canceled'}>('ui.connectionHome.pickPrivateKey',{pageSession:session.current,draftId:d.id,requestId:crypto.randomUUID()});if(result.status==='selected'&&store.items.has(d.id))store.secret(d.id,{privateKeyPath:result.path});}catch(e){setError(errorMessage(e));}},
  };
}
export type Controller=ReturnType<typeof useConnectionHome>;
