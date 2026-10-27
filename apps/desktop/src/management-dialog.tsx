import {CallRecordsPanel} from './call-records-panel';
export function ManagementDialog(props:{page:'plugins'|'resources'|'calls';onClose():void}){return props.page==='calls'?<CallRecordsPanel onClose={props.onClose}/>:<GeneralManagementDialog page={props.page} onClose={props.onClose}/>;}
import {Button,Input} from '@sodeck/ui';
import {NetworkResources} from './network-resources';
import {RoutePath,RouteBridges} from './route-path';
import type {DisplayRoute} from './route-client';
import type {ResourcePage} from '@sodeck/sdk';
import { useCallback, useEffect, useState } from "react";
import { Package, RefreshCw } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import { Dialog } from "./dialog";
import type { PluginStatus } from "./types";

function GeneralManagementDialog({ page, onClose }: { page: "plugins" | "resources"; onClose(): void }) {
  const [plugins, setPlugins] = useState<PluginStatus[]>([]);
  const [resources, setResources] = useState<{ connections: { connectionId: string; serverId?: string; state?: string }[]; ptys: { ptyId: string; state?: string }[]; leases: unknown[] }>({ connections: [], ptys: [], leases: [] });
  const [routes,setRoutes]=useState<{connectionId:string;generation?:number;failureHop?:number|null;state:string;route:DisplayRoute;holders:string[];resources?:{sessions:number;bridges:number}}[]>([]);
  const [path,setPath]=useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    try {
      if (page === "plugins") { const capabilities=await core.call<{capabilities:string[]}>('capabilities.list'); const result = await core.call<PluginStatus[] | { plugins: PluginStatus[] }>(capabilities?.capabilities?.includes('plugins.metadata.v1')?'plugins.metadata.list':'plugins.list'); setPlugins(Array.isArray(result) ? result : result.plugins); }
      if (page === "resources") {
        const result = await core.call<{ connections: { id?: string; connectionId?: string; serverId?: string; state?: string }[]; ptys: { id?: string; ptyId?: string; state?: string }[]; leases: unknown[] }>("resources.list");
        const capabilities=await core.call<{capabilities:string[]}>('capabilities.list');
        if(capabilities?.capabilities?.includes('connections.route.v2')){
          const items:typeof routes=[];let cursor:string|null=null;
          do{const page:ResourcePage=await core.call<ResourcePage>('connections.route.v2.list',{cursor});items.push(...page.connections);cursor=page.nextCursor;}while(cursor);setRoutes(items);
        }else if(capabilities?.capabilities?.includes('connections.route.v1'))setRoutes((await core.call<{connections:typeof routes}>('connections.routes.list')).connections);
        setResources({ connections: result.connections.map(item => ({ ...item, connectionId: item.connectionId ?? item.id ?? "" })), ptys: result.ptys.map(item => ({ ...item, ptyId: item.ptyId ?? item.id ?? "" })), leases: result.leases });
      }

    } catch (e) { setError(errorMessage(e)); }
  }, [page]);
  useEffect(()=>{void load();},[load]);
  const update = async () => {
    if (!path.trim() || busy) return; setBusy(true); setError(""); setNotice("");
    try { await core.call("plugins.prepareUpdate", { path: path.trim() }); setNotice("更新已准备好，将在退出并重新启动 SoDeck 后生效。"); await load(); }
    catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  const stop = async (resourceId: string) => {
    try { await core.call("resources.stop", { resourceId }); await load(); } catch (e) { setError(errorMessage(e)); }
  };
  return <Dialog title={page === "plugins" ? "插件" : page === "resources" ? "运行资源" : "核心调用记录"} subtitle={page === "plugins" ? "独立更新，重启后统一启用。" : page === "resources" ? "查看和结束核心持有的连接与终端。" : "仅记录调用来源和结果，终端输入输出不会保存在这里。"} onClose={onClose} wide>
    <div className="management-body">
      <div className="management-tools"><Button onClick={() => void load()} ><RefreshCw size={14} />刷新</Button></div>
      {error && <p role="alert" className="inline-error">{error}</p>}{notice && <p role="status" className="inline-success">{notice}</p>}
      {page === "plugins" && <><div className="plugin-cards">{plugins.length ? plugins.map(plugin => <article key={plugin.pluginId}><div className="plugin-icon"><Package size={23} /></div><div><h3>{plugin.displayName ?? plugin.pluginId}</h3><p><code>{plugin.pluginId}</code></p><p>当前版本 {plugin.activeVersion ?? plugin.version ?? "未启用"}</p>{plugin.pendingVersion && <span className="pending-badge">{plugin.pendingDisplayName && plugin.pendingDisplayName !== plugin.displayName ? `${plugin.pendingDisplayName} · ` : ""}{plugin.pendingVersion} · 重启后生效</span>}{plugin.error && <p className="inline-error">{plugin.error}</p>}</div></article>) : <p className="empty-text">尚未加载可用的插件包。核心服务器和资源管理仍可使用。</p>}</div><label>从本地包准备更新<div className="update-input"><Input value={path} onChange={e => setPath(e.target.value)} placeholder=".sodeck-plugin 文件的完整路径" /><Button variant="primary" className="primary" onClick={() => void update()} disabled={!path.trim() || busy}>{busy ? "正在验证…" : "准备更新"}</Button></div></label></>}
      {page === "resources" && <><div className="resource-stats"><span><strong>{resources.connections.length}</strong>连接</span><span><strong>{resources.ptys.length}</strong>终端</span><span><strong>{resources.leases.length}</strong>使用关系</span></div><ul className="resource-list">{routes.map(r=><li key={r.connectionId}><div><RoutePath route={r.route}/><small>{r.state} · {r.holders.length} 个持有实例{r.resources?` · ${r.resources.sessions} 个 SSH 会话 / ${r.resources.bridges} 段中转`:""}</small><code>{r.holders.join("、")}</code>{r.failureHop!=null&&<p>首先检测到第 {r.failureHop} 段中转断开。</p>}{r.generation!==undefined&&"reference" in r.route&&<RouteBridges connectionId={r.connectionId} generation={r.generation}/>}</div><Button onClick={()=>void stop(r.connectionId)}>结束路径</Button></li>)}{resources.ptys.map(item => <li key={item.ptyId}><div><strong>终端</strong><code>{item.ptyId}</code><small>{item.state}</small></div><Button onClick={() => void stop(item.ptyId)}>结束</Button></li>)}</ul>{!resources.ptys.length && <p className="empty-text">当前没有运行的终端。</p>}</>}
      {page === "resources" && <NetworkResources/>}
    </div>
  </Dialog>;
}
