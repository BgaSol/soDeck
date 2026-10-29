import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react';
import {core,errorMessage,onCoreNotification,observeState} from '@sodeck/sdk';
import {validateTraffic} from '@sodeck/sdk/traffic';
import type {MonitorContext,Policy} from '@sodeck/sdk/traffic';
import {Button,Input,Select,BrandMark,SegmentedTabs} from '@sodeck/ui';
import {WindowControls} from '../window-controls';
import {loadServers} from '../route-client';
import type {Server} from '../types';
import {MonitorOverview} from './overview';
import {MonitorHistory} from './history';
import {MonitorSettings} from './settings';
import {MonitorDiagnostics} from './diagnostics';
import {useCommunicationPolicy} from './policy';
import './monitor.css';

declare global {interface Window {__SODECK_MONITOR_CONTEXT__?:MonitorContext}}
export function CommunicationMonitor() {
  const [context,setContext]=useState(window.__SODECK_MONITOR_CONTEXT__),[servers,setServers]=useState<Server[]>([]),[search,setSearch]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const header=useRef<HTMLElement>(null),live=useRef(true);
  const [section,setSection]=useState<'overview'|'history'|'diagnostics'|'settings'>('overview'),[visited,setVisited]=useState(new Set(['overview']));
  const {policy,setPolicy,error:policyError}=useCommunicationPolicy();
  const accept=useCallback((next:MonitorContext)=>{
    const failure=validateTraffic('x-results','ui.communicationMonitor.get',next);if(failure){setError(failure);return;}
    setContext(old=>old&&old.generation!==next.generation||old&&BigInt(old.navigationRevision)>BigInt(next.navigationRevision)?old:next);
  },[]);
  useEffect(()=>{
    live.current=true;const off=onCoreNotification(event=>{if(event.type==='communicationMonitor.navigate'&&live.current)accept(event.payload as MonitorContext);});
    void core.call<MonitorContext>('ui.communicationMonitor.get',{}).then(next=>{if(live.current)accept(next);}).catch(e=>{if(live.current)setError(errorMessage(e));});
    return()=>{live.current=false;off();};
  },[accept]);
  useEffect(()=>{
    const controller=new AbortController();let sequence=0;const refresh=()=>{const current=++sequence;void loadServers().then(result=>{if(!controller.signal.aborted&&current===sequence)setServers(result.servers);}).catch(e=>{if(!controller.signal.aborted)setError(errorMessage(e));});};
    void observeState({call:core.call,onNotification:onCoreNotification},{contract:'servers.catalog.v2',target:{app:'current'}},v=>v,refresh,{signal:controller.signal,onError:e=>{if(!controller.signal.aborted)setError(errorMessage(e));}}).catch(e=>{if(!controller.signal.aborted)setError(errorMessage(e));});
    return()=>controller.abort();
  },[]);
  const run=useCallback(async(method:string,params?:unknown)=>{try{return await core.call(method,params);}catch(e){if(live.current)setError(errorMessage(e));}},[]);
  useLayoutEffect(()=>{
    const rect=(element:Element|null)=>{const r=element?.getBoundingClientRect();return {x:r?.x??0,y:r?.y??0,width:r?.width??0,height:r?.height??0};};
    const report=()=>void run('windows.chrome',{titlebar:rect(header.current),drag:rect(document.querySelector('[data-chrome-drag]')),maximize:rect(document.querySelector('[data-chrome-maximize]')),tabs:[]});
    const observer=new ResizeObserver(report);if(header.current)observer.observe(header.current);window.addEventListener('resize',report);report();
    return()=>{observer.disconnect();window.removeEventListener('resize',report);};
  },[run]);
  const navigate=async(serverId:string,layer=context?.target.layer??'channel')=>{
    const epoch=context?.target.target?.configEpoch;if(!epoch||busy)return;setBusy(true);setError('');
    try {const next=await core.call<MonitorContext>('ui.communicationMonitor.open',{operationId:crypto.randomUUID(),target:{target:serverId?{kind:'server',serverId,configEpoch:epoch}:{kind:'all',configEpoch:epoch},layer}});if(live.current)accept(next);}
    catch(e){if(live.current)setError(errorMessage(e));}finally{if(live.current)setBusy(false);}
  };
  const target=context?.target.target,serverId=target?.kind==='server'?target.serverId:'',names=Object.fromEntries(servers.map(s=>[s.id,s.name]));
  const scopeLabel=serverId?names[serverId]??`服务器 ${serverId.slice(0,8)}`:'全部服务器';
  return <div className="app-shell communication-monitor">
    <header className="window-topbar monitor-topbar" ref={header}><BrandMark size="small"/><strong>通信监控</strong>
      <Select className="monitor-scope" aria-label="监控范围" value={serverId} disabled={busy} onChange={e=>void navigate(e.target.value)}><option value="">全部服务器</option>{servers.filter(s=>s.id===serverId||s.name.toLowerCase().includes(search.toLowerCase())).map(s=><option key={s.id} value={s.id}>{s.name}</option>)}{serverId&&!names[serverId]&&<option value={serverId}>{scopeLabel} · 已删除</option>}</Select>
      <div className="window-drag-space" data-chrome-drag onPointerDown={e=>{if(e.button===0)void run('windows.control',{action:'drag'});}} onDoubleClick={()=>void run('windows.control',{action:'toggleMaximize'})}/>
      <Button variant="quiet" size="compact" onClick={()=>void run('ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:appearance'})}>外观</Button><WindowControls run={run}/>
    </header>
    <main className="monitor-content"><div className="monitor-row monitor-range"><Input aria-label="搜索服务器" placeholder="搜索服务器" value={search} onChange={e=>setSearch(e.target.value)}/>
      <Select aria-label="统计口径" value={context?.target.layer??'channel'} disabled={busy} onChange={e=>void navigate(serverId,e.target.value as 'channel'|'transport')}><option value="channel">通道数据</option><option value="transport">外层传输</option></Select>
    </div>
    <h1>通信监控 · {scopeLabel}</h1>
    <SegmentedTabs id="monitor" label="通信监控分区" value={section} items={[{id:'overview',label:'概览'},{id:'history',label:'历史'},{id:'diagnostics',label:'诊断'},{id:'settings',label:'设置'}]} onChange={value=>{setSection(value);setVisited(old=>new Set([...old,value]));}}/>
    {(error||policyError)&&<p className="inline-error" role="alert">{error||policyError} <Button size="compact" onClick={()=>setError('')}>关闭提示</Button></p>}
    {context?<>
      <div id="monitor-panel-overview" role="tabpanel" aria-labelledby="monitor-overview" hidden={section!=='overview'}>{section==='overview'&&<MonitorOverview key={JSON.stringify(context.target)} request={context.target} names={names} unit={policy?.rateUnit??'bytes'} onServer={id=>void navigate(id)}/>}</div>
      <div id="monitor-panel-history" role="tabpanel" aria-labelledby="monitor-history" hidden={section!=='history'}>{visited.has('history')&&<MonitorHistory request={context.target} unit={policy?.rateUnit??'bytes'}/>}</div>
      <div id="monitor-panel-diagnostics" role="tabpanel" aria-labelledby="monitor-diagnostics" hidden={section!=='diagnostics'}>{visited.has('diagnostics')&&target&&<MonitorDiagnostics key={JSON.stringify(target)} target={{configEpoch:target.configEpoch,...(target.kind==='server'?{serverId:target.serverId}:{})}} active={section==='diagnostics'} names={names} onServer={id=>void navigate(id)}/>}</div>
      <div id="monitor-panel-settings" role="tabpanel" aria-labelledby="monitor-settings" hidden={section!=='settings'}>{visited.has('settings')&&<MonitorSettings policy={policy} onSaved={setPolicy}/>}</div>
    </>:<p role="status">正在打开通信监控</p>}
    </main>
  </div>;
}
