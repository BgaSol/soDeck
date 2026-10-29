import {useEffect,useRef,useState} from 'react';
import {core,errorMessage,observeState,onCoreNotification} from '@sodeck/sdk';
import {callQuality,decodeQualityState} from '@sodeck/sdk/quality';
import type {QualitySummary,QualityPolicy,CheckResult,Outcome,SummaryRequest} from '@sodeck/sdk/quality';
import {Button,Checkbox} from '@sodeck/ui';

const call=<T,>(method:string,params:unknown)=>callQuality<T>(core.call,method,params);
const labels:Record<Outcome['outcome'],string>={succeeded:'已收到 SSH 响应',CANCELED:'检查已取消',SCHEDULING_DELAY:'本机调度延迟，本次无法测量',UNSUPPORTED:'服务器不支持诊断通道',DISCONNECTED:'连接已结束',CHECK_TIMEOUT:'SSH 探测未响应',CAPACITY:'探测名额已满，本次跳过'};
const ms=(value:number|null)=>value===null?'样本不足':`${value.toLocaleString('zh-CN',{maximumFractionDigits:1})} ms`;
export function MonitorDiagnostics({target,active,names,onServer}:{target:SummaryRequest;active:boolean;names:Record<string,string>;onServer:(id:string)=>void}) {
  const [summary,setSummary]=useState<QualitySummary>(),[policy,setPolicy]=useState<QualityPolicy>(),[task,setTask]=useState<CheckResult>(),[error,setError]=useState(''),[busy,setBusy]=useState(false),[saving,setSaving]=useState(false);
  const alive=useRef(true),taskId=useRef<string|undefined>(undefined),query=useRef(0),targetKey=JSON.stringify(target);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;if(taskId.current)void call('quality.check.cancel',{taskId:taskId.current}).catch(()=>{});};},[]);
  const refresh=async()=>{const sequence=++query.current;try{const value=await call<QualitySummary>('quality.summary',target);if(alive.current&&sequence===query.current)setSummary(value);}catch(e){if(alive.current&&sequence===query.current)setError(errorMessage(e));}};
  useEffect(()=>{
    if(!active)return;const controller=new AbortController();
    void observeState({call:core.call,onNotification:onCoreNotification},{contract:'communication.quality.v1',target},decodeQualityState,()=>{if(!controller.signal.aborted)void refresh();},{signal:controller.signal,onError:e=>{if(!controller.signal.aborted)setError(errorMessage(e));}}).catch(e=>{if(!controller.signal.aborted)setError(errorMessage(e));});
    return()=>{controller.abort();query.current++;};
  },[targetKey,active]);
  useEffect(()=>{
    let current=true;if(target.serverId)void call<QualityPolicy>('quality.policy.get',target).then(value=>{if(current){setPolicy(value);}}).catch(e=>{if(current)setError(errorMessage(e));});
    return()=>{current=false;};
  },[targetKey]);
  useEffect(()=>{
    if(!task||task.state==='finished'&&!task.cleanupPending)return;let current=true,timer:ReturnType<typeof setTimeout>;
    const poll=async()=>{try{const value=await call<CheckResult>('quality.check.get',{taskId:task.taskId});if(current){setTask(value);if(value.state!=='finished'||value.cleanupPending)timer=setTimeout(()=>void poll(),250);else void refresh();}}catch(e){if(current)setError(errorMessage(e));}};
    timer=setTimeout(()=>void poll(),250);return()=>{current=false;clearTimeout(timer);};
  },[task?.taskId,task?.state,task?.cleanupPending]);
  const start=async()=>{setBusy(true);setError('');try{const value=await call<CheckResult>('quality.check.start',{...target,operationId:crypto.randomUUID()});
    if(!alive.current){void call('quality.check.cancel',{taskId:value.taskId}).catch(()=>{});return;}taskId.current=value.taskId;setTask(value);void refresh();
  }catch(e){if(alive.current)setError(errorMessage(e));}finally{if(alive.current)setBusy(false);}};
  const cancel=async()=>{if(!task)return;try{const value=await call<CheckResult>('quality.check.cancel',{taskId:task.taskId});if(alive.current)setTask(value);}catch(e){if(alive.current)setError(errorMessage(e));}};
  const save=async(patch:Partial<QualityPolicy>)=>{
    if(!policy)return;setSaving(true);setError('');
    try{const value=await call<QualityPolicy>('quality.policy.set',{...target,expectedRevision:policy.revision,patch});if(alive.current){setPolicy(value);void refresh();}}
    catch(e){if(alive.current)setError(errorMessage(e));}finally{if(alive.current)setSaving(false);}
  };
  return <div className="monitor-overview">
    <section className="monitor-metrics" aria-label="SSH 响应检查"><div className="monitor-row"><h2>SSH 往返响应</h2>{target.serverId&&<Button disabled={busy||task?.state==='checking'||task?.cleanupPending} onClick={()=>void start()}>{busy?'正在发起':'检查响应'}</Button>}{task?.state==='checking'&&<Button onClick={()=>void cancel()}>取消检查</Button>}</div>
      <p>只使用已有的已认证连接，检查不会自动连接服务器。测量包含网络、SSH 处理和远端调度时间。</p>
      {target.serverId?policy&&<Checkbox label="持续探测：每 5 秒发送少量 SSH 请求" checked={policy.enabled} disabled={saving} onChange={e=>void save({enabled:e.target.checked})}/>:<p>选择一台服务器，可检查响应或设置持续探测。</p>}
      {task&&<p role="status">{task.result?labels[task.result.outcome]:'正在检查 SSH 响应'}{task.result?.durationMs!==null&&task.result?.durationMs!==undefined?` · ${ms(task.result.durationMs)}`:''}{task.cleanupPending&&task.state==='finished'?' · 正在释放诊断通道':''}</p>}
      {error&&<p role="alert" className="inline-error">{error}</p>}
    </section>
    {summary?.coverage==='partial'&&<p>部分探测明细超出保留容量，统计仅包含已保留样本。</p>}
    {summary?.segments.length===0&&<p>尚无检测记录。主动探测默认关闭；未检测不代表连接异常。</p>}
    {summary?.segments.map(segment=><section key={segment.segmentId} className="monitor-metrics" aria-label={`${names[segment.serverId]??'服务器'}响应统计`}>
      <div className="monitor-row"><h2>{names[segment.serverId]??'服务器'} · {segment.state==='ended'?'连接已结束':segment.state==='checking'?'正在检查':segment.latest?labels[segment.latest.outcome]:'未检测'}</h2>{!target.serverId&&<Button size="compact" onClick={()=>onServer(segment.serverId)}>查看服务器</Button>}</div>
      <p>连接 {segment.connectionId.slice(0,8)} · 路径 {segment.routeRevision.slice(0,8)}{segment.latest?` · ${new Date(segment.latest.observedAt).toLocaleTimeString()}`:''}</p>
      <dl className="monitor-statistics">
        <div><dt>最近一次响应</dt><dd>{segment.latest?.durationMs===null||segment.latest?.durationMs===undefined?'未测得':ms(segment.latest.durationMs)}</dd></div>
        <div><dt>近 5 分钟中位数</dt><dd>{ms(segment.statistics.medianMs)}</dd></div><div><dt>p95</dt><dd>{ms(segment.statistics.p95Ms)}</dd></div>
        <div><dt>连续响应波动</dt><dd>{ms(segment.statistics.variationMs)}</dd></div><div><dt>探测超时率</dt><dd>{segment.statistics.timeoutRate===null?'未测得':`${(segment.statistics.timeoutRate*100).toLocaleString('zh-CN',{maximumFractionDigits:1})}%`}</dd></div>
      </dl>
      <p>成功 {segment.statistics.successes} 次 · 已发送 {segment.statistics.sent} 次 · 超时 {segment.statistics.timeouts} 次 · 跳过或取消 {segment.statistics.skipped} 次</p>
      <p>中位数和 p95 需要至少 10 个成功样本；波动需要至少 10 对连续成功样本。探测超时率不等于链路丢包率。</p>
    </section>)}
  </div>;
}
