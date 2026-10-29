import {useEffect,useRef,useState} from 'react';
import {core,errorMessage,formatByteCount,formatTrafficRate,type CommunicationPage} from '@sodeck/sdk';
import type {PageRequest,Row,SummaryRequest} from '@sodeck/sdk/traffic';
import {Button,Select} from '@sodeck/ui';

const sources:Record<string,string>={'@other':'其他来源（明细合并）','sodeck.terminal':'终端','sodeck.files':'文件','sodeck.network-proxy':'网络隧道'};
export function sourceName(row:Row):string {return row.resource?({pty:'终端',sftp:'文件',exec:'远端执行',endpoint:'网络端点',transport:'外层传输',other:'其他资源'}[row.resource.kind]??row.resource.kind):row.pluginId?(sources[row.pluginId]??row.pluginId):'核心';}
const rowIdentity=(row:Row)=>JSON.stringify([row.id,row.resource?.generation??null]);
export function Sources({request,names,onServer,unit='bytes'}:{request:SummaryRequest;names:Record<string,string>;onServer(id:string):void;unit?:'bytes'|'bits'}) {
  const [mode,setMode]=useState<'sources'|'connections'>('sources'),[sort,setSort]=useState<NonNullable<PageRequest['sort']>>('totalRate');
  const [pages,setPages]=useState<CommunicationPage[]>([]),[index,setIndex]=useState(0),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const interaction=useRef({hover:false,focus:false,left:0}),[refresh,setRefresh]=useState(0),[selected,setSelected]=useState<string>();
  const query=JSON.stringify({...request,sort,groupBy:mode==='sources'&&request.target?.kind==='all'?'server':'source'});
  const generation=useRef(0),loading=useRef(false),current=pages[index];
  useEffect(()=>{
    const serial=++generation.current;loading.current=true;setBusy(true);setError('');
    void core.call<CommunicationPage>(`communication.${mode}`,{...JSON.parse(query),limit:50}).then(page=>{if(generation.current===serial){setPages([page]);setIndex(0);}})
      .catch(e=>{if(generation.current===serial)setError(errorMessage(e));}).finally(()=>{if(generation.current===serial){loading.current=false;setBusy(false);}});
    return()=>{generation.current++;};
  },[mode,query,refresh]);
  useEffect(()=>{setPages([]);setIndex(0);setSelected(undefined);},[mode,query]);
  useEffect(()=>{const timer=setInterval(()=>{const p=interaction.current;if(!loading.current&&index===0&&!p.hover&&!p.focus&&Date.now()-p.left>=1000)setRefresh(v=>v+1);},5000);return()=>clearInterval(timer);},[index]);
  const next=async()=>{
    if(!current?.nextCursor||loading.current)return;
    if(pages[index+1]){setIndex(i=>i+1);return;}
    const serial=generation.current;loading.current=true;setBusy(true);setError('');
    try{const page=await core.call<CommunicationPage>(`communication.${mode}`,{...JSON.parse(query),cursor:current.nextCursor,limit:50});if(generation.current===serial){setPages(p=>[...p,page]);setIndex(i=>i+1);}}
    catch(e){if(generation.current===serial)setError(errorMessage(e));}
    finally{if(generation.current===serial){loading.current=false;setBusy(false);}}
  };
  return <section className="monitor-sources" aria-label="通信来源" onPointerEnter={()=>{interaction.current.hover=true;}} onPointerLeave={()=>{interaction.current.hover=false;interaction.current.left=Date.now();}}
    onFocusCapture={()=>{interaction.current.focus=true;}} onBlurCapture={e=>{if(!e.currentTarget.contains(e.relatedTarget)){interaction.current.focus=false;interaction.current.left=Date.now();}}}>
    <div className="monitor-row"><h2>{mode==='connections'?'活动资源':request.target?.kind==='all'?'服务器':'来源'}</h2>
      <Select aria-label="明细类型" value={mode} onChange={e=>setMode(e.target.value as typeof mode)}><option value="sources">汇总</option><option value="connections">活动资源</option></Select>
      <Select aria-label="来源排序" value={sort} onChange={e=>setSort(e.target.value as typeof sort)}><option value="totalRate">当前双向速率</option><option value="sendRate">发送速率</option><option value="receiveRate">接收速率</option><option value="sentBytes">累计发送</option><option value="receivedBytes">累计接收</option></Select>
      <Button size="compact" busy={busy} onClick={()=>setRefresh(v=>v+1)}>刷新</Button>
    </div>
    {current&&<p className="form-help">采样于 {new Date(current.summary.observedAt).toLocaleTimeString()} · 浏览或翻页时固定此份数据</p>}
    {error&&<p className="inline-error" role="alert">{error}</p>}
    <div className="monitor-table-scroll"><table className="monitor-table"><thead><tr><th>来源 / 服务器</th><th>发送速率</th><th>接收速率</th><th>本次发送 / 接收</th><th>状态</th></tr></thead>
      <tbody>{current?.items.map(row=><tr key={rowIdentity(row)} aria-selected={selected===rowIdentity(row)}>
        <th scope="row"><Button size="compact" variant="quiet" onClick={()=>{setSelected(rowIdentity(row));if(request.target?.kind==='all')onServer(row.serverId);}}>{request.target?.kind==='all'?names[row.serverId]??`服务器 ${row.serverId.slice(0,8)}`:sourceName(row)}</Button>{row.resource&&<small>{row.resource.resourceId.slice(0,8)} · {sourceName(row)}</small>}</th>
        <td>{formatTrafficRate(row.sendRate,unit)}</td><td>{formatTrafficRate(row.receiveRate,unit)}</td><td>{formatByteCount(row.sentBytes)} / {formatByteCount(row.receivedBytes)}</td><td>{row.invalid?'统计不可用':row.active?'活动中':'已结束'}</td>
      </tr>)}</tbody></table></div>
    {current&&!current.items.length&&<p className="form-help">当前范围尚无通信数据。连接服务器后将自动采集。</p>}
    {current&&selected&&!current.items.some(row=>rowIdentity(row)===selected)&&<p className="form-help">所选资源已结束或不在这页数据中。</p>}
    {current&&(current.other.resources>0||current.other.sentBytes!=='0'||current.other.receivedBytes!=='0')&&<p>其他或已结束资源：发送 {formatByteCount(current.other.sentBytes)} · 接收 {formatByteCount(current.other.receivedBytes)}</p>}
    <div className="monitor-row"><Button size="compact" disabled={index===0||busy} onClick={()=>setIndex(i=>i-1)}>上一页</Button><span>第 {index+1} 页</span><Button size="compact" disabled={!current?.nextCursor||busy} onClick={()=>void next()}>下一页</Button></div>
  </section>;
}
