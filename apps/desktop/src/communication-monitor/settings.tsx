import {useEffect,useRef,useState} from 'react';
import {core,errorMessage} from '@sodeck/sdk';
import type {Policy} from '@sodeck/sdk/traffic';
import {Button,Checkbox,SelectField} from '@sodeck/ui';
export function MonitorSettings({policy,onSaved}:{policy:Policy|undefined;onSaved(policy:Policy):void}) {
 const [draft,setDraft]=useState(policy),[busy,setBusy]=useState(false),[error,setError]=useState(''),[saved,setSaved]=useState(false),live=useRef(true);
 useEffect(()=>{setDraft(policy);},[policy]);useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 async function save(){if(!draft||!policy||busy)return;setBusy(true);setError('');setSaved(false);
  try{const next=await core.call<Policy>('communication.policy.set',{expectedRevision:policy.revision,patch:{saveHistory:draft.saveHistory,rateUnit:draft.rateUnit,notifications:draft.notifications}});if(live.current){onSaved(next);setSaved(true);}}
  catch(e){if(live.current)setError(errorMessage(e));}finally{if(live.current)setBusy(false);}
 }
 return <section className="monitor-metrics" aria-label="通信设置"><h2>设置</h2>{draft?<form className="monitor-settings" onSubmit={e=>{e.preventDefault();void save();}}>
  <SelectField label="速率单位" value={draft.rateUnit} disabled={busy} onChange={e=>setDraft({...draft,rateUnit:e.target.value as Policy['rateUnit']})}><option value="bytes">字节每秒（B/s、KiB/s）</option><option value="bits">比特每秒（bit/s、Mbit/s）</option></SelectField>
  <Checkbox label="保存通信历史" checked={draft.saveHistory} disabled={busy} onChange={e=>setDraft({...draft,saveHistory:e.target.checked})}/><p className="form-help">关闭后继续显示实时数据，已有历史保留；重新开启只记录之后的数据。调用记录始终保存。</p>
  <div className="monitor-row"><Button type="submit" disabled={busy}>保存设置</Button><Button disabled={busy} onClick={()=>setDraft({...draft,saveHistory:true,rateUnit:'bytes',notifications:true})}>恢复建议值</Button></div>
  {saved&&<p role="status">设置已保存</p>}
 </form>:<p role="status">正在读取设置</p>}{error&&<p className="inline-error" role="alert">{error}</p>}</section>;
}
