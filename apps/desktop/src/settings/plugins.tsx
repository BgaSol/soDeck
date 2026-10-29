import {useCallback,useEffect,useRef,useState} from 'react';
import {core,errorMessage,onCoreNotification,observeState} from '@sodeck/sdk';
import type {EnablementChange,EnablementOperation,EnablementSaved,EnablementSnapshot,ManagedPlugin,ManagementList,PackageInfo,PackageOperation} from '@sodeck/sdk/plugin-management';
import {Button,Checkbox,Input,Notice} from '@sodeck/ui';
import {acceptPlugins,editPlugin,emptyPlugins,installPlugins,pluginChanges,type PluginDraft} from './plugin-model';
import type {SettingsSaveParticipant} from './save-coordinator';
import {PluginUpdate} from './updates';
import type {UpdatesController} from './update-model';
const bridge={call:core.call,onNotification:onCoreNotification};
type PendingSave={id:string;generation:string;changes:EnablementChange[]};
const uncertain=(e:unknown)=>['TIMEOUT','TRANSPORT_ERROR','RESULT_UNKNOWN'].includes((e as {code?:string}).code??'');
export function usePluginManagement(){
  const [model,setModel]=useState(emptyPlugins),[error,setError]=useState(''),[loading,setLoading]=useState(true),[notice,setNotice]=useState('');
  const [operation,setOperation]=useState<PendingSave>(),[importing,setImporting]=useState<PackageOperation>();
  const [working,setWorking]=useState(false);
  const current=useRef(model),pending=useRef(operation),importRef=useRef(importing),live=useRef(true),sequence=useRef(0);
  const update=useCallback((f:(m:PluginDraft)=>PluginDraft)=>{current.current=f(current.current);setModel(current.current);},[]);
  const saveOperation=(o:PendingSave|undefined)=>{pending.current=o;setOperation(o);};
  const importOperation=(o:PackageOperation|undefined)=>{importRef.current=o;setImporting(o);};
  const refresh=useCallback(async(discard=false)=>{
    const seq=++sequence.current;
    try{
      // Both lists and preferences must describe the same saved revision.
      for(let attempt=0;attempt<3;attempt++){
        const snapshot=await core.call<EnablementSnapshot>('plugins.enablement.get',{});
        const rows:ManagedPlugin[]=[];let cursor:string|undefined;
        try{do{const page=await core.call<ManagementList>('plugins.management.list',{limit:100,...(cursor?{cursor}:{})});rows.push(...page.items);cursor=page.nextCursor??undefined;}while(cursor&&live.current&&seq===sequence.current);}
        catch(e){if((e as {code?:string}).code==='CONFLICT'&&attempt<2)continue;throw e;}
        const end=await core.call<EnablementSnapshot>('plugins.enablement.get',{});
        if(end.generation!==snapshot.generation||end.revision!==snapshot.revision){if(attempt<2)continue;throw Error('插件状态正在改变，请重新载入');}
        if(live.current&&seq===sequence.current){update(m=>installPlugins(m,rows,snapshot,discard));setError('');setLoading(false);}return;
      }
    }catch(e){if(live.current&&seq===sequence.current){setLoading(false);setError(errorMessage(e));}throw e;}
  },[update]);
  useEffect(()=>{
    live.current=true;const abort=new AbortController();
    void refresh().catch(()=>{});
    void observeState(bridge,{contract:'plugins.management.v1',target:{app:'current'}},v=>v,()=>void refresh().catch(()=>{}),{signal:abort.signal,onError:e=>{if(live.current)setError(errorMessage(e));}}).catch(e=>{if(live.current)setError(errorMessage(e));});
    return()=>{live.current=false;sequence.current++;abort.abort();};
  },[refresh]);
  const finishImport=useCallback((value:PackageOperation)=>{
    importOperation(value);
    if(value.state==='prepared'){setNotice('插件包已准备好，退出并重新启动 SoDeck 后生效。');void refresh().catch(()=>{});}
    if(value.state==='failed')setError(value.error??'插件包导入失败');
  },[refresh]);
  useEffect(()=>{
    if(!importing||!['selecting','verifying'].includes(importing.state))return;
    let stopped=false;let timer:ReturnType<typeof setTimeout>;
    const poll=async()=>{try{const result=await core.call<PackageOperation>('plugins.package.operation.get',{operationId:importing.operationId});if(stopped)return;finishImport(result);if(['selecting','verifying'].includes(result.state))timer=setTimeout(()=>void poll(),500);}catch(e){if(!stopped){setError(errorMessage(e));timer=setTimeout(()=>void poll(),2000);}}};
    timer=setTimeout(()=>void poll(),500);return()=>{stopped=true;clearTimeout(timer);};
  },[importing?.operationId,importing?.state,finishImport]);
  const reconcile=async()=>{
    const op=pending.current;if(!op)return true;
    const result=await core.call<EnablementOperation>('plugins.enablement.operation.get',{generation:op.generation,operationId:op.id},10_000);
    if(result.state==='saved'){saveOperation(undefined);update(m=>acceptPlugins(m,op.changes));await refresh();setNotice('启用选择已保存，退出并重新启动 SoDeck 后生效。');return true;}
    if(result.state==='rejected'){saveOperation(undefined);throw Error(result.reason??'更改未保存，草稿已保留');}
    return false;
  };
  const participant:SettingsSaveParticipant={
    id:'core:plugins',dirty:pluginChanges(model).length>0,
    unresolvedOperations:[...(operation?[operation.id]:[]),...(importing&&['selecting','verifying','unknown'].includes(importing.state)?[importing.operationId]:[])],
    async validate(){const m=current.current;if(m.conflict)throw Error('插件启用设置已改变，请先载入最新值');if(m.snapshot?.storageStatus!=='ready'||!m.snapshot.generation)throw Error('插件启用配置不可用');await core.call('plugins.enablement.validate',{generation:m.snapshot.generation,expectedRevision:m.snapshot.revision,changes:pluginChanges(m)});},
    async commit(){const m=current.current,changes=pluginChanges(m);const op={id:crypto.randomUUID(),generation:m.snapshot!.generation!,changes};saveOperation(op);
      try{await core.call<EnablementSaved>('plugins.enablement.commit',{generation:op.generation,expectedRevision:m.snapshot!.revision,changes,operationId:op.id},10_000);saveOperation(undefined);update(m=>acceptPlugins(m,changes));await refresh();setNotice('启用选择已保存，退出并重新启动 SoDeck 后生效。');}
      catch(e){if(uncertain(e)){setNotice('保存结果尚未确定，正在核对，请勿重复提交。');if(await reconcile())return;throw Error('保存结果尚未确定，请核对结果后再应用');}saveOperation(undefined);throw e;}
    },reconcile,
    async discard(){if(pending.current)throw Error('请先核对保存结果');await refresh(true);},
  };
  const importPackage=async()=>{
    if(working||pending.current||importRef.current&&['selecting','verifying','unknown'].includes(importRef.current.state))return;
    const operationId=crypto.randomUUID();setError('');setNotice('');setWorking(true);importOperation({operationId,state:'selecting'});
    try{finishImport(await core.call<PackageOperation>('plugins.package.prepare',{operationId}));}catch(e){if(uncertain(e))importOperation({operationId,state:'verifying'});else{importOperation({operationId,state:'failed'});setError(errorMessage(e));}}finally{setWorking(false);}
  };
  const repair=async()=>{
    const token=current.current.snapshot?.diagnosticToken;if(!token||working)return;setWorking(true);setError('');
    const operationId=crypto.randomUUID();
    try{const result=await core.call<EnablementSaved>('plugins.enablement.repair',{operationId,diagnosticToken:token});await refresh(true);setNotice(`已备份原配置并恢复为全部停用。备份：${result.backupPath}`);}
    catch(e){setError(errorMessage(e));await refresh().catch(()=>{});}finally{setWorking(false);}
  };
  return {model,error,loading,notice,operation,importing,working,participant,refresh,reconcile,repair,importPackage,reportError:(e:unknown)=>setError(errorMessage(e)),
    edit:(id:string,value:boolean)=>{if(!pending.current){setNotice('');update(m=>editPlugin(m,id,value));}},
    reconcileImport:async()=>{if(importRef.current)finishImport(await core.call<PackageOperation>('plugins.package.operation.get',{operationId:importRef.current.operationId}));},
  };
}
const signatureText={unsigned:'未签名 · 完整性已通过',verified:'签名已验证',developmentVerified:'开发签名已验证',unverified:'签名来源未验证 · 完整性已通过',invalid:'验证未通过'};
function PackageVersion({label,value}:{label:string;value:PackageInfo|null}){
  return value?<p>{label} {value.version} · {value.valid?signatureText[value.signature.status]:'完整性或包验证失败'}{value.error&&`：${value.error}`}</p>:null;
}
export function PluginSettings({controller:c,disabled,updates}:{controller:ReturnType<typeof usePluginManagement>;disabled:boolean;updates?:UpdatesController}){
  const [query,setQuery]=useState('');const {model}=c;
  const rows=model.rows.filter(r=>`${r.displayName} ${r.pluginId}`.toLowerCase().includes(query.toLowerCase()));
  const importing=!!c.importing&&['selecting','verifying'].includes(c.importing.state);
  const frozen=disabled||c.working||importing||!!c.operation;
  const action=(fn:()=>Promise<unknown>)=>void fn().catch(c.reportError);
  return <section className="settings-form plugin-settings"><div className="settings-heading"><h1>插件管理</h1><Button disabled={frozen} onClick={()=>void c.importPackage()}>安装或更新插件</Button></div>
    <p>勾选需要启用的插件，点击底部“应用”保存。退出并重新启动 SoDeck 后生效，插件包和配置会保留。</p>
    <p>导入插件包会立即准备候选版本，取消设置不会撤销导入。</p>
    {c.error&&<Notice kind="error" action={<Button disabled={frozen} onClick={()=>action(()=>c.refresh())}>重新载入</Button>}>{c.error}</Notice>}
    {c.notice&&<Notice>{c.notice}</Notice>}
    {importing&&<Notice>{c.importing?.state==='selecting'?'正在选择插件包…':'正在验证插件包…'}</Notice>}
    {c.importing?.state==='unknown'&&<Notice kind="warning" action={<Button onClick={()=>action(c.reconcileImport)}>核对导入结果</Button>}>导入结果尚未确定，请核对已安装版本后重新打开设置。</Notice>}
    {model.snapshot&&model.snapshot.storageStatus!=='ready'&&<Notice kind="warning" action={model.snapshot.storageStatus==='corrupt'?<Button disabled={frozen} onClick={()=>void c.repair()}>备份并恢复为全部停用</Button>:undefined}>{model.snapshot.reason}</Notice>}
    {model.conflict&&<Notice kind="warning" action={<Button disabled={frozen} onClick={()=>action(()=>c.participant.discard())}>载入最新</Button>}>启用设置已在其他位置改变，当前草稿已保留。载入最新值将放弃这些草稿。</Notice>}
    {c.operation&&<Notice action={<Button disabled={disabled} onClick={()=>action(c.reconcile)}>核对结果</Button>}>正在核对保存结果，此组暂时不能再次应用。</Notice>}
    {model.rows.some(r=>r.restartRequired)&&<Notice>有已保存的更改待重启生效。关闭主窗口仅收起到托盘，请使用“退出”后重新启动。</Notice>}
    <Input aria-label="搜索插件" placeholder="搜索名称或插件标识…" value={query} onChange={e=>setQuery(e.target.value)}/>
    {c.loading?<p role="status">正在读取插件…</p>:!rows.length?<p role="status">{model.rows.length?'没有匹配的插件':'尚未安装插件。核心服务器与设置管理仍可使用。'}</p>:<div className="plugin-settings-list">{rows.map(row=><article className="plugin-settings-row" key={row.pluginId}>
      <Checkbox label="启用" aria-label={`启用 ${row.displayName}`} checked={model.edits[row.pluginId]??row.enabled} disabled={frozen||model.conflict||model.snapshot?.storageStatus!=='ready'} onChange={e=>c.edit(row.pluginId,e.target.checked)}/>
      <div><h2>{row.displayName}</h2><p className="plugin-settings-id">{row.pluginId}</p><p>本次运行：{{running:'运行中',disabled:'已停用',failed:'加载失败',pending:'尚未加载'}[row.runtimeStatus]}{row.restartRequired?' · 待重启':''}{model.edits[row.pluginId]!==undefined?' · 未应用':''}</p>
      {updates&&<PluginUpdate controller={updates} id={row.pluginId} digest={row.pending?.digest??row.selected?.digest??undefined} disabled={frozen}/>}
      <PackageVersion label="当前版本" value={row.selected}/><PackageVersion label="待启用版本" value={row.pending}/>{row.error&&<Notice kind="error">{row.error}</Notice>}</div>
    </article>)}</div>}
  </section>;
}
