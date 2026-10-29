import {RoutePath} from './route-path';
import type {DisplayRoute} from './route-client';
import {Input,Button,CodeValue,IdentitySummary,Notice,ScrollArea,Stack,Text} from '@sodeck/ui';
import { useState } from "react";
import { Copy,Check,ShieldCheck,ShieldAlert } from "lucide-react";
import { core, errorMessage } from "@sodeck/sdk";
import type { CoreError } from "@sodeck/sdk";
import type { Server } from "./types";
import { Dialog } from "./dialog";

export function AuthDialog({ server, fault, onClose, onReady }: { server: Server; fault: CoreError; onClose(): void; onReady(): void }) {
  const [password, setPassword] = useState("");
  const [privateKeyPath, setPrivateKeyPath] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [remember, setRemember] = useState(Boolean((fault.details as {rememberDefault?:boolean})?.rememberDefault));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [replacePreset,setReplacePreset]=useState(false);
  const [copied,setCopied]=useState(false),[copyError,setCopyError]=useState('');
  const hostKey = ["HOST_KEY_REQUIRED", "HOST_KEY_CHANGED"].includes(fault.code);
  const changed = fault.code === "HOST_KEY_CHANGED";
  const details = (fault.details ?? {}) as { challengeId?:string; operationId?:string; fingerprint?: string; previousFingerprint?: string; algorithm?: string; role?:string; stepId?:string; stepIndex?:number; stepCount?:number; ownerKind?:string; ownerId?:string; hasPreset?:boolean; proxy?:{name:string;host:string;port:number;username:string}; route?:DisplayRoute };
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); if (busy||(hostKey&&(!details.fingerprint||!details.algorithm))) return; setBusy(true); setError("");
    try {
      if(details.ownerKind!=='socks5')await core.call('auth.ssh.confirm',{challengeId:details.challengeId,operationId:details.operationId??null,trust:hostKey?(changed?'update':'accept'):'keep',credential:!replacePreset&&(details.hasPreset||fault.code==='CREDENTIAL_CONFIRM_REQUIRED')?'preset':'input',remember,...(!replacePreset&&(details.hasPreset||fault.code==='CREDENTIAL_CONFIRM_REQUIRED')?{}:server.authType==='key'?{privateKeyPath,passphrase}:{password})});
      else if(fault.code==='CREDENTIAL_CONFIRM_REQUIRED'&&!replacePreset)await core.call('auth.credentials.usePreset',{challengeId:details.challengeId,operationId:details.operationId,remember});
      else if(details.ownerKind==="socks5") await core.call("auth.socks5.set",{proxyId:details.ownerId,challengeId:details.challengeId,password,remember});
      else if (hostKey) await core.call("hosts.trust", { serverId: server.id, fingerprint: details.fingerprint, challengeId:details.challengeId });
      else await core.call("auth.ssh.set", { serverId: server.id, challengeId:details.challengeId, remember, ...(server.authType === "key" ? { privateKeyPath, passphrase } : { password }) });
      setPassword(""); setPassphrase(""); onReady();
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  if(fault.code==='CREDENTIAL_CONFIRM_REQUIRED'&&!replacePreset)return <Dialog title="将已保存凭据用于此路径" onClose={()=>{if(!busy)onClose();}}><form onSubmit={event=>void submit(event)}><Stack gap="md"><p>第 {(details.stepIndex??0)+1} / {details.stepCount} 站 · {details.proxy?.name??server.name}</p><p>此访问路径尚未使用过该凭据。确认后，核心将它绑定到已经核验的前序路径。</p>{details.route&&<RoutePath route={details.route}/>}<label className="check-label"><Input type="checkbox" checked={remember} onChange={e=>setRemember(e.target.checked)}/>在此设备上记住此路径的凭据</label>{error&&<Notice kind="error">{error}</Notice>}</Stack><footer><Button disabled={busy} onClick={onClose}>取消</Button><Button disabled={busy} onClick={()=>setReplacePreset(true)}>重新输入</Button><Button variant="primary" type="submit" busy={busy}>确认并继续</Button></footer></form></Dialog>;
  if(details.ownerKind==='socks5')return <Dialog title="网络代理认证" subtitle={details.proxy?`${details.proxy.name} · ${details.proxy.host}:${details.proxy.port}`:"SOCKS5 网络代理"} onClose={()=>{if(!busy)onClose();}}><form onSubmit={event=>void submit(event)}><p>仅验证 SOCKS5 代理账号，无需登录代理主机。</p><p>代理用户名：{details.proxy?.username}</p><label>代理密码<Input type="password" autoComplete="off" value={password} onChange={e=>setPassword(e.target.value)} required autoFocus/></label><label className="check-label"><Input type="checkbox" checked={remember} onChange={e=>setRemember(e.target.checked)}/>在此设备上记住代理密码</label>{error&&<p role="alert" className="inline-error">{error}</p>}<footer><Button disabled={busy} onClick={onClose}>取消</Button><Button type="submit" variant="primary" disabled={busy}>继续连接目标</Button></footer></form></Dialog>;
  if(hostKey) {
    const nodes=details.route?details.route.stepCount:details.stepCount??1;
    const address=`${server.host.includes(':')?`[${server.host}]`:server.host}:${server.port}`;
    const copy=async()=>{setCopyError('');try{await core.call('clipboard.writeText',{text:details.fingerprint});setCopied(true);}catch(e){setCopied(false);setCopyError(errorMessage(e));}};
    return <Dialog title={changed?'身份与登录 · 主机密钥已变化':'确认身份与登录'} width={480} className="identity-dialog" onClose={()=>{if(!busy)onClose();}}>
      <form className="identity-form" onSubmit={event=>void submit(event)}>
        <ScrollArea className="identity-content"><Stack gap="lg">
          <IdentitySummary icon={changed?ShieldAlert:ShieldCheck} tone={changed?'warning':'accent'} title={server.name===server.host?address:server.name} detail={server.name===server.host?`登录用户 ${server.username}`:`${server.username}@${address}`}/>
          {changed?<Notice kind="warning">密钥与上次连接不同。请向服务器管理员核实新指纹，确认后再更新信任。</Notice>:<Text tone="muted">首次连接，请核对服务器提供的指纹。</Text>}
          <Stack gap="sm">
            {changed&&<CodeValue label="原指纹" value={details.previousFingerprint??'未提供原指纹'} muted/>}
            <CodeValue label={changed?'新指纹':'服务器指纹'} value={details.fingerprint??'未提供指纹'} caption={`密钥算法 · ${details.algorithm??'未知'}`} action={<Button size="compact" variant="quiet" icon={copied?Check:Copy} aria-label="复制当前指纹" disabled={!details.fingerprint} onClick={()=>void copy()}>{copied?'已复制':'复制'}</Button>}/>
            {copyError&&<Notice kind="error">{copyError}</Notice>}
          </Stack>
          {nodes>1&&details.route&&<Stack gap="xs"><Text size="small" tone="muted">正在核对{details.role==='jump'?'跳板':'目标'} · 第 {(details.stepIndex??0)+1} / {nodes} 站</Text><RoutePath route={details.route} compact/></Stack>}
          <Stack gap="sm"><strong>登录方式 · {server.authType==='key'?'SSH 私钥':'密码'}</strong>
            {details.hasPreset&&!replacePreset?<><Text>使用此连接已保存的凭据。信任并连接将同时允许当前路径使用该凭据。</Text><Button variant="quiet" disabled={busy} onClick={()=>setReplacePreset(true)}>重新输入</Button></>:server.authType==='key'?<><label>私钥文件路径<Input value={privateKeyPath} onChange={e=>setPrivateKeyPath(e.target.value)} autoComplete="off" required/></label><label>私钥口令<Input type="password" value={passphrase} onChange={e=>setPassphrase(e.target.value)} autoComplete="off"/></label></>:<label>登录密码<Input type="password" value={password} onChange={e=>setPassword(e.target.value)} autoComplete="off" required/></label>}
            <label className="check-label"><Input type="checkbox" checked={remember} onChange={e=>setRemember(e.target.checked)}/>在此设备上记住此路径的凭据</label><Text tone="muted" size="small">未勾选时仅在本次程序运行期间使用。</Text>
          </Stack>
          {error&&<Notice kind="error">{error}</Notice>}
        </Stack></ScrollArea>
        <footer><Button onClick={onClose} disabled={busy}>取消</Button><Button variant={changed?'danger':'primary'} type="submit" busy={busy} disabled={!details.fingerprint||!details.algorithm}>{busy?'正在确认':changed?'更新信任并连接':'信任并连接'}</Button></footer>
      </form>
    </Dialog>;
  }
  return <Dialog title="连接认证" subtitle={`${server.username}@${server.host}:${server.port}`} onClose={() => { if (!busy) onClose(); }}><form onSubmit={event => void submit(event)}>
    {details.route && <p className="route-preview"><strong>{details.role==="jump"?"跳板":"目标"} {(details.stepIndex??0)+1}/{details.stepCount}</strong><span>最终目标：{details.route.targetName}</span></p>}
    {details.route&&<RoutePath route={details.route}/>}
    <>
      {server.authType === "key" ? <><label>私钥文件路径<Input value={privateKeyPath} onChange={e => setPrivateKeyPath(e.target.value)} placeholder="C:\Users\...\.ssh\id_ed25519" required autoComplete="off" /></label><label>私钥口令<Input type="password" value={passphrase} onChange={e => setPassphrase(e.target.value)} autoComplete="off" placeholder="未加密的私钥可留空" /></label></> : <label>密码<Input type="password" value={password} onChange={e => setPassword(e.target.value)} autoComplete="off" required /></label>}
      <label className="check-label"><Input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />在此设备上记住凭据</label><p className="form-help">未勾选时只在本次程序运行期间使用。</p>
    </>
    {error && <p className="inline-error" role="alert">{error}</p>}<footer><Button type="button" onClick={onClose} disabled={busy}>取消</Button><Button variant="primary" type="submit" className="primary" disabled={busy}>{busy ? "处理中…" : "连接"}</Button></footer>
  </form></Dialog>;
}
