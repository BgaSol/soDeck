import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react';
import {core,errorMessage,onCoreNotification,observeState} from '@sodeck/sdk';
import {settingsClient,validateSettings,type SettingsCatalogEntry,type SettingsContext,type SettingsSnapshot,type SettingsCloseIntent} from '@sodeck/sdk/settings';
import {BrandMark,Button,Input,Notice,Select} from '@sodeck/ui';
import {Dialog} from '../dialog';
import {WindowControls} from '../window-controls';
import {PluginSettings,usePluginManagement} from './plugins';
import {saveParticipants,type SettingsSaveParticipant} from './save-coordinator';
import {SettingsForm} from './form';
import {ApplicationSettings} from './application';
import {useUpdates} from './update-model';
import {UpdateSettings} from './updates';
import {acceptSaved,changesFor,dirty,draftFor,installSnapshot,matchesSearch,type PageDraft,type DraftValue} from './model';
import './settings.css';
declare global {interface Window {__SODECK_SETTINGS_CONTEXT__?:SettingsContext}}
const client=settingsClient(core),bridge={call:core.call,onNotification:onCoreNotification};
export function SettingsWindow(){
  const [context,setContext]=useState(window.__SODECK_SETTINGS_CONTEXT__),[catalog,setCatalog]=useState<SettingsCatalogEntry[]>([]),[selected,setSelected]=useState(context?.pageId??'core:appearance');
  const [records,setRecords]=useState<Record<string,PageDraft>>({}),[query,setQuery]=useState(''),[message,setMessage]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[closing,setClosing]=useState<SettingsCloseIntent>();
  const plugins=usePluginManagement();
  const updates=useUpdates(),updatesCurrent=useRef(updates);updatesCurrent.current=updates;
  const pluginCurrent=useRef(plugins);pluginCurrent.current=plugins;
  const closeFreeze=useRef<string|undefined>(undefined);
  const current=useRef(records),revision=useRef(0n),alive=useRef(true),inflight=useRef(false),header=useRef<HTMLElement>(null),requests=useRef(new Map<string,number>()),search=useRef<HTMLInputElement>(null);
  const update=useCallback((fn:(old:Record<string,PageDraft>)=>Record<string,PageDraft>)=>{current.current=fn(current.current);revision.current++;setRecords(current.current);},[]);
  const load=useCallback(async(id:string,discard=false)=>{
    if(id==='core:application'||id==='core:plugins'||id==='core:updates')return;const sequence=(requests.current.get(id)??0)+1;requests.current.set(id,sequence);
    try{const page=await client.page(id);if(alive.current&&requests.current.get(id)===sequence)update(old=>({...old,[id]:!discard&&old[id]?installSnapshot({...old[id],page:{...page,snapshot:old[id].page.snapshot}},page.snapshot):draftFor(page)}));}
    catch(e){if(alive.current){update(old=>old[id]?{...old,[id]:{...old[id],status:'unavailable'}}:old);setError(errorMessage(e));}}
  },[update]);
  const report=useCallback(async(closeTicket?:string)=>{if(!context)return;const rs=Object.values(current.current);await core.call('ui.settings.session.report',{generation:context.generation,...(closeTicket?{closeTicket}:{}),draftRevision:revision.current.toString(),dirtyPageIds:[...rs.filter(dirty).map(r=>r.page.snapshot.pageId),...(pluginCurrent.current.participant.dirty?["core:plugins"]:[]),...(updatesCurrent.current.participant.dirty?["core:updates"]:[])],unresolvedOperationIds:[...rs.flatMap(r=>r.operation?[r.operation.id]:[]),...pluginCurrent.current.participant.unresolvedOperations,...updatesCurrent.current.participant.unresolvedOperations]});},[context]);
  useEffect(()=>{revision.current++;void report().catch(e=>setError(errorMessage(e)));},[records,plugins.model,plugins.operation,plugins.importing,updates.draft,updates.saving,report]);
  const closeReply=async(intent:SettingsCloseIntent,decision:'discard'|'cancel'|'review')=>{try{await report(intent.ticket);await core.call('ui.settings.close.respond',{generation:intent.generation,ticket:intent.ticket,draftRevision:revision.current.toString(),decision});if(decision==='cancel'||decision==='review')closeFreeze.current=undefined;setClosing(old=>old?.ticket===intent.ticket?undefined:old);}catch(e){setError(errorMessage(e));}};
  const closeRef=useRef<(intent:SettingsCloseIntent)=>void>(()=>{});closeRef.current=intent=>{
    if(intent.generation!==context?.generation)return;closeFreeze.current=intent.ticket;
    if(!inflight.current&&!updatesCurrent.current.participant.dirty&&!updatesCurrent.current.participant.unresolvedOperations.length&&!pluginCurrent.current.working&&!pluginCurrent.current.participant.dirty&&!pluginCurrent.current.participant.unresolvedOperations.length&&!Object.values(current.current).some(r=>dirty(r)||r.operation))void closeReply(intent,'discard');else {void report(intent.ticket).catch(e=>setError(errorMessage(e)));setClosing(intent);}
  };
  useEffect(()=>{
    alive.current=true;const off=onCoreNotification(e=>{
      if(e.type==='settings.navigate'&&!validateSettings('x-events',e.type,e.payload)){const next=e.payload as SettingsContext;setContext(old=>{if(!old||old.generation===next.generation&&BigInt(next.navigationRevision)>=BigInt(old.navigationRevision)){setSelected(next.pageId);return next;}return old;});}
      if(e.type==='settings.closeFinished'&&!validateSettings('x-events',e.type,e.payload)){const p=e.payload as {ticket:string};if(closeFreeze.current===p.ticket){closeFreeze.current=undefined;setClosing(undefined);}}
      if(e.type==='settings.closeRequested'&&!validateSettings('x-events',e.type,e.payload))closeRef.current(e.payload as SettingsCloseIntent);
    });
    void core.call<SettingsContext>('ui.settings.context.get').then(setContext).catch(e=>setError(errorMessage(e)));
    return()=>{alive.current=false;off();};
  },[]);
  useEffect(()=>{
    const abort=new AbortController();let seq=0;
    const refresh=async()=>{const n=++seq;try{const items:SettingsCatalogEntry[]=[];let cursor:string|undefined;do{const page=await client.catalog({cursor,limit:100});items.push(...page.items);cursor=page.nextCursor??undefined;}while(cursor&&!abort.signal.aborted);if(!abort.signal.aborted&&n===seq)setCatalog(items);}catch(e){if(!abort.signal.aborted)setError(errorMessage(e));}};
    void refresh();void observeState(bridge,{contract:'settings.catalog.v1',target:{app:'current'}},v=>v,()=>void refresh(),{signal:abort.signal,onError:e=>setError(errorMessage(e))}).catch(e=>setError(errorMessage(e)));
    return()=>abort.abort();
  },[]);
  useEffect(()=>{
    setError('');void load(selected);if(selected==='core:application'||selected==='core:plugins'||selected==='core:updates')return;
    const abort=new AbortController();void observeState(bridge,{contract:'settings.page.v1',target:{pageId:selected}},v=>{const invalid=validateSettings('x-results','storage.preferences.read',v);if(invalid)throw Error(invalid);return v as SettingsSnapshot;},next=>update(old=>old[selected]?{...old,[selected]:installSnapshot(old[selected],next)}:old),{signal:abort.signal,onError:e=>{if(!abort.signal.aborted)setError(errorMessage(e));}}).catch(e=>{if(!abort.signal.aborted)setError(errorMessage(e));});
    return()=>abort.abort();
  },[selected,load,update]);
  const run=useCallback(async(method:string,p?:unknown)=>{try{return await core.call(method,p);}catch(e){setError(errorMessage(e));}},[]);
  useLayoutEffect(()=>{
    const rect=(el:Element|null)=>{const r=el?.getBoundingClientRect();return {x:r?.x??0,y:r?.y??0,width:r?.width??0,height:r?.height??0};};
    const report=()=>void run('windows.chrome',{titlebar:rect(header.current),drag:rect(document.querySelector('[data-chrome-drag]')),maximize:rect(document.querySelector('[data-chrome-maximize]')),tabs:[]});
    const observer=new ResizeObserver(report);if(header.current)observer.observe(header.current);window.addEventListener('resize',report);report();return()=>{observer.disconnect();window.removeEventListener('resize',report);};
  },[run]);
  const requestClose=()=>void run('ui.settings.close.request',{reason:'close'});
  useEffect(()=>{const key=(e:KeyboardEvent)=>{if(e.isComposing||closing)return;if(e.key==='Escape'){e.preventDefault();requestClose();}if(e.ctrlKey&&e.key.toLowerCase()==='f'){e.preventDefault();search.current?.focus();}};window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);});
  useEffect(()=>{if(!busy&&Object.values(records[selected]?.errors??{}).some(Boolean))document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();},[records,selected,busy]);
  const edit=(key:string,value:DraftValue)=>{if(inflight.current||closeFreeze.current)return;setMessage('');update(old=>{const record=old[selected];if(!record)return old;const next={...record,draft:{...record.draft,[key]:value},errors:{...record.errors,[key]:''}};next.status=dirty(next)?'dirty':'clean';return {...old,[selected]:next};});};
  const reconcile=async(id:string)=>{
    const op=current.current[id]?.operation;if(!op)return false;
    const result=await client.operation({pageId:id,storageGeneration:op.generation,operationId:op.id});
    if(result.state==='saved'&&result.result){update(old=>({...old,[id]:acceptSaved(old[id],result.result!)}));await load(id,true);return true;}
    if(result.state==='rejected'||result.state==='notStarted'){update(old=>({...old,[id]:{...old[id],operation:undefined,status:'dirty'}}));throw Error(result.reason??'设置未保存，草稿已保留');}
    return false;
  };
  const apply=async(close=false)=>{
    if(inflight.current||closeFreeze.current)return;inflight.current=true;setBusy(true);setError('');setMessage('');let saved=0;
    try{
      const groups:SettingsSaveParticipant[]=Object.entries(current.current).map(([id,r])=>{
        const data=changesFor(r),request={pageId:id,providerEpoch:r.page.snapshot.providerEpoch,schemaVersion:r.page.snapshot.schemaVersion,expectedRevision:r.page.snapshot.settingsRevision,changes:data.changes};
        return {id,dirty:dirty(r),unresolvedOperations:r.operation?[r.operation.id]:[],reconcile:()=>reconcile(id),discard:()=>load(id,true),
          async validate(){
            if(r.status==='unavailable')throw Error('设置暂不可用，请重新载入后应用');if(r.conflict)throw Error('请先处理设置冲突');
            update(old=>({...old,[id]:{...old[id],errors:data.errors}}));
            if(Object.keys(data.errors).length){setSelected(id);throw Error('请修正标出的设置项');}
            const validation=await client.validate(request);
            if(validation.errors.length){update(old=>({...old,[id]:{...old[id],errors:Object.fromEntries(validation.errors.map(e=>[e.key,e.message]))}}));setSelected(id);throw Error('插件校验未通过，所有草稿均已保留');}
          },
          async commit(){
            const operation={id:crypto.randomUUID(),generation:r.page.snapshot.storageGeneration,changes:request.changes};update(old=>({...old,[id]:{...old[id],status:'saving',operation}}));
            try{const result=await client.commit({...request,operationId:operation.id});update(old=>({...old,[id]:acceptSaved(old[id],result)}));await load(id,true);}
            catch(e){update(old=>({...old,[id]:{...old[id],status:'reconciling'}}));
              if(!['TIMEOUT','RESULT_UNKNOWN','TRANSPORT_ERROR'].includes((e as {code?:string}).code??'')){update(old=>({...old,[id]:{...old[id],status:'dirty',operation:undefined}}));throw e;}
              if(!await reconcile(id))throw Error('保存结果尚未确定，请点击“核对结果”；此组暂时不能再次应用');
            }
          },
        };
      });
      await saveParticipants([...groups,pluginCurrent.current.participant,updatesCurrent.current.participant],()=>saved++);
      setMessage(saved?Object.values(current.current).some(r=>r.page.snapshot.syncState!=='current')?'更改已保存，部分页面待重新同步':'更改已应用':'没有待应用的更改');if(close){inflight.current=false;setBusy(false);requestClose();}
    }catch(e){setError(`${saved?`已保存 ${saved} 组，其余草稿保留。`:''}${errorMessage(e)}`);}
    finally{inflight.current=false;setBusy(false);}
  };
  const shown=catalog.filter(entry=>matchesSearch([entry.title,entry.pluginName,entry.owner==='plugin'?'插件':'',...entry.definition?.sections.flatMap(s=>[s.title,...s.fields.flatMap(f=>[f.label,f.description,...f.keywords??[]])])??[]].join(' '),query));
  const record=records[selected],dirtyCount=Object.values(records).filter(dirty).length+(plugins.participant.dirty?1:0)+(updates.participant.dirty?1:0);
  const locked=busy||updates.working||plugins.working||!!plugins.importing&&["selecting","verifying"].includes(plugins.importing.state);
  return <div className="app-shell settings-window"><header className="window-topbar" ref={header}><BrandMark size="small"/><strong>设置</strong><div className="window-drag-space" data-chrome-drag onPointerDown={e=>{if(e.button===0)void run('windows.control',{action:'drag'});}} onDoubleClick={()=>void run('windows.control',{action:'toggleMaximize'})}/><WindowControls run={run}/></header>
    <div className="settings-layout"><aside className="settings-navigation"><Input ref={search} autoFocus aria-label="搜索设置" placeholder="搜索设置…" value={query} onChange={e=>setQuery(e.target.value)}/><nav aria-label="设置分组">{(['core','plugin'] as const).map(owner=><div key={owner}><p>{owner==='core'?'应用':'插件'}</p>{shown.filter(e=>e.owner===owner).filter((e,i,all)=>owner==='core'||all.findIndex(other=>other.pluginId===e.pluginId)===i).map(entry=><Button key={entry.pageId} variant={(entry.pageId===selected||entry.pluginId&&entry.pluginId===catalog.find(e=>e.pageId===selected)?.pluginId)?'secondary':'quiet'} aria-current={entry.pageId===selected?'page':undefined} onClick={()=>setSelected(entry.pageId)}>{entry.owner==='plugin'?entry.pluginName??entry.title:entry.title}{(entry.pageId==='core:plugins'?plugins.participant.dirty:entry.pageId==='core:updates'?updates.participant.dirty:records[entry.pageId]&&dirty(records[entry.pageId]))?' ·':''}{!entry.available?' · 不可用':''}</Button>)}</div>)}</nav>{!shown.length&&<p role="status">没有匹配的设置</p>}<Button variant="quiet" onClick={()=>setSelected('core:plugins')}>管理插件</Button></aside>
    <main className="settings-content">{catalog.find(e=>e.pageId===selected)?.pluginId&&catalog.filter(e=>e.pluginId===catalog.find(c=>c.pageId===selected)?.pluginId).length>1&&<Select aria-label="插件设置页面" value={selected} onChange={e=>setSelected(e.target.value)}>{catalog.filter(e=>e.pluginId===catalog.find(c=>c.pageId===selected)?.pluginId).map(e=><option key={e.pageId} value={e.pageId}>{e.title}</option>)}</Select>}{error&&<Notice kind="error">{error}</Notice>}{selected==='core:plugins'?<PluginSettings controller={plugins} updates={updates} disabled={locked||!!closeFreeze.current}/>:selected==='core:updates'?<UpdateSettings controller={updates} disabled={locked||!!closeFreeze.current}/>:selected==='core:application'?<ApplicationSettings onPlugins={()=>setSelected('core:plugins')}/>:record?<>{record.status==='unavailable'&&<Notice kind="warning" action={<Button disabled={busy} onClick={()=>void load(selected)}>重新载入</Button>}>设置提供者暂不可用，草稿已保留。</Notice>}{record.conflict&&<Notice kind="warning" action={<Button disabled={busy} onClick={()=>void load(selected,true)}>载入最新</Button>}>此页已在其他位置改变。草稿仍保留，载入最新值后可继续编辑。</Notice>}{record.operation&&<Notice action={<Button disabled={busy} onClick={()=>void reconcile(selected).catch(e=>setError(errorMessage(e)))}>核对结果</Button>}>正在核对保存结果</Notice>}{record.page.snapshot.warnings.map(w=><Notice key={w}>{w}</Notice>)}{record.page.snapshot.syncState!=='current'&&<Notice action={<Button onClick={()=>void client.sync(selected,record.page.snapshot.settingsRevision).then(()=>load(selected)).catch(e=>setError(errorMessage(e)))}>重试同步</Button>}>配置已保存，同步待恢复</Notice>}<SettingsForm record={record} query={query} disabled={locked||!!closeFreeze.current||record.conflict||!!record.operation||record.status==='unavailable'} onChange={edit}/></>:<p role="status">{catalog.find(e=>e.pageId===selected)?.reason??'正在读取设置…'}</p>}</main></div>
    <footer className="settings-footer"><span role="status">{busy?'正在应用…':message|| (dirtyCount?`${dirtyCount} 个分组有未应用的更改`:'更改后点击应用')}</span><Button onClick={requestClose}>取消</Button><Button disabled={locked||!!closeFreeze.current||!dirtyCount} onClick={()=>void apply()}>应用</Button><Button variant="primary" disabled={locked||!!closeFreeze.current} onClick={()=>void apply(true)}>确定</Button></footer>
    {closing&&<Dialog title={closing.reason==='exit'?'退出前处理设置':'放弃未应用的更改？'} onClose={()=>void closeReply(closing,'cancel')}><p>{locked?'设置仍在保存或导入，请等待结果。':Object.values(records).some(r=>!!r.operation)?'部分保存结果尚未确定，已提交的配置仍可能保存。继续关闭仅丢弃未提交草稿。':'已应用的设置会保留，尚未应用的草稿将丢弃。'}</p><div className="settings-actions"><Button variant="primary" onClick={()=>void closeReply(closing,'cancel')}>继续编辑</Button><Button disabled={locked} onClick={()=>void closeReply(closing,'discard')}>{closing.reason==='exit'?'放弃并退出':'放弃更改'}</Button></div></Dialog>}
  </div>;
}
