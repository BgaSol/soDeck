import {useCallback,useEffect,useRef,useState} from 'react';
import {Button,Input,Select} from '@sodeck/ui';
import {RefreshCw} from 'lucide-react';
import {core,errorMessage} from '@sodeck/sdk';
import {Dialog} from './dialog';
type RecordRow={id:string;started:number;caller?:{source?:string;pluginId?:string};method:string;status:string;errorCode?:string};
type Cursor={started:number;id:string};
type CallPage={records?:RecordRow[];calls?:RecordRow[];nextCursor?:Cursor|null;retainedFrom?:number|null};

export function CallRecordsPanel({onClose}:{onClose():void}) {
  const [method,setMethod]=useState(''),[pluginId,setPluginId]=useState(''),[serverId,setServerId]=useState(''),[status,setStatus]=useState('');
  const [rows,setRows]=useState<RecordRow[]>([]),[cursor,setCursor]=useState<Cursor|null>(null),[retainedFrom,setRetainedFrom]=useState<number|null>(null),[loading,setLoading]=useState(false),[error,setError]=useState('');
  const request=useRef(0),inFlight=useRef(false),scroll=useRef<HTMLDivElement>(null),end=useRef<HTMLParagraphElement>(null);
  const load=useCallback(async(next?:Cursor)=>{
    if(next&&inFlight.current)return;const generation=++request.current;inFlight.current=true;setLoading(true);setError('');
    if(!next){setRows([]);setCursor(null);setRetainedFrom(null);if(scroll.current)scroll.current.scrollTop=0;}
    try {
      const result=await core.call<RecordRow[]|CallPage>('calls.list',{limit:100,...(next?{cursor:next}:{}),...(method.trim()?{method:method.trim()}:{}),...(pluginId.trim()?{pluginId:pluginId.trim()}:{}),...(serverId.trim()?{serverId:serverId.trim()}:{}),...(status?{status}:{})});
      if(generation!==request.current)return;
      const records=Array.isArray(result)?result:result.records??result.calls??[];
      setRows(previous=>{const all=next?[...previous,...records]:records;return [...new Map(all.map(row=>[row.id,row])).values()];});
      setCursor(Array.isArray(result)?null:result.nextCursor??null);setRetainedFrom(Array.isArray(result)?null:result.retainedFrom??null);
    }catch(e){if(generation===request.current)setError(errorMessage(e));}
    finally{if(generation===request.current){inFlight.current=false;setLoading(false);}}
  },[method,pluginId,serverId,status]);
  useEffect(()=>{void load();return()=>{request.current++;inFlight.current=false;};},[load]);
  useEffect(()=>{if(!cursor||loading||error||!scroll.current||!end.current)return;let active=true;
    const observer=new IntersectionObserver(entries=>{if(active&&entries.some(e=>e.isIntersecting))void load(cursor);},{root:scroll.current,rootMargin:'0px 0px 120px 0px'});observer.observe(end.current);
    return()=>{active=false;observer.disconnect();};
  },[cursor,loading,error,load]);
  return <Dialog title="核心调用记录" subtitle="仅记录调用来源和结果，终端输入输出不会保存在这里。" onClose={onClose} wide>
    <div className="call-records-panel">
      <div className="call-query"><div className="management-tools"><Button onClick={()=>void load()} disabled={loading}><RefreshCw size={14}/>刷新</Button></div>
        <div className="call-filters"><Input value={method} onChange={e=>setMethod(e.target.value)} placeholder="方法，例如 pty.open" aria-label="方法筛选"/><Input value={pluginId} onChange={e=>setPluginId(e.target.value)} placeholder="插件 ID" aria-label="来源筛选"/><Input value={serverId} onChange={e=>setServerId(e.target.value)} placeholder="服务器 ID" aria-label="服务器筛选"/><Select value={status} onChange={e=>setStatus(e.target.value)} aria-label="结果筛选"><option value="">全部结果</option><option value="succeeded">成功</option><option value="failed">失败</option><option value="running">执行中</option><option value="interrupted">已中断</option></Select></div>
        {error&&<p role="alert" className="inline-error">{error}</p>}
        <p className="call-retention">{retainedFrom===null?'暂无保留记录':`保留起点：${new Date(retainedFrom).toLocaleString()}`} · 最多保留 7 天 / 256 MiB</p>
      </div>
      <div className="call-records-body" ref={scroll} tabIndex={0} role="region" aria-label="调用记录内容">
        <div className="call-table"><table aria-label="核心调用记录" aria-busy={loading}><thead><tr><th>时间</th><th>来源 / 方法</th><th>结果</th></tr></thead><tbody>{rows.map(row=><tr key={row.id}><td>{new Date(row.started).toLocaleTimeString()}</td><td><small>{row.caller?.pluginId??row.caller?.source??'核心'}</small><code>{row.method}</code></td><td><span className={`record-state ${row.status}`}>{row.errorCode??row.status}</span></td></tr>)}</tbody></table></div>
        <p ref={end} className="call-load-status" role="status">{error?(cursor?'较早记录读取失败。':'读取失败，请点击刷新重试。'):loading?'正在读取记录…':!rows.length?'没有符合筛选条件的记录。':cursor?`已显示 ${rows.length} 条，向下滚动查看更多`:`已全部加载，共 ${rows.length} 条`}</p>
        {error&&cursor&&<Button disabled={loading} onClick={()=>void load(cursor)}>重试读取较早记录</Button>}
      </div>
    </div>
  </Dialog>;
}
