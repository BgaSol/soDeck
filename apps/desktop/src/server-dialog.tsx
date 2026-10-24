import { useState } from "react";
import { core, errorMessage } from "@sodeck/sdk";
import { Dialog } from "./dialog";
import type { Server } from "./types";

export function ServerDialog({ server, onClose, onSaved }: { server?: Server; onClose(): void; onSaved(server: Server): void }) {
  const [name, setName] = useState(server?.name ?? "");
  const [host, setHost] = useState(server?.host ?? "");
  const [port, setPort] = useState(String(server?.port ?? 22));
  const [username, setUsername] = useState(server?.username ?? "");
  const [authType, setAuthType] = useState<"password" | "key">(server?.authType ?? "password");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const valid = name.trim() && host.trim() && !/\s|[/\\]/.test(host.trim()) && username.trim() && /^\d+$/.test(port) && Number(port) > 0 && Number(port) < 65536;
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); if (!valid || busy) return; setBusy(true); setError("");
    try {
      const saved = await core.call<Server>(server ? "servers.update" : "servers.create", { ...(server ? { serverId: server.id, expectedRevision: server.revision } : {}), name: name.trim(), host: host.trim(), port: Number(port), username: username.trim(), authType });
      onSaved(saved);
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  return <Dialog title={server ? "编辑服务器" : "添加服务器"} subtitle="先保存连接信息，打开终端时再进行认证。" onClose={() => { if (!busy) onClose(); }}><form onSubmit={event => void submit(event)}>
    <label>显示名称<input autoComplete="off" value={name} maxLength={200} onChange={event => setName(event.target.value)} placeholder="例如：开发服务器" required /></label>
    <div className="form-row host-row"><label>主机地址<input autoComplete="off" value={host} maxLength={253} onChange={event => setHost(event.target.value)} placeholder="主机名或 IP 地址" required /></label><label>端口<input inputMode="numeric" value={port} onChange={event => setPort(event.target.value)} aria-invalid={Boolean(port) && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)} required /></label></div>
    <label>用户名<input autoComplete="off" value={username} maxLength={200} onChange={event => setUsername(event.target.value)} placeholder="远程账号" required /></label>
    <label>认证方式<select value={authType} onChange={event => setAuthType(event.target.value as "password" | "key")}><option value="password">密码</option><option value="key">SSH 私钥</option></select></label>
    {server && <p className="form-help">现有终端继续使用原来的连接。新终端使用修改后的配置。</p>}
    {error && <p className="inline-error" role="alert">{error}</p>}
    <footer><button type="button" onClick={onClose} disabled={busy}>取消</button><button className="primary" disabled={!valid || busy}>{busy ? "正在保存…" : "保存服务器"}</button></footer>
  </form></Dialog>;
}
