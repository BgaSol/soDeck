import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Pencil, Plus, TerminalSquare, X } from "lucide-react";
import { core, context, errorMessage, onCoreNotification } from "@sodeck/sdk";

interface Session { sessionId: string; viewId: string; title?: string; name?: string; status: string; serverId: string }
export function SessionList() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [editing, setEditing] = useState<Session | null>(null);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const nameField = useRef<HTMLInputElement>(null);
  const title = draft.trim();
  const validTitle = [...title].length > 0 && [...title].length <= 80 && !/[\u0000-\u001f\u007f]/.test(title);
  useEffect(() => { if (editing) { nameField.current?.focus(); nameField.current?.select(); } }, [editing]);
  const refresh = useCallback(async () => {
    try { setSessions(await core.call<Session[]>("plugin.invoke", { command: "sessions.list", args: { workspaceId: context().workspaceId } })); }
    catch (e) { setError(errorMessage(e)); }
  }, []);
  useEffect(() => {
    void refresh(); void core.call("view.ready", {}).catch(e => setError(errorMessage(e)));
    const off = onCoreNotification(n => { if (n.type === "sessions.changed" || n.type === "workbench.changed") void refresh(); });
    const interval = setInterval(() => void refresh(), 1500);
    return () => { off(); clearInterval(interval); };
  }, [refresh]);
  const open = async () => {
    if (pending) return; setPending(true); setError("");
    try { await core.call("workbench.openTerminal", { serverId: context().serverId }, 130_000); await refresh(); }
    catch (e) { setError(errorMessage(e)); } finally { setPending(false); }
  };
  const finishRename = (sessionId: string) => {
    setEditing(null);
    requestAnimationFrame(() => document.getElementById(`rename-${sessionId}`)?.focus());
  };
  const rename = async () => {
    if (!editing || !validTitle || saving) return;
    const sessionId = editing.sessionId; setSaving(true); setError("");
    try {
      await core.call("plugin.invoke", { command: "sessions.rename", args: { sessionId, title } });
      finishRename(sessionId); await refresh();
    } catch (e) { setError(errorMessage(e)); } finally { setSaving(false); }
  };
  return <section className="session-list" aria-label="终端会话">
    <header><span>会话</span><button className="icon" title="新建终端" aria-label="新建终端" onClick={() => void open()} disabled={pending}><Plus size={16} /></button></header>
    {error && <div className="notice error" role="alert">{error}</div>}
    {sessions.length === 0 ? <div className="session-empty"><TerminalSquare size={26} strokeWidth={1.3} /><p>还没有终端</p><button onClick={() => void open()} disabled={pending}>{pending ? "正在连接…" : "新建终端"}</button></div> :
      <ul>{sessions.map((s, index) => <li key={s.sessionId}>
        {editing?.sessionId === s.sessionId ? <form className="session-rename" onSubmit={event => { event.preventDefault(); void rename(); }}>
          <label htmlFor={`name-${s.sessionId}`}>终端显示名称</label>
          <input ref={nameField} id={`name-${s.sessionId}`} value={draft} maxLength={160} disabled={saving} aria-invalid={!validTitle} aria-describedby={`name-help-${s.sessionId}`} onChange={event => setDraft(event.target.value)} onKeyDown={event => {
            if (event.key === "Enter" && event.nativeEvent.isComposing) { event.preventDefault(); event.stopPropagation(); }
            if (event.key === "Escape" && !saving) { event.preventDefault(); event.stopPropagation(); finishRename(s.sessionId); }
          }} />
          <div className="rename-help" id={`name-help-${s.sessionId}`}>1–80 个字符</div>
          <div className="rename-actions"><button type="button" disabled={saving} onClick={() => finishRename(s.sessionId)}>取消</button><button type="submit" disabled={!validTitle || saving}><Check size={13} />{saving ? "保存中…" : "保存名称"}</button></div>
        </form> : <><button className="session-row" onClick={() => void core.call("workbench.activate", { viewId: s.viewId }).catch(e => setError(errorMessage(e)))}>
          <TerminalSquare size={16} /><span>{s.title ?? s.name ?? `终端 ${index + 1}`}<small>{statusLabel(s.status)}</small></span><i className={`status-dot ${s.status}`} />
        </button>
        <div className="session-actions"><button id={`rename-${s.sessionId}`} className="icon" title="修改显示名称" aria-label={`重命名 ${s.title ?? s.name ?? `终端 ${index + 1}`}`} onClick={() => { setError(""); setEditing(s); setDraft(s.title ?? s.name ?? `终端 ${index + 1}`); }}><Pencil size={13} /></button><button className="icon" title="关闭终端并结束会话" aria-label={`关闭 ${s.title ?? s.name ?? `终端 ${index + 1}`}`} onClick={() => void core.call("workbench.close", { viewId: s.viewId }).then(refresh).catch(e => setError(errorMessage(e)))}><X size={13} /></button></div></>}
      </li>)}</ul>}
    <footer>{context().serverName ?? "当前服务器"}</footer>
  </section>;
}
export function statusLabel(status: string) {
  return ({ active: "已连接", running: "已连接", connected: "已连接", connecting: "连接中", ended: "已结束", closed: "已结束", exited: "会话已结束", disconnected: "连接已断开", failed: "连接失败" } as Record<string, string>)[status] ?? status;
}
