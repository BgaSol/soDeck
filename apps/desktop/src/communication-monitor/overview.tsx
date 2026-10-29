import {useEffect,useState} from 'react';
import {core,errorMessage,formatByteCount,formatTrafficRate,type CommunicationSummary} from '@sodeck/sdk';
import type {LiveHistoryResult,SummaryRequest} from '@sodeck/sdk/traffic';
import {Button,DataCoverage,MetricPair,Select,TrendChart,type TrendPoint} from '@sodeck/ui';
import {trafficState,useTrafficSummary} from './client';
import {Sources} from './sources';

type CurvePoint=TrendPoint&{duration:number};
type Curve={points:CurvePoint[];until:number;revision:bigint;resolution:number;from:number;retained:number|null};
function append(curve:Curve,value:CommunicationSummary,minutes:number):Curve {
  if(value.observedAt<=curve.until)return curve;
  const gap=BigInt(value.revision)!==curve.revision+1n||value.observedAt-curve.until>3000||value.state!=='active'||value.sendRate===null||value.receiveRate===null;
  const next:CurvePoint={time:curve.from+(Math.floor((value.observedAt-curve.from-1)/curve.resolution)+1)*curve.resolution,
    send:value.sendRate===null?null:value.sendRate/1024,receive:value.receiveRate===null?null:value.receiveRate/1024,gap,duration:value.observedAt-curve.until};
  const points=[...curve.points],last=points.at(-1);
  if(last&&next.time===last.time){
    const duration=last.duration+next.duration;
    const mean=(direction:'send'|'receive')=>last[direction]===null||next[direction]===null?null:(last[direction]*last.duration+next[direction]*next.duration)/duration;
    points[points.length-1]={...next,gap:last.gap||gap,send:mean('send'),receive:mean('receive'),duration};
  }else points.push(next);
  return {...curve,points:points.filter(p=>p.time>=value.observedAt-minutes*60_000).slice(-1200),until:value.observedAt,revision:BigInt(value.revision)};
}
export function MonitorOverview({request,names,onServer,unit='bytes'}:{request:SummaryRequest;names:Record<string,string>;onServer(id:string):void;unit?:'bytes'|'bits'}) {
  const {value,error,retry}=useTrafficSummary(request),[minutes,setMinutes]=useState<1|5|30|60>(5),[curve,setCurve]=useState<Curve>(),[curveError,setCurveError]=useState(''),[refresh,setRefresh]=useState(0);
  const key=JSON.stringify(request);
  useEffect(()=>{
    let active=true;setCurve(undefined);setCurveError('');
    void core.call<LiveHistoryResult>('communication.history.query',{...JSON.parse(key),range:{kind:'live',minutes}}).then(result=>{if(active)setCurve({
      points:result.points.map(p=>({time:p.toUtc,send:p.sendRate===null?null:p.sendRate/1024,receive:p.receiveRate===null?null:p.receiveRate/1024,gap:p.gap,duration:p.validMs})),
      until:result.toUtc,revision:BigInt(result.summary.revision),resolution:result.resolutionMs,from:result.fromUtc,retained:result.retainedFromUtc,
    });}).catch(e=>{if(active)setCurveError(errorMessage(e));});return()=>{active=false;};
  },[key,minutes,refresh]);
  useEffect(()=>{if(value)setCurve(current=>current?append(current,value,minutes):current);},[value,minutes]);
  return <div className="monitor-overview">
    <section className="monitor-metrics" aria-label="通信摘要">
      <MetricPair send={formatTrafficRate(value?.state==='active'?value.sendRate:null,unit)} receive={formatTrafficRate(value?.state==='active'?value.receiveRate:null,unit)} sendTotal={value?formatByteCount(value.sentBytes):undefined} receiveTotal={value?formatByteCount(value.receivedBytes):undefined}/>
      <p>{value?trafficState(value.state):'正在获取通信数据'}{value&&<> · <DataCoverage partial={value.coverage==='partial'}/></>}</p>
      <p className="form-help">{request.layer==='transport'?'外层传输包含 SSH 加密、认证和协议开销。':'通道数据按实际读写计量；跳板中转不重复计入业务总量。'}{value&&` 本次累计始于 ${new Date(value.startedAt).toLocaleString()}。`}</p>
      {error&&<p className="inline-error" role="alert">{error} <Button size="compact" onClick={retry}>重试</Button></p>}
    </section>
    <section className="monitor-trend" aria-label="实时趋势"><div className="monitor-row"><h2>实时趋势</h2><Select aria-label="曲线时间范围" value={minutes} onChange={e=>setMinutes(Number(e.target.value) as typeof minutes)}>{[1,5,30,60].map(v=><option key={v} value={v}>最近 {v} 分钟</option>)}</Select></div>
      {curve?<><TrendChart points={unit==='bytes'?curve.points:curve.points.map(p=>({...p,send:p.send===null?null:p.send*8.192,receive:p.receive===null?null:p.receive*8.192}))} unit={unit==='bytes'?'KiB/s':'kbit/s'} formatValue={v=>v.toLocaleString('zh-CN',{maximumFractionDigits:1})}/><p className="form-help">缺失时段不补为零。{curve.retained!==null?` 当前保留起点 ${new Date(curve.retained).toLocaleTimeString()}。`:' 尚无有效采样。'}</p></>:!curveError&&<p role="status">正在读取实时曲线</p>}
      {curveError&&<p className="inline-error" role="alert">{curveError} <Button size="compact" onClick={()=>setRefresh(v=>v+1)}>重试</Button></p>}
    </section>
    <Sources request={request} names={names} onServer={onServer} unit={unit}/>
  </div>;
}
