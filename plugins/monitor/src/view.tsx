import {core,sameDiagnostic,errorMessage,openConnectionDiagnostics} from '@sodeck/sdk';
import {useEffect,useState,type ReactNode} from 'react';
import {Button,IconButton,SelectField,Checkbox,Stack,Inline,ScrollArea,Surface,Toolbar,Text,Notice,StatusBar,Badge,PropertyList,SeriesChart,capacityUsage,type SeriesPoint} from '@sodeck/ui';
import {Pause,Play,RefreshCw} from 'lucide-react';
import {onThemeError,retryTheme} from '@sodeck/sdk';
import {useMonitor} from './view-client';
import type {Metric,Options,Snapshot} from './protocol';
declare const __MONITOR_VERSION__:string;
const bytes=(value:string|number|null|undefined)=>{if(value===null||value===undefined)return '—';const n=Number(value);if(!Number.isFinite(n))return '—';const units=['B','KiB','MiB','GiB','TiB','PiB'];let index=0,v=n;while(v>=1024&&index<units.length-1){v/=1024;index++;}return `${v.toFixed(index?1:0)} ${units[index]}`;};
const percent=(value:number|null|undefined)=>value===null||value===undefined?'计算中':`${value.toFixed(1)}%`;
const rate=(value:number|null|undefined)=>value===null||value===undefined?'计算中':`${bytes(value)}/s`;
const actionId=()=>crypto.randomUUID();
function Age({metric,elapsed}:{metric:Metric<unknown>;elapsed:number}) {
  const age=metric.ageMs===null?null:Math.floor((metric.ageMs+elapsed)/1000);
  return <Text size="small" tone="muted">{metric.quality==='stale'?'旧数据 · ':''}{metric.message??(age===null?'等待采集':age<2?'刚刚更新':`${age} 秒前更新`)}</Text>;
}
function Card({title,value,children,detail,onDetailToggle}:{title:string;value:ReactNode;children:ReactNode;detail?:ReactNode;onDetailToggle?(open:boolean):void}) {
  return <Surface className="monitor-card"><Stack gap="xs"><Inline className="monitor-heading"><Text size="title">{title}</Text><Text numeric size="title">{value}</Text></Inline>{children}{detail&&<details onToggle={event=>onDetailToggle?.(event.currentTarget.open)}><summary>查看明细</summary>{detail}</details>}</Stack></Surface>;
}
function Chart({snapshot,elapsed,keys,labels,percentage=false,identity}:{snapshot:Snapshot;elapsed:number;keys:string[];labels:string[];percentage?:boolean;identity?:string|null}) {
  const times=new Map<number,Record<string,number|null>>();
  keys.forEach((key,i)=>{for(const p of snapshot.trends[key]??[]){const time=p.time+snapshot.timelineMs;const row=times.get(time)??{};row[String(i)]=p.value;times.set(time,row);}});
  const points:SeriesPoint[]=[...times].sort(([a],[b])=>a-b).map(([time,values])=>({time,values}));
  return <SeriesChart key={identity??keys.join(':')} size="compact" label={labels.join(' / ')+'趋势'} series={labels.map((label,i)=>({id:String(i),label}))} points={points} unit={percentage?'%':'/s'} fixedMaximum={percentage?100:undefined} formatValue={n=>percentage?n.toFixed(1):bytes(n)} formatTime={time=>`${Math.max(0,Math.round((snapshot.timelineMs+elapsed-time)/1000))}秒前`}/>;
}
export function MonitorView() {
  const {snapshot:s,error,errorRef,elapsed,action}=useMonitor(),[themeError,setThemeError]=useState<string>(),[detailError,setDetailError]=useState('');
  useEffect(()=>onThemeError(setThemeError),[]);
  const options=(options:Partial<Options>)=>void action('monitor.setOptions',{options});
  const select=(kind:string,id:string|null,extra:Record<string,unknown>={})=>void action('monitor.select',{kind,id,...extra});
  const network=s?.network.value?.find(r=>r.id===s.selection.network),disk=s?.disk.value?.find(r=>r.id===s.selection.disk),mount=s?.selectedMount,capacity=mount?.capacity.value;
  const pages=(kind:'disk'|'network')=>s&&s[kind==='disk'?'diskCount':'networkCount']>31&&<Inline><Button size="compact" disabled={s.selection[kind==='disk'?'diskPage':'networkPage']===0} onClick={()=>select(kind,null,{page:s.selection[kind==='disk'?'diskPage':'networkPage']-1})}>上一页</Button><Button size="compact" disabled={(s.selection[kind==='disk'?'diskPage':'networkPage']+1)*31>=s[kind==='disk'?'diskCount':'networkCount']} onClick={()=>select(kind,null,{page:s.selection[kind==='disk'?'diskPage':'networkPage']+1})}>下一页</Button></Inline>;
  const pressure=capacity?capacityUsage({used:capacity.used,total:capacity.denominator}):null;
  return <main className="monitor-view" aria-label="远端资源监控">
    <Toolbar className="monitor-toolbar"><Text size="title">资源监控</Text><Inline gap="xs"><IconButton label={s?.options.paused?'恢复采集':'暂停采集'} icon={s?.options.paused?Play:Pause} disabled={!s} variant="quiet" onClick={()=>options({paused:!s?.options.paused})}/><IconButton label="刷新一次" icon={RefreshCw} disabled={!s||s.cleanupPending} variant="quiet" onClick={()=>void action('monitor.refresh',{actionId:actionId()})}/></Inline></Toolbar>
    {themeError&&<Notice kind="error" action={<Button onClick={()=>void retryTheme()}>重试外观</Button>}>{themeError}</Notice>}
    {error&&!sameDiagnostic(errorRef,s?.diagnosticRef)&&<Notice kind="error">{error}</Notice>}{detailError&&<Notice kind="error" onDismiss={()=>setDetailError('')}>{detailError}</Notice>}
    {!s?(!error&&<Text tone="muted">正在读取监控状态…</Text>):<>
      <div className="monitor-intro"><Text>{s.serverName}</Text><Text size="small" tone="muted">{s.status}</Text>{s.diagnosticRef&&<Button variant="quiet" onClick={()=>void openConnectionDiagnostics(core,s.diagnosticRef!).catch(e=>setDetailError(errorMessage(e)))}>查看连接详情</Button>}</div>
      {s.error&&<Notice kind="warning">{s.error}</Notice>}
      {s.connection!=='connected'&&<div className="monitor-connect"><Button busy={s.connection==='connecting'} disabled={s.cleanupPending} onClick={()=>void action('monitor.connect',{actionId:actionId()})}>连接并监控</Button></div>}
      <Toolbar className="monitor-options"><SelectField label="采样间隔" value={s.options.refreshSeconds} onChange={event=>options({refreshSeconds:Number(event.target.value) as Options['refreshSeconds']})}><option value="2">2 秒</option><option value="5">5 秒</option><option value="10">10 秒</option></SelectField><SelectField label="趋势范围" value={s.options.rangeSeconds} onChange={event=>options({rangeSeconds:Number(event.target.value) as Options['rangeSeconds']})}><option value="60">1 分钟</option><option value="300">5 分钟</option><option value="600">10 分钟</option></SelectField></Toolbar>
      <ScrollArea className="monitor-content"><Stack gap="md">
        {s.partial&&<Text tone="muted" size="small">部分指标不可用或对象数量超过上限，以下只显示已取得的数据。</Text>}
        {s.historyReleased&&<Text tone="muted" size="small">此前隐藏期间的历史缓存已释放。</Text>}
        <Card title="CPU" value={percent(s.cpu.value?.percent)} detail={<PropertyList><dt>逻辑 CPU</dt><dd>{s.cpu.value?.cores??'—'}</dd><dt>I/O 等待</dt><dd>{percent(s.cpu.value?.iowait)}</dd><dt>平均负载 1 / 5 / 15 分钟</dt><dd>{s.cpu.value?.load.map(v=>v.toFixed(2)).join(' / ')||'—'}</dd></PropertyList>}><Age metric={s.cpu} elapsed={elapsed}/><Chart snapshot={s} elapsed={elapsed} keys={['cpu']} labels={['CPU 使用率']} percentage/></Card>
        <Card title="内存" value={percent(s.memory.value?.percent)} detail={<PropertyList><dt>可用</dt><dd>{bytes(s.memory.value?.available)}</dd><dt>计算方式</dt><dd>{s.memory.value?.estimated?'兼容估算':'MemAvailable'}</dd><dt>Swap</dt><dd>{s.memory.value?.swapTotal==='0'?'未配置':`${bytes(s.memory.value?.swapUsed)} / ${bytes(s.memory.value?.swapTotal)}`}</dd></PropertyList>}><Text numeric>{bytes(s.memory.value?.used)} / {bytes(s.memory.value?.total)}</Text><Age metric={s.memory} elapsed={elapsed}/><Chart snapshot={s} elapsed={elapsed} keys={['memory']} labels={['内存使用率']} percentage/></Card>
        <Card title="文件系统" onDetailToggle={expanded=>select('mount',null,{expanded})} value={capacity?.percent===null?'—':capacity?percent(capacity.percent):'—'} detail={<Stack>
          {s.mounts.map(row=><Surface key={row.identity}><Stack gap="xs"><Button variant="quiet" onClick={()=>select('mount',row.id)}>{row.path}</Button><Text size="small" tone="muted">{row.fsType} · {row.source}{row.shared?' · 同文件系统':''}{row.temporary?' · 内存文件系统':''}{row.network?' · 网络文件系统':''}</Text><Text numeric>{bytes(row.capacity.value?.used)} / {bytes(row.capacity.value?.total)}</Text><Age metric={row.capacity} elapsed={elapsed}/>{row.network&&!row.readAllowed&&<Button size="compact" onClick={()=>select('mount',row.id,{readCapacity:true})}>读取容量</Button>}</Stack></Surface>)}
          <Inline><Button size="compact" disabled={s.selection.mountPage===0} onClick={()=>select('mount',null,{page:s.selection.mountPage-1})}>上一页</Button><Text>{s.selection.mountPage+1} / {Math.max(1,Math.ceil(s.mountCount/8))}</Text><Button size="compact" disabled={(s.selection.mountPage+1)*8>=s.mountCount} onClick={()=>select('mount',null,{page:s.selection.mountPage+1})}>下一页</Button></Inline><Text size="small" tone="muted">容量按文件系统分别展示；使用率为已用 /（已用 + 当前账号可用）。网络文件系统按需读取。</Text>
        </Stack>}><Text>{mount?.path??'等待挂载信息'}</Text><Text numeric>{bytes(capacity?.used)} / {bytes(capacity?.total)}</Text>{pressure&&<Badge tone={pressure.level==='critical'?'error':pressure.level==='warning'?'warning':'neutral'}>{pressure.description}</Badge>}{mount&&<Age metric={mount.capacity} elapsed={elapsed}/>}</Card>
        <Card title="磁盘 I/O" value={disk?.name??'—'} detail={<Stack><SelectField label="块设备" value={s.selection.disk??''} onChange={event=>select('disk',event.target.value)}>{(s.disk.value??[]).map(row=><option value={row.id} key={row.id}>{row.name}{row.partition?' · 分区':''}{row.weakIdentity?' · 名称识别':''}</option>)}</SelectField>{pages('disk')}<Text size="small" tone="muted">按块设备展示读写，不与分区或映射设备相加；不推断与上方文件系统的对应关系。</Text></Stack>}><Text numeric>读取 {rate(disk?.first)} · 写入 {rate(disk?.second)}</Text><Age metric={s.disk} elapsed={elapsed}/><Chart snapshot={s} elapsed={elapsed} keys={['diskFirst','diskSecond']} labels={['读取','写入']} identity={s.selection.disk}/></Card>
        <Card title="网络" value={network?.name??'—'} detail={<Stack><Checkbox label="显示虚拟接口" checked={s.options.showVirtual} onChange={event=>options({showVirtual:event.target.checked})}/><Checkbox label="显示回环接口" checked={s.options.showLoopback} onChange={event=>options({showLoopback:event.target.checked})}/><SelectField label="网络接口" value={s.selection.network??''} onChange={event=>select('network',event.target.value)}>{(s.network.value??[]).map(row=><option value={row.id} key={row.id}>{row.name}{row.virtual?' · 虚拟':''}{row.weakIdentity?' · 名称识别':''}</option>)}</SelectField>{pages('network')}<Text size="small" tone="muted">只显示所选接口的 RX / TX；不汇总网桥、虚拟接口与物理网卡。</Text></Stack>}><Text numeric>接收 {rate(network?.first)} · 发送 {rate(network?.second)}</Text><Age metric={s.network} elapsed={elapsed}/><Chart snapshot={s} elapsed={elapsed} keys={['networkFirst','networkSecond']} labels={['接收','发送']} identity={s.selection.network}/></Card>
        <details><summary>主机信息</summary><PropertyList><dt>主机名</dt><dd>{s.system.host||'—'}</dd><dt>系统 / 内核</dt><dd>{s.system.os} {s.system.kernel}</dd><dt>运行时间</dt><dd>{s.system.uptime===null?'—':`${Math.floor(s.system.uptime/86400)} 天 ${Math.floor(s.system.uptime%86400/3600)} 小时`}</dd><dt>采集范围</dt><dd>当前 Tab 的 SSH 可见环境</dd><dt>插件版本</dt><dd>{__MONITOR_VERSION__}</dd></PropertyList></details>
      </Stack></ScrollArea><StatusBar subtle><Text size="small" tone="muted">{s.options.paused?'采集已暂停':s.connection==='connected'?`每 ${s.options.refreshSeconds} 秒采样`:'等待连接'} · 历史仅保留在内存</Text></StatusBar>
    </>}
  </main>;
}
