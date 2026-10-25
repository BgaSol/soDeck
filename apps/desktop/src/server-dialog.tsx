import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import { Dialog } from "./dialog";
import type { Server } from "./types";

export function ServerDialog({ server, onClose, onSaved }: { server?: Server; onClose(): void; onSaved(server: Server): void }) {
  const [name, setName] = useState(server?.name ?? "");
  const [host, setHost] = useState(server?.host ?? "");
  const [port, setPort] = useState(String(server?.port ?? 22));
  const [username, setUsername] = useState(server?.username ?? "");
  const [authType, setAuthType] = useState<"password" | "key">(server?.authType ?? "password");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);
  const [showPassword, setShowPassword] = useState(false);
  const [persisted, setPersisted] = useState(server);
  const persistedRef = useRef(server);
  const saving = useRef(false), alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const valid = name.trim() && host.trim() && !/\s|[/\\]/.test(host.trim()) && username.trim() && /^\d+$/.test(port) && Number(port) > 0 && Number(port) < 65536;
  const config = { name: name.trim(), host: host.trim(), port: Number(port), username: username.trim(), authType };
  const sameTarget = persisted && persisted.host === config.host && persisted.port === config.port && persisted.username === config.username && persisted.authType === config.authType;
  const close = () => { if (saving.current) return; setPassword(""); onClose(); };
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); if (!valid || saving.current) return; saving.current = true; setBusy(true); setError("");
    let phase: "config" | "credential" = "config";
    try {
      // Keep the committed ID/revision when credential storage fails. Retrying is
      // an update of that draft, never another servers.create call.
      let saved = persistedRef.current;
      if (!saved || Object.entries(config).some(([key,value]) => saved![key as keyof Server] !== value)) {
        saved = await core.call<Server>(saved ? "servers.update" : "servers.create", { ...(saved ? { serverId: saved.id, expectedRevision: saved.revision } : {}), ...config });
        persistedRef.current = saved;
        if (alive.current) setPersisted(saved);
      }
      if (!alive.current) return;
      if (authType === "password" && password.length) {
        phase = "credential";
        const result = await core.call<{remembered:boolean}>("auth.set", { serverId: saved.id, password, remember });
        saved = { ...saved, hasSecret: result.remembered };
        persistedRef.current = saved;
      }
      if (alive.current) { setPassword(""); onSaved(saved); }
    } catch (e) { if (alive.current) setError(`${phase === "credential" ? "服务器已保存，密码未保存。请重试；取消将保留服务器档案。" : ""}${errorMessage(e)}`); }
    finally { saving.current = false; if (alive.current) setBusy(false); }
  };
  return <Dialog className="server-editor-dialog" title={server ? "编辑服务器" : "添加服务器"} subtitle="设置连接信息，也可以提前保存登录密码。" onClose={close}><form onSubmit={event => void submit(event)} onKeyDown={event => { if (event.key === "Enter" && event.nativeEvent.isComposing) event.preventDefault(); }}>
    <fieldset disabled={busy} className="server-fields">
    <label>显示名称<input autoComplete="off" value={name} maxLength={200} onChange={event => setName(event.target.value)} placeholder="例如：开发服务器" required /></label>
    <div className="form-row host-row"><label>主机地址<input autoComplete="off" value={host} maxLength={253} onChange={event => setHost(event.target.value)} placeholder="主机名或 IP 地址" required /></label><label>端口<input inputMode="numeric" value={port} onChange={event => setPort(event.target.value)} aria-invalid={Boolean(port) && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)} required /></label></div>
    <label>用户名<input autoComplete="off" value={username} maxLength={200} onChange={event => setUsername(event.target.value)} placeholder="远程账号" required /></label>
    <label>认证方式<select value={authType} onChange={event => { setAuthType(event.target.value as "password" | "key"); setPassword(""); setShowPassword(false); }}><option value="password">密码</option><option value="key">SSH 私钥</option></select></label>
    {authType === "password" && <div className="server-credentials">
      <label htmlFor="server-password">登录密码 <span className="field-optional">可选</span></label>
      <div className="password-field"><input id="server-password" type={showPassword ? "text" : "password"} autoComplete="new-password" value={password} onChange={event => setPassword(event.target.value)} placeholder={sameTarget && persisted.hasSecret ? "已保存密码，留空保持" : "留空则在连接时输入"} /><button type="button" className="icon-button" aria-label={showPassword ? "隐藏密码" : "显示密码"} aria-pressed={showPassword} title={showPassword ? "隐藏密码" : "显示密码"} onClick={() => setShowPassword(value => !value)}>{showPassword ? <EyeOff size={17} /> : <Eye size={17} />}</button></div>
      <label className="check-label"><input type="checkbox" checked={remember} onChange={event => setRemember(event.target.checked)} />在此设备上记住密码</label>
      <p className="credential-help">{remember ? "密码将加密保存在此设备。" : "密码仅在本次运行期间使用，退出后不保留。"}{persisted?.hasSecret && !sameTarget ? "连接地址或账号已更改，需要重新提供密码。" : ""}</p>
    </div>}
    {server && <p className="form-help">现有终端继续使用原来的连接。新终端使用修改后的配置。</p>}
    </fieldset>
    {error && <p className="inline-error" role="alert">{error}</p>}
    <footer><button type="button" onClick={close} disabled={busy}>取消</button><button className="primary" disabled={!valid || busy}>{busy ? "正在保存" : "保存服务器"}</button></footer>
  </form></Dialog>;
}
