import { useCallback, useEffect, useRef, useState } from "react";
import { Package, RefreshCw } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import { Dialog } from "./dialog";
import type { PluginStatus } from "./types";

type RecordRow = { id: string; started: number; caller?: { source?: string; pluginId?: string }; method: string; status: string; errorCode?: string };
type Cursor = { started: number; id: string };
type CallPage = { calls?: RecordRow[]; records?: RecordRow[]; nextCursor?: Cursor | null; retainedFrom?: number | null };
export function ManagementDialog({ page, onClose }: { page: "plugins" | "resources" | "calls"; onClose(): void }) {
  const [plugins, setPlugins] = useState<PluginStatus[]>([]);
  const [resources, setResources] = useState<{ connections: { connectionId: string; serverId?: string; state?: string }[]; ptys: { ptyId: string; state?: string }[]; leases: unknown[] }>({ connections: [], ptys: [], leases: [] });
  const [calls, setCalls] = useState<RecordRow[]>([]);
  const [path, setPath] = useState("");
  const [method, setMethod] = useState("");
  const [pluginId, setPluginId] = useState("");
  const [serverId, setServerId] = useState("");
  const [status, setStatus] = useState("");
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [retainedFrom, setRetainedFrom] = useState<number | null>(null);
  const [loadingCalls, setLoadingCalls] = useState(false);
  const callRequest = useRef(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const loadCalls = useCallback(async (next?: Cursor) => {
    const request = ++callRequest.current;
    setLoadingCalls(true); setError("");
    if (!next) { setCalls([]); setCursor(null); }
    try {
      const result = await core.call<RecordRow[] | CallPage>("calls.list", {
        limit: 100, ...(next ? { cursor: next } : {}),
        ...(method.trim() ? { method: method.trim() } : {}),
        ...(pluginId.trim() ? { pluginId: pluginId.trim() } : {}),
        ...(serverId.trim() ? { serverId: serverId.trim() } : {}), ...(status ? { status } : {}),
      });
      if (request !== callRequest.current) return;
      const records = Array.isArray(result) ? result : result.records ?? result.calls ?? [];
      setCalls(previous => next ? [...previous, ...records] : records);
      setCursor(Array.isArray(result) ? null : result.nextCursor ?? null);
      setRetainedFrom(Array.isArray(result) ? null : result.retainedFrom ?? null);
    } catch (e) { if (request === callRequest.current) setError(errorMessage(e)); }
    finally { if (request === callRequest.current) setLoadingCalls(false); }
  }, [method, pluginId, serverId, status]);
  const load = useCallback(async () => {
    try {
      if (page === "plugins") { const result = await core.call<PluginStatus[] | { plugins: PluginStatus[] }>("plugins.list"); setPlugins(Array.isArray(result) ? result : result.plugins); }
      if (page === "resources") {
        const result = await core.call<{ connections: { id?: string; connectionId?: string; serverId?: string; state?: string }[]; ptys: { id?: string; ptyId?: string; state?: string }[]; leases: unknown[] }>("resources.list");
        setResources({ connections: result.connections.map(item => ({ ...item, connectionId: item.connectionId ?? item.id ?? "" })), ptys: result.ptys.map(item => ({ ...item, ptyId: item.ptyId ?? item.id ?? "" })), leases: result.leases });
      }
      if (page === "calls") await loadCalls();
    } catch (e) { setError(errorMessage(e)); }
  }, [page, loadCalls]);
  useEffect(() => { void load(); return () => { callRequest.current++; }; }, [load]);
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
      <div className="management-tools"><button onClick={() => void load()} disabled={loadingCalls}><RefreshCw size={14} />刷新</button></div>
      {page === "calls" && <div className="call-filters"><input value={method} onChange={event => setMethod(event.target.value)} placeholder="方法，例如 pty.open" aria-label="方法筛选" /><input value={pluginId} onChange={event => setPluginId(event.target.value)} placeholder="插件 ID" aria-label="来源筛选" /><input value={serverId} onChange={event => setServerId(event.target.value)} placeholder="服务器 ID" aria-label="服务器筛选" /><select value={status} onChange={event => setStatus(event.target.value)} aria-label="结果筛选"><option value="">全部结果</option><option value="succeeded">成功</option><option value="failed">失败</option><option value="running">执行中</option><option value="interrupted">已中断</option></select></div>}
      {error && <p role="alert" className="inline-error">{error}</p>}{notice && <p role="status" className="inline-success">{notice}</p>}
      {page === "plugins" && <><div className="plugin-cards">{plugins.length ? plugins.map(plugin => <article key={plugin.pluginId}><div className="plugin-icon"><Package size={23} /></div><div><h3>{plugin.pluginId === "sodeck.terminal" ? "终端" : plugin.pluginId}</h3><p>当前版本 {plugin.activeVersion ?? plugin.version ?? "未启用"}</p>{plugin.pendingVersion && <span className="pending-badge">{plugin.pendingVersion} · 重启后生效</span>}{plugin.error && <p className="inline-error">{plugin.error}</p>}</div></article>) : <p className="empty-text">尚未加载可用的插件包。核心服务器和资源管理仍可使用。</p>}</div><label>从本地包准备更新<div className="update-input"><input value={path} onChange={e => setPath(e.target.value)} placeholder=".sodeck-plugin 文件的完整路径" /><button className="primary" onClick={() => void update()} disabled={!path.trim() || busy}>{busy ? "正在验证…" : "准备更新"}</button></div></label></>}
      {page === "resources" && <><div className="resource-stats"><span><strong>{resources.connections.length}</strong>连接</span><span><strong>{resources.ptys.length}</strong>终端</span><span><strong>{resources.leases.length}</strong>使用关系</span></div><ul className="resource-list">{resources.ptys.map(item => <li key={item.ptyId}><div><strong>终端</strong><code>{item.ptyId}</code><small>{item.state}</small></div><button onClick={() => void stop(item.ptyId)}>结束</button></li>)}</ul>{!resources.ptys.length && <p className="empty-text">当前没有运行的终端。</p>}</>}
      {page === "calls" && <><p className="call-retention">{retainedFrom === null ? "暂无保留记录" : `保留起点：${new Date(retainedFrom).toLocaleString()}`} · 最多保留 7 天 / 256 MiB</p><div className="call-table"><table><thead><tr><th>时间</th><th>来源 / 方法</th><th>结果</th></tr></thead><tbody>{calls.map(item => <tr key={item.id}><td>{new Date(item.started).toLocaleTimeString()}</td><td><small>{item.caller?.pluginId ?? item.caller?.source ?? "核心"}</small><code>{item.method}</code></td><td><span className={`record-state ${item.status}`}>{item.errorCode ?? item.status}</span></td></tr>)}</tbody></table>{!calls.length && <p className="empty-text">{loadingCalls ? "正在读取记录…" : "没有符合筛选条件的记录。"}</p>}</div>{cursor && <button onClick={() => void loadCalls(cursor)} disabled={loadingCalls}>{loadingCalls ? "正在读取…" : "加载更早记录"}</button>}</>}
    </div>
  </Dialog>;
}
