import {useEffect,useState} from 'react';
import {core,errorMessage} from '@sodeck/sdk';
import {NetworkClient} from '@sodeck/sdk/network-client';
import type {Resource,ResourcePage,Stopped} from '@sodeck/sdk/network';
import {Button,Inline,Notice,Stack,Text} from '@sodeck/ui';
const names={group:'网络运行',listener:'本地入口',route:'路径租约',transport:'SSH 数据连接',stream:'TCP 通道',splice:'直通传输'};
export function NetworkResources() {
  const [items,setItems]=useState<Resource[]>([]),[cursor,setCursor]=useState<string|null>(null),[error,setError]=useState(''),[notice,setNotice]=useState(''),[busy,setBusy]=useState(false);
  const load=async(next?:string)=>{setBusy(true);setError('');try{const page=await core.call<ResourcePage>('net.resources.list',next?{cursor:next}:{});setItems(old=>next?[...old,...page.items.filter(i=>!old.some(o=>o.handle.id===i.handle.id))]:page.items);setCursor(page.nextCursor);}catch(e){setError(errorMessage(e));}finally{setBusy(false);}};
  useEffect(()=>{void load();},[]);
  const stop=async(item:Resource)=>{setBusy(true);try{const result=await new NetworkClient(core).execute<Stopped>('net.resources.stop',{resource:item.handle});setNotice(`已停止，影响 ${result.affectedGroups.length} 个网络运行；${result.cleanupPending} 项仍在清理。`);await load();}catch(e){setError(errorMessage(e));}finally{setBusy(false);}};
  return <Stack><Inline><h3>网络资源</h3><Button disabled={busy} onClick={()=>void load()}>刷新网络资源</Button></Inline>
    {error&&<Notice kind="error">{error}</Notice>}{notice&&<Notice>{notice}</Notice>}
    {!items.length&&<Text tone="muted">没有网络隧道资源</Text>}
    <ul className="resource-list">{items.map(item=><li key={item.handle.id}><Stack><strong>{names[item.kind]} · {item.state}</strong><Text size="small">{item.address?`${item.address.host}:${item.address.port} · `:''}工作区 {item.scope.workspaceId} · 视图 {item.viewId}</Text><Text size="small">发送 {item.bytesRead} B · 接收 {item.bytesWritten} B · 待清理 {item.cleanupPending}</Text>{item.kind==='transport'&&<Text tone="warning" size="small">强停此共享连接会中断使用它的网络规则；各规则需要重新连接。</Text>}</Stack><Button disabled={busy} onClick={()=>void stop(item)}>{item.kind==='transport'?'强停共享连接':item.kind==='group'?'停止此运行':'停止资源'}</Button></li>)}</ul>
    {cursor&&<Button disabled={busy} onClick={()=>void load(cursor)}>更多网络资源</Button>}
  </Stack>;
}
