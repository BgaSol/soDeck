import {useState} from 'react';
import {core,errorMessage,type Proxy,type Group} from '@sodeck/sdk';
import {Button,Dialog,Inline,ListRow,Notice,Stack,TextField,EmptyState} from '@sodeck/ui';
import type {Controller} from './controller';
import type {Catalog} from './model';
import {GroupEditorFields} from './group-fields';
export function Managers({kind,catalog,controller:c,refresh}:{kind:'proxies'|'groups';catalog:Catalog;controller:Controller;refresh():Promise<void>}){
  const [query,setQuery]=useState(''),[page,setPage]=useState(0),[remove,setRemove]=useState<{proxy?:Proxy;group?:Group;operation:string;revision:string}>(),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const destroy=async()=>{if(!remove||busy)return;setBusy(true);setError('');try{
    if(remove.proxy)await core.call('proxies.catalog.remove',{operationId:remove.operation,proxyId:remove.proxy.id,expectedRevision:remove.proxy.revision});
    else await core.call('serverGroups.delete',{operationId:remove.operation,groupId:remove.group!.id,expectedRevision:remove.group!.revision,expectedCatalogRevision:remove.revision});
    setRemove(undefined);await refresh();
  }catch(e){setError(errorMessage(e));}finally{setBusy(false);}};
  const groups=catalog.groups.filter(g=>g.name.toLowerCase().includes(query.toLowerCase())),proxies=catalog.proxies.filter(({proxy:p})=>(p.name+' '+p.host).toLowerCase().includes(query.toLowerCase()));
  const count=kind==='groups'?groups.length:proxies.length,start=Math.min(page*50,Math.max(0,Math.floor((count-1)/50)*50));
  return <div className="connection-form-frame"><header className="sd-editor-heading"><div><h1>{kind==='groups'?'管理分组':'管理代理'}</h1><p>{kind==='groups'?'删除分组会将服务器移到未分组。':'共享修改会影响之后使用此代理的连接；已有连接保留原路径。'}</p></div><Button disabled={c.store.frozen} onClick={()=>{if(kind==='proxies')c.act(()=>c.store.create('proxy',catalog));else c.act(()=>c.store.startGroup());}}>{kind==='groups'?'新建分组':'新建代理'}</Button></header>
    <div className="connection-fields"><Stack gap="md"><TextField label={kind==='groups'?'搜索分组':'搜索代理'} labelVisibility="sr-only" placeholder={kind==='groups'?'搜索分组名称':'搜索名称或地址'} value={query} onChange={e=>{setQuery(e.target.value);setPage(0);}}/>
    {kind==='groups'&&c.store.groupEdit&&<GroupEditorFields editor={c.store.groupEdit} controller={c} onCancel={()=>{c.store.groupEdit=undefined;c.store.changed();}} onSaved={()=>{c.store.groupEdit=undefined;c.store.changed();void refresh();}}/>}
    {kind==='proxies'?proxies.slice(start,start+50).map(({proxy:p,referenceCount})=><ListRow key={p.id}><div className="connection-manager-row"><div><strong>{p.name}</strong><p>{p.host}:{p.port} · {p.authType==='none'?'无认证':'账号认证'} · {referenceCount} 台服务器引用</p></div><Inline><Button disabled={c.store.frozen} onClick={()=>c.act(()=>c.store.create('proxy',catalog,{proxy:p}))}>编辑</Button><Button variant="quiet" disabled={c.store.frozen} onClick={()=>setRemove({proxy:p,operation:crypto.randomUUID(),revision:catalog.revision})}>删除</Button></Inline></div></ListRow>):groups.slice(start,start+50).map(g=><ListRow key={g.id}><div className="connection-manager-row"><span>{g.name} · {g.serverCount} 台服务器</span><Inline><Button disabled={c.store.frozen} onClick={()=>c.act(()=>c.store.startGroup(g))}>重命名</Button><Button variant="quiet" disabled={c.store.frozen} onClick={()=>setRemove({group:g,operation:crypto.randomUUID(),revision:catalog.revision})}>删除</Button></Inline></div></ListRow>)}
    {!count&&<EmptyState title={kind==='groups'?'还没有分组':'还没有代理'}>新建后可以在连接配置中选择。</EmptyState>}
    {count>50&&<Inline><Button disabled={start===0} onClick={()=>setPage(Math.max(0,page-1))}>上一页</Button><span>{start+1}–{Math.min(count,start+50)} / {count}</span><Button disabled={start+50>=count} onClick={()=>setPage(page+1)}>下一页</Button></Inline>}
    </Stack></div>{remove&&<Dialog title={`删除${remove.proxy?'代理':'分组'}`} onClose={()=>{if(!busy)setRemove(undefined);}}><p>{remove.proxy?`删除“${remove.proxy.name}”。仍有保存配置或运行连接引用时，核心会阻止删除。`:`删除“${remove.group!.name}”，${remove.group!.serverCount} 台服务器将移至未分组。`}</p>{error&&<Notice kind="error">{error}</Notice>}<Inline><Button onClick={()=>setRemove(undefined)} disabled={busy}>取消</Button><Button variant="danger" busy={busy} onClick={()=>void destroy()}>确认删除</Button></Inline></Dialog>}
  </div>;
}
