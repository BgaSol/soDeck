import {CallRecordsPanel} from './call-records-panel';
export function ManagementDialog(props:{page:'resources'|'calls';onClose():void}){return props.page==='calls'?<CallRecordsPanel onClose={props.onClose}/>:<GeneralManagementDialog page={props.page} onClose={props.onClose}/>;}
import {Button} from '@sodeck/ui';
import {NetworkResources} from './network-resources';
import {RoutePath,RouteBridges} from './route-path';
import type {DisplayRoute} from './route-client';
import type {ResourcePage} from '@sodeck/sdk';
import { useCallback, useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import { Dialog } from "./dialog";


function GeneralManagementDialog({ page, onClose }: { page: "resources"; onClose(): void }) {
  const [resources, setResources] = useState<{ connections: { connectionId: string; serverId?: string; state?: string }[]; ptys: { ptyId: string; state?: string }[]; leases: unknown[] }>({ connections: [], ptys: [], leases: [] });
  const [routes,setRoutes]=useState<{connectionId:string;generation?:number;failureHop?:number|null;state:string;route:DisplayRoute;holders:string[];resources?:{sessions:number;bridges:number}}[]>([]);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    try {
      if (page === "resources") {
        const result = await core.call<{ connections: { id?: string; connectionId?: string; serverId?: string; state?: string }[]; ptys: { id?: string; ptyId?: string; state?: string }[]; leases: unknown[] }>("resources.list");
        const capabilities=await core.call<{capabilities:string[]}>('capabilities.list');
        if(capabilities?.capabilities?.includes('connections.route.v2')){
          const items:typeof routes=[];let cursor:string|null=null;
          do{const page:ResourcePage=await core.call<ResourcePage>('connections.route.list',{cursor});items.push(...page.connections);cursor=page.nextCursor;}while(cursor);setRoutes(items);
        }
        setResources({ connections: result.connections.map(item => ({ ...item, connectionId: item.connectionId ?? item.id ?? "" })), ptys: result.ptys.map(item => ({ ...item, ptyId: item.ptyId ?? item.id ?? "" })), leases: result.leases });
      }

    } catch (e) { setError(errorMessage(e)); }
  }, [page]);
  useEffect(()=>{void load();},[load]);
  const stop = async (resourceId: string) => {
    try { await core.call("resources.stop", { resourceId }); await load(); } catch (e) { setError(errorMessage(e)); }
  };
  return <Dialog title="运行资源" subtitle="查看和结束核心持有的连接与终端。" onClose={onClose} wide>
    <div className="management-body">
      <div className="management-tools"><Button onClick={() => void load()} ><RefreshCw size={14} />刷新</Button></div>
      {error && <p role="alert" className="inline-error">{error}</p>}
      {page === "resources" && <><div className="resource-stats"><span><strong>{resources.connections.length}</strong>连接</span><span><strong>{resources.ptys.length}</strong>终端</span><span><strong>{resources.leases.length}</strong>使用关系</span></div><ul className="resource-list">{routes.map(r=><li key={r.connectionId}><div><RoutePath route={r.route}/><small>{r.state} · {r.holders.length} 个持有实例{r.resources?` · ${r.resources.sessions} 个 SSH 会话 / ${r.resources.bridges} 段中转`:""}</small><code>{r.holders.join("、")}</code>{r.failureHop!=null&&<p>首先检测到第 {r.failureHop} 段中转断开。</p>}{r.generation!==undefined&&"reference" in r.route&&<RouteBridges connectionId={r.connectionId} generation={r.generation}/>}</div><Button onClick={()=>void stop(r.connectionId)}>结束路径</Button></li>)}{resources.ptys.map(item => <li key={item.ptyId}><div><strong>终端</strong><code>{item.ptyId}</code><small>{item.state}</small></div><Button onClick={() => void stop(item.ptyId)}>结束</Button></li>)}</ul>{!resources.ptys.length && <p className="empty-text">当前没有运行的终端。</p>}</>}
      {page === "resources" && <NetworkResources/>}
    </div>
  </Dialog>;
}
