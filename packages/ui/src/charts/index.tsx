import {SeriesChart,SeriesLegend,type SeriesPoint} from './series';
export * from './series';
export interface TrendPoint {time:number;send:number|null;receive:number|null;gap?:boolean}
const trafficSeries=[{id:'send',label:'↑ 发送'},{id:'receive',label:'↓ 接收'}];
export function ChartLegend(){return <SeriesLegend series={trafficSeries}/>;}
export function TrendChart({points,...props}:{points:readonly TrendPoint[];unit:string;formatValue(value:number):string;formatTime?(time:number):string;label?:string}) {
  const values:SeriesPoint[]=points.map(p=>({time:p.time,values:{send:p.send,receive:p.receive},gap:p.gap}));
  return <SeriesChart {...props} label={props.label??'通信速率'} points={values} series={[{id:'send',label:'发送',legendLabel:'↑ 发送'},{id:'receive',label:'接收',legendLabel:'↓ 接收'}]}/>;
}
