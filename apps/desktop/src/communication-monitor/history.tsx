import {useEffect,useRef,useState} from 'react';
import {core,errorMessage,formatByteCount,formatTrafficRate} from '@sodeck/sdk';
import type {CompareResult,HistoryPoint,StoredHistoryResult,SummaryRequest} from '@sodeck/sdk/traffic';
import {Button,Checkbox,DataCoverage,MetricPair,SelectField,TextField,TrendChart} from '@sodeck/ui';

function localTime(value:number){const date=new Date(value);return new Date(value-date.getTimezoneOffset()*60000).toISOString().slice(0,19);}
function range(hours:number){const to=Date.now();return {from:localTime(to-hours*3600000),to:localTime(to)};}
function utc(from:string,to:string){const a=new Date(from).getTime(),b=new Date(to).getTime();if(!Number.isFinite(a)||!Number.isFinite(b)||b<=a||b-a>30*86400000)throw new Error('请选择有效的开始与结束时间，区间最长 30 天。');return {kind:'utc' as const,fromUtc:a,toUtc:b};}
function Stats({point,unit}:{point:HistoryPoint;unit:'bytes'|'bits'}) {
 return <><MetricPair send={formatTrafficRate(point.sendRate,unit)} receive={formatTrafficRate(point.receiveRate,unit)} sendTotal={formatByteCount(point.sentBytes)} receiveTotal={formatByteCount(point.receivedBytes)}/>
  <p className="form-help">上方速率为有效时段平均，包含已连接的空闲时间。</p>
  <dl className="monitor-statistics"><div><dt>发送峰值</dt><dd>{formatTrafficRate(point.peakSendRate??null,unit)}</dd></div><div><dt>接收峰值</dt><dd>{formatTrafficRate(point.peakReceiveRate??null,unit)}</dd></div><div><dt>有效时长</dt><dd>{(point.validMs/1000).toLocaleString('zh-CN',{maximumFractionDigits:1})} 秒</dd></div><div><dt>峰值采样粒度</dt><dd>{point.sampleCount?`${(point.peakResolutionMs??1000)/1000} 秒`:'—'}</dd></div></dl></>;
}
export function MonitorHistory({request,unit}:{request:SummaryRequest;unit:'bytes'|'bits'}) {
 const [period,setPeriod]=useState(()=>range(1)),[preset,setPreset]=useState('1'),[compare,setCompare]=useState(false),[second,setSecond]=useState<{from:string;to:string}>(),[data,setData]=useState<StoredHistoryResult>(),[comparison,setComparison]=useState<CompareResult>(),[loading,setLoading]=useState(false),[error,setError]=useState(''),[refresh,setRefresh]=useState(0);
 const sequence=useRef(0),key=JSON.stringify(request);
 const selected=useRef({period,compare,second});selected.current={period,compare,second};
 useEffect(()=>{
  const token=++sequence.current;const current=selected.current;setLoading(true);setError('');
  void (async()=>{
   const first=utc(current.period.from,current.period.to);let result:StoredHistoryResult;let compared:CompareResult|undefined;
   if(current.compare){const previous=current.second?utc(current.second.from,current.second.to):{kind:'utc' as const,fromUtc:first.fromUtc-(first.toUtc-first.fromUtc),toUtc:first.fromUtc};
    compared=await core.call<CompareResult>('communication.history.compare',{...JSON.parse(key),first,second:previous});result=compared.first;
   }else result=await core.call<StoredHistoryResult>('communication.history.query',{...JSON.parse(key),range:first});
   if(token===sequence.current){setData(result);setComparison(compared);}
  })().catch(e=>{if(token===sequence.current)setError(errorMessage(e));}).finally(()=>{if(token===sequence.current)setLoading(false);});
  return()=>{sequence.current++;};
 },[key,refresh]);
 const defaults=()=>{try{const a=utc(period.from,period.to);return {from:localTime(a.fromUtc-(a.toUtc-a.fromUtc)),to:period.from};}catch{return period;}};
 const alternative=second??defaults(),scale=unit==='bits'?8/1000:1/1024;
 return <div className="monitor-overview">
  <section className="monitor-metrics" aria-label="历史查询条件"><h2>历史</h2><form onSubmit={event=>{event.preventDefault();setRefresh(value=>value+1);}}>
   <div className="monitor-row"><SelectField label="时间范围" value={preset} onChange={e=>{setPreset(e.target.value);setSecond(undefined);if(e.target.value!=='custom'){setPeriod(range(Number(e.target.value)));setRefresh(v=>v+1);}}}>
    <option value="1">最近 1 小时</option><option value="24">最近 24 小时</option><option value="168">最近 7 天</option><option value="720">最近 30 天</option><option value="custom">自定义</option></SelectField>
    <TextField label="开始时间" type="datetime-local" step="1" required value={period.from} onChange={e=>{setPreset('custom');setPeriod(p=>({...p,from:e.target.value}));}}/>
    <TextField label="结束时间" type="datetime-local" step="1" required value={period.to} onChange={e=>{setPreset('custom');setPeriod(p=>({...p,to:e.target.value}));}}/>
   </div><p className="form-help">时间按本地时区 {Intl.DateTimeFormat().resolvedOptions().timeZone} 显示；每个区间最长 30 天。</p>
   <Checkbox label="对比另一个区间" checked={compare} onChange={e=>setCompare(e.target.checked)}/>
   {compare&&<div className="monitor-row"><TextField label="对比开始时间" type="datetime-local" step="1" required value={alternative.from} onChange={e=>setSecond({...alternative,from:e.target.value})}/><TextField label="对比结束时间" type="datetime-local" step="1" required value={alternative.to} onChange={e=>setSecond({...alternative,to:e.target.value})}/></div>}
   <Button type="submit" disabled={loading}>查询历史</Button>{loading&&<p role="status">正在查询历史</p>}
  </form>{error&&<p className="inline-error" role="alert">{error} <Button size="compact" onClick={()=>setRefresh(v=>v+1)}>重试</Button></p>}</section>
  {data&&<><section className="monitor-metrics" aria-label="历史统计"><h2>查询结果</h2><p>{new Date(data.fromUtc).toLocaleString()} — {new Date(data.toUtc).toLocaleString()} · <DataCoverage partial={data.coverage==='partial'}/></p>
   <Stats point={data.statistics} unit={unit}/>
   {data.statistics.sampleCount===0&&<p>这个区间没有已保存的通信采样。应用运行并建立连接后才会产生记录。</p>}
   {data.coverage==='partial'&&<p className="form-help">总量仅包含已知数据，未按覆盖比例推算。缺失时段不补零。{data.boundaryOmitted&&'区间边缘无法拆分的采样未计入，请扩大范围查看完整桶。'}</p>}
   <p className="form-help">当前保留起点：{data.retainedFromUtc===null?'暂无数据':new Date(data.retainedFromUtc).toLocaleString()}。</p>
  </section><section className="monitor-trend" aria-label="历史趋势"><h2>历史趋势</h2><TrendChart points={data.points.map(p=>({time:p.toUtc,send:p.sendRate===null?null:p.sendRate*scale,receive:p.receiveRate===null?null:p.receiveRate*scale,gap:p.gap}))} unit={unit==='bits'?'kbit/s':'KiB/s'} formatValue={v=>v.toLocaleString('zh-CN',{maximumFractionDigits:1})}/><p className="form-help">曲线粒度 {data.resolutionMs/1000} 秒。峰值来自桶内有效采样，平均线不代表峰值。</p></section></>}
  {comparison&&<section className="monitor-metrics" aria-label="对比统计"><h2>对比区间</h2><p>{new Date(comparison.second.fromUtc).toLocaleString()} — {new Date(comparison.second.toUtc).toLocaleString()}</p><Stats point={comparison.second.statistics} unit={unit}/><p className="form-help">{comparison.comparable?'两个区间长度和数据覆盖一致，可比较数值。':'两个区间的长度或数据覆盖不同，仅并列已知数值。'}数据差异不直接说明连接性能变化。</p></section>}
 </div>;
}
