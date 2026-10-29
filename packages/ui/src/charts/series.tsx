import {useId,useState} from 'react';
import {Button} from '../components/controls';
export interface SeriesDefinition {id:string;label:string;legendLabel?:string}
export interface SeriesPoint {time:number;values:Readonly<Record<string,number|null>>;gap?:boolean}
export function SeriesLegend({series}:{series:readonly SeriesDefinition[]}) {
  return <span className="sd-chart-legend">{series.map((item,index)=><span key={item.id}><i data-direction={index===0?'send':'receive'}/>{item.legendLabel??item.label} · {index===0?'实线':'虚线'}</span>)}</span>;
}
export function SeriesChart({series,points,unit,formatValue,formatTime=(time:number)=>new Date(time).toLocaleTimeString(),label,fixedMaximum,size='standard'}:{
  series:readonly SeriesDefinition[];points:readonly SeriesPoint[];unit:string;formatValue(value:number):string;formatTime?(time:number):string;label:string;fixedMaximum?:number;size?:'standard'|'compact';
}) {
  const [selectedTime,setSelectedTime]=useState<number|null>(null),[table,setTable]=useState(false),description=useId();
  if(series.length<1||series.length>2||points.length>1200)return <p role="alert">图表需要 1–2 条系列，最多 1200 个时间点。</p>;
  if(fixedMaximum!==undefined&&(!Number.isFinite(fixedMaximum)||fixedMaximum<=0))return <p role="alert">图表上限必须为有限正数。</p>;
  if(!points.length)return <p className="sd-data-coverage">暂无采样数据</p>;
  const valid=(n:number|null|undefined):n is number=>typeof n==='number'&&Number.isFinite(n)&&n>=0;
  const first=points[0].time,last=points[points.length-1].time;
  let index=points.length-1;
  if(selectedTime!==null){index=0;for(let i=1;i<points.length;i++)if(Math.abs(points[i].time-selectedTime)<Math.abs(points[index].time-selectedTime))index=i;}
  const current=points[index],expired=selectedTime!==null&&selectedTime<first;
  const maximum=fixedMaximum??Math.max(1,...points.flatMap(p=>p.gap?[]:series.map(s=>p.values[s.id]).filter(valid)));
  const x=(time:number)=>60+(time-first)/Math.max(1,last-first)*880,y=(value:number)=>210-value/maximum*180;
  const path=(id:string)=>{let connected=false;return points.map(p=>{const v=p.values[id];if(p.gap||!valid(v)){connected=false;return '';}const command=`${connected?'L':'M'}${x(p.time).toFixed(2)},${y(v).toFixed(2)}`;connected=true;return command;}).join(' ');};
  const display=(p:SeriesPoint,id:string)=>p.gap?'采集缺口':valid(p.values[id])?formatValue(p.values[id]):'暂无有效样本';
  return <figure className="sd-trend-chart" data-size={size} aria-label={label}>
    <figcaption><SeriesLegend series={series}/><Button variant="quiet" size="compact" onClick={()=>setTable(v=>!v)}>{table?'显示图表':'显示数据表'}</Button></figcaption>
    {!table&&size==='compact'&&<div className="sd-chart-scale"><span>0</span><span>{formatValue(maximum)} {unit}</span></div>}
    {table?<div className="sd-chart-table"><table><thead><tr><th scope="col">时间</th>{series.map(s=><th scope="col" key={s.id}>{s.label}（{unit}）</th>)}</tr></thead><tbody>{points.map((p,i)=><tr key={`${p.time}:${i}`}><th scope="row">{formatTime(p.time)}</th>{series.map(s=><td key={s.id}>{display(p,s.id)}</td>)}</tr>)}</tbody></table></div>:
    <svg viewBox="0 0 960 250" role="img" tabIndex={0} aria-label={`${label}，单位 ${unit}。使用左右方向键查看时间点。`} aria-describedby={description}
      onKeyDown={event=>{let next=index;if(event.key==='ArrowLeft')next=Math.max(0,index-1);else if(event.key==='ArrowRight')next=Math.min(points.length-1,index+1);else if(event.key==='Home')next=0;else if(event.key==='End')next=points.length-1;else return;event.preventDefault();setSelectedTime(points[next].time);}}
      onPointerMove={event=>{const bounds=event.currentTarget.getBoundingClientRect(),time=first+Math.max(0,Math.min(1,((event.clientX-bounds.left)/bounds.width*960-60)/880))*(last-first);let best=0;for(let i=1;i<points.length;i++)if(Math.abs(points[i].time-time)<Math.abs(points[best].time-time))best=i;setSelectedTime(points[best].time);}}>
      <path className="sd-chart-axis" d="M60,30 V210 H940"/>{size==='standard'&&<><text x="6" y="32">{formatValue(maximum)}</text><text x="32" y="214">0</text><text x="60" y="242">{formatTime(first)}</text><text x="940" y="242" textAnchor="end">{formatTime(last)}</text></>}
      {series.map((s,i)=><path key={s.id} className="sd-chart-line" data-direction={i===0?'send':'receive'} d={path(s.id)}/>)}
      {!current.gap&&series.map((s,i)=>valid(current.values[s.id])&&<circle key={s.id} className="sd-chart-point" data-direction={i===0?'send':'receive'} cx={x(current.time)} cy={y(current.values[s.id]!)} r="4"/>)}
    </svg>}
    {!table&&size==='compact'&&<div className="sd-chart-scale"><span>{formatTime(first)}</span><span>{formatTime(last)}</span></div>}
    <p id={description} className="sd-chart-readout">{expired?'所选数据已过期 · ':''}{formatTime(current.time)}{series.map(s=>` · ${s.label} ${display(current,s.id)}`)}{!current.gap&&` ${unit}`}</p>
  </figure>;
}
