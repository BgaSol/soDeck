import { useState } from "react";
import { ShieldCheck } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import type { CoreError } from "@sodeck/sdk";
import type { Server } from "./types";
import { Dialog } from "./dialog";

export function AuthDialog({ server, fault, onClose, onReady }: { server: Server; fault: CoreError; onClose(): void; onReady(): void }) {
  const [password, setPassword] = useState("");
  const [privateKeyPath, setPrivateKeyPath] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const hostKey = ["HOST_KEY_REQUIRED", "HOST_KEY_CHANGED"].includes(fault.code);
  const changed = fault.code === "HOST_KEY_CHANGED";
  const details = (fault.details ?? {}) as { fingerprint?: string; previousFingerprint?: string; algorithm?: string };
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); if (busy) return; setBusy(true); setError("");
    try {
      if (hostKey) await core.call("hosts.trust", { serverId: server.id, fingerprint: details.fingerprint, algorithm: details.algorithm });
      else await core.call("auth.set", { serverId: server.id, remember, ...(server.authType === "key" ? { privateKeyPath, passphrase } : { password }) });
      setPassword(""); setPassphrase(""); onReady();
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  return <Dialog title={hostKey ? (changed ? "服务器身份发生变化" : "确认服务器身份") : "连接认证"} subtitle={`${server.username}@${server.host}:${server.port}`} onClose={() => { if (!busy) onClose(); }}><form onSubmit={event => void submit(event)}>
    {hostKey ? <><div className={`trust-heading ${changed ? "changed" : ""}`}><ShieldCheck size={24} /><p>{changed ? "此服务器的密钥与上次保存的不同。核实新的指纹后，才能更新信任并连接。" : "首次连接此服务器。请核对指纹与服务器提供的信息是否一致。"}</p></div><dl className="fingerprint"><dt>算法</dt><dd>{details.algorithm}</dd>{changed && <><dt>原指纹</dt><dd>{details.previousFingerprint}</dd></>}<dt>当前指纹</dt><dd>{details.fingerprint}</dd></dl></> : <>
      {server.authType === "key" ? <><label>私钥文件路径<input value={privateKeyPath} onChange={e => setPrivateKeyPath(e.target.value)} placeholder="C:\Users\...\.ssh\id_ed25519" required autoComplete="off" /></label><label>私钥口令<input type="password" value={passphrase} onChange={e => setPassphrase(e.target.value)} autoComplete="off" placeholder="未加密的私钥可留空" /></label></> : <label>密码<input type="password" value={password} onChange={e => setPassword(e.target.value)} autoComplete="off" required /></label>}
      <label className="check-label"><input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />在此设备上记住凭据</label><p className="form-help">未勾选时只在本次程序运行期间使用。</p>
    </>}
    {error && <p className="inline-error" role="alert">{error}</p>}<footer><button type="button" onClick={onClose} disabled={busy}>取消</button><button className="primary" disabled={busy}>{busy ? "处理中…" : hostKey ? (changed ? "更新信任并连接" : "信任并连接") : "连接"}</button></footer>
  </form></Dialog>;
}
