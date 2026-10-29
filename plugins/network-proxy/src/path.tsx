import {useEffect,useRef,useState} from 'react';
import {core,errorMessage,type NodePage,type RouteMeta} from '@sodeck/sdk';
import {Button,Notice,Stack,Text} from '@sodeck/ui';
export function Path({route}:{route?:RouteMeta}) {
  const [pages,setPages]=useState<NodePage[]>([]),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const key=JSON.stringify(route?.reference);
  const active=useRef(key);active.current=key;
  useEffect(()=>{let alive=true;setPages([]);setError('');setBusy(false);if(!route)return;
    void core.call<NodePage>('connections.route.nodes',{reference:route.reference}).then(page=>{if(alive)setPages([page]);}).catch(e=>{if(alive)setError(errorMessage(e));});return()=>{alive=false;};
  },[key]);
  const more=async()=>{if(!route||busy)return;setBusy(true);try{const page=await core.call<NodePage>('connections.route.nodes',{reference:route.reference,cursor:pages.at(-1)?.nextCursor});if(active.current===key)setPages(old=>[...old,page]);}catch(e){if(active.current===key)setError(errorMessage(e));}finally{if(active.current===key)setBusy(false);}};
  return <Stack><Text tone="muted">{route&&route.stepCount>1?'经由中转':'直接连接'}{route?` · ${route.sshCount} 个 SSH · ${route.proxyCount} 个 SOCKS5`:''}</Text><Text>本机{pages.flatMap(p=>p.nodes).map(n=>` → ${n.kind==='ssh'?'SSH':'SOCKS5'} ${n.name} (${n.host}:${n.port})`).join('')}{!route?' → 当前服务器':''}</Text>{error&&<Notice kind="warning">{error}</Notice>}{pages.at(-1)?.nextCursor&&<Button busy={busy} onClick={()=>void more()}>继续展开路径</Button>}</Stack>;
}
