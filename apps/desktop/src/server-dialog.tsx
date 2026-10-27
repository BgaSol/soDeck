import {serverPath} from './route-client';
import {Input,InputGroup,Select,Button} from '@sodeck/ui';
import {GroupEditor} from './server-groups';
import {useServerGroups,saveCatalog,catalogServer} from './server-catalog-client';
import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { core, CoreError,errorMessage,type CatalogSave } from "@sodeck/sdk";
import type { ConnectionRoute } from "@sodeck/sdk";
import { Dialog } from "./dialog";
import type { Server } from "./types";

interface Props { server?: Server; servers?: Server[]; routesSupported?: boolean; multihopSupported?: boolean;catalogSupported?:boolean; onClose(): void; onSaved(server: Server): void }
export function ServerDialog({server,servers=[],routesSupported=false,multihopSupported=false,catalogSupported=false,onClose,onSaved}:Props) {
  const [steps,setSteps]=useState<{id:number;added?:Server}[]>([{id:0}]),[busy,setBusy]=useState(false);
  const nextId=useRef(1),focus=useRef(new Map<number,HTMLElement>());
  const [localServers,setLocalServers]=useState(servers);
  useEffect(()=>{setLocalServers(current=>[...servers,...current.filter(s=>!servers.some(v=>v.id===s.id))]);},[servers]);
  const active=steps.at(-1)!.id;
  useEffect(()=>{focus.current.get(active)?.focus();},[active]);
  const close=()=>{if(busy)return;if(steps.length>1)setSteps(v=>v.slice(0,-1));else onClose();};
  return <Dialog className="server-editor-dialog" title={steps.length>1?"添加跳板服务器":server?"编辑服务器":"添加服务器"} subtitle={steps.length>1?"保存后返回上一台服务器，不会打开跳板工作区。":"设置目标地址、连接路径和登录认证。"} onClose={close}>
    {steps.map((step,index)=><div key={step.id} className="server-form-step" hidden={step.id!==active}><ServerForm catalogSupported={catalogSupported} server={index===0?server:undefined} servers={localServers} routesSupported={routesSupported} multihopSupported={multihopSupported} added={step.added} jumpOnly={index>0} onBusy={setBusy} onClose={close} onSaved={value=>{
      if(index===0){onSaved(value);return;}
      setLocalServers(s=>[...s.filter(v=>v.id!==value.id),value]);
      setSteps(previous=>previous.slice(0,-1).map((p,i)=>i===previous.length-2?{...p,added:value}:p));
    }} onAdd={()=>{focus.current.set(active,document.activeElement as HTMLElement);setSteps(v=>[...v,{id:nextId.current++}]);}} /></div>)}
  </Dialog>;
}
function ServerForm({server,servers,routesSupported,multihopSupported=false,catalogSupported=false,added,jumpOnly=false,onAdd,onBusy,onClose,onSaved}:Props & {servers:Server[];routesSupported:boolean;added?:Server;jumpOnly?:boolean;onAdd?():void;onBusy(value:boolean):void}) {
  const {groups,error:groupError,refresh:refreshGroups}=useServerGroups(catalogSupported);
  const [groupId,setGroupId]=useState(server?.groupId??''),[newGroup,setNewGroup]=useState(false),[groupNotice,setGroupNotice]=useState(''),[outcomePending,setOutcomePending]=useState(false);
  const saveRequest=useRef<CatalogSave|undefined>(undefined);
  const [name,setName]=useState(server?.name??""),[host,setHost]=useState(server?.host??""),[port,setPort]=useState(String(server?.port??22));
  const [username,setUsername]=useState(server?.username??""),[authType,setAuthType]=useState<"password"|"key">(server?.authType??"password");
  const [password,setPassword]=useState(""),[remember,setRemember]=useState(true),[showPassword,setShowPassword]=useState(false);
  const [viaJump,setViaJump]=useState(server?.connectionRoute?.kind==="sshJump"),[jumpId,setJumpId]=useState(server?.connectionRoute?.kind==="sshJump"?server.connectionRoute.jumpServerId:"");
  const [query,setQuery]=useState(""),[error,setError]=useState(""),[busy,setBusy]=useState(false),[persisted,setPersisted]=useState(server);
  const persistedRef=useRef(server),saving=useRef(false),alive=useRef(true);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  useEffect(()=>{if(added){setJumpId(added.id);setViaJump(true);setQuery("");}},[added]);
  const candidates=servers.filter(s=>s.id!==server?.id&&(multihopSupported?serverPath(servers,s.id,persisted?.id)!==null:s.connectionRoute?.kind!=="sshJump"));
  const jump=candidates.find(s=>s.id===jumpId);
  const usedAsJump=Boolean(server&&servers.some(s=>s.connectionRoute?.kind==="sshJump"&&s.connectionRoute.jumpServerId===server.id));
  const chain=jump?serverPath(servers,jump.id,persisted?.id):null;
  const dependents=server?servers.filter(s=>s.id!==server.id&&serverPath(servers,s.id)?.some(n=>n.id===server.id)):[];
  const route:ConnectionRoute=viaJump&&(!jumpOnly||multihopSupported)?{kind:"sshJump",jumpServerId:jumpId}:{kind:"direct"};
  const valid=Boolean(name.trim()&&host.trim()&&!/\s|[/\\]/.test(host.trim())&&username.trim()&&/^\d+$/.test(port)&&Number(port)>0&&Number(port)<65536&&(!viaJump||(jumpOnly&&!multihopSupported)||jump));
  const config={name:name.trim(),host:host.trim(),port:Number(port),username:username.trim(),authType,...(routesSupported?{connectionRoute:route}:{})};
  const sameTarget=persisted&&persisted.host===config.host&&persisted.port===config.port&&persisted.username===config.username&&persisted.authType===config.authType&&JSON.stringify(persisted.connectionRoute??{kind:"direct"})===JSON.stringify(route);
  const close=()=>{if(!saving.current){setPassword("");onClose();}};
  const submit=async(event:React.FormEvent)=>{
    event.preventDefault();if(!valid||saving.current)return;saving.current=true;setBusy(true);onBusy(true);setError("");
    let phase:"config"|"credential"="config";
    try {
      let saved=persistedRef.current;
      if(catalogSupported) {
        const changed=!saved||Object.entries(config).some(([key,value])=>JSON.stringify(saved![key as keyof Server])!==JSON.stringify(value));
        if(changed||(saved?.groupId??'')!==groupId||saveRequest.current) {
          saveRequest.current??={operationId:crypto.randomUUID(),...(saved?{serverId:saved.id,expectedServerRevision:saved.revision,expectedMembershipRevision:saved.membershipRevision??'0'}:{}),...(changed?{configuration:{...config,connectionRoute:route}}:{}),groupId:groupId||null};
          const result=await saveCatalog(saveRequest.current);
          if(result.postCommitPending)throw new CoreError({code:'RESULT_UNKNOWN',message:'服务器已保存，凭据清理尚未完成，请重试。'});
          saved=catalogServer(result);saveRequest.current=undefined;setOutcomePending(false);
          persistedRef.current=saved;if(alive.current)setPersisted(saved);
        }
      } else if(!saved||Object.entries(config).some(([key,value])=>JSON.stringify(saved![key as keyof Server])!==JSON.stringify(value))) {
        saved=await core.call<Server>(multihopSupported?(saved?"servers.routes.v2.update":"servers.routes.v2.create"):routesSupported?(saved?"servers.routes.update":"servers.routes.create"):(saved?"servers.update":"servers.create"),{...(saved?{serverId:saved.id,expectedRevision:saved.revision}:{}),...config});
        persistedRef.current=saved;if(alive.current)setPersisted(saved);
      }
      if(!alive.current)return;
      if(!saved)throw new Error('服务器保存结果尚未确认');
      if(authType==="password"&&password.length){phase="credential";const result=await core.call<{remembered:boolean}>("auth.set",{serverId:saved.id,password,remember});saved={...saved,hasSecret:result.remembered};persistedRef.current=saved;}
      if(alive.current){setPassword("");onSaved(saved);}
    }catch(e){if(alive.current){const committed=e instanceof CoreError&&typeof e.details==='object'&&e.details!==null&&'committed' in e.details;const uncertain=e instanceof CoreError&&e.code==='RESULT_UNKNOWN'||committed;setOutcomePending(Boolean(uncertain));if(!uncertain)saveRequest.current=undefined;setError(`${phase==="credential"?"服务器已保存，密码未保存。请重试；取消将保留服务器档案。":uncertain?'保存结果待核实，请重试同一次保存。':''}${errorMessage(e)}`);}}
    finally{saving.current=false;onBusy(false);if(alive.current)setBusy(false);}
  };
  const options=candidates.filter(s=>s.id===jumpId||`${s.name} ${s.host} ${s.username}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  return <><form onSubmit={e=>void submit(e)} onKeyDown={e=>{if(e.key==="Enter"&&e.nativeEvent.isComposing)e.preventDefault();}}>
    <div className="server-fields-scroll"><fieldset disabled={busy||outcomePending} className="server-fields">
      <label>显示名称<Input autoFocus={jumpOnly} autoComplete="off" value={name} maxLength={200} onChange={e=>setName(e.target.value)} placeholder={jumpOnly?"例如：入口机":"例如：开发服务器"} required /></label>
      <div className="form-row host-row"><label>主机地址<Input autoComplete="off" value={host} maxLength={253} onChange={e=>setHost(e.target.value)} placeholder="主机名或 IP 地址" required /></label><label>端口<Input inputMode="numeric" value={port} onChange={e=>setPort(e.target.value)} aria-invalid={Boolean(port)&&(!/^\d+$/.test(port)||Number(port)<1||Number(port)>65535)} required /></label></div>
      {catalogSupported?<div className="server-group-field"><label>分组<Select value={groupId} onChange={e=>setGroupId(e.target.value)}><option value="">未分组</option>{groupId&&!groups.some(g=>g.id===groupId)&&<option value={groupId}>原分组不可用，请重新选择</option>}{groups.map(g=><option key={g.id} value={g.id}>{g.name}</option>)}</Select></label><Button onClick={()=>setNewGroup(true)}>新建分组</Button></div>:<p className="form-help">当前核心不支持服务器分组。</p>}
      {(groupNotice||groupError)&&<p role={groupError?'alert':'status'} className="form-help">{groupError||groupNotice}</p>}
      <label>认证方式<Select value={authType} onChange={e=>{setAuthType(e.target.value as "password"|"key");setPassword("");setShowPassword(false);}}><option value="password">密码</option><option value="key">SSH 私钥</option></Select></label>
      <div className="credentials-row">
      <label>用户名<Input autoComplete="off" value={username} maxLength={200} onChange={e=>setUsername(e.target.value)} placeholder="远程账号" required /></label>
      {authType==="password"&&<div className="server-credentials"><label><span>登录密码 <span className="field-optional">可选</span></span><InputGroup className="password-field"><Input type={showPassword?"text":"password"} autoComplete="new-password" value={password} onChange={e=>setPassword(e.target.value)} placeholder={sameTarget&&persisted.hasSecret?"已保存密码，留空保持":"留空则在连接时输入"}/><Button variant="quiet" size="compact" type="button" className="icon-button" aria-label={showPassword?"隐藏密码":"显示密码"} aria-pressed={showPassword} title={showPassword?"隐藏密码":"显示密码"} onClick={()=>setShowPassword(v=>!v)}>{showPassword?<EyeOff size={17}/>:<Eye size={17}/>}</Button></InputGroup></label><label className="check-label"><Input type="checkbox" checked={remember} onChange={e=>setRemember(e.target.checked)}/>在此设备上记住密码</label><p className="credential-help">{remember?"密码将加密保存在此设备。":"密码仅在本次运行期间使用，退出后不保留。"}{persisted?.hasSecret&&!sameTarget?"连接地址、账号或路径已更改，需要重新提供密码。":""}</p></div>}
      </div>
      {(!jumpOnly||multihopSupported)&&<section className="connection-route-fields" aria-label="连接路径">
        <label>连接方式<Select value={viaJump?"sshJump":"direct"} onChange={e=>setViaJump(e.target.value==="sshJump")}><option value="direct">直接连接</option><option value="sshJump" disabled={!routesSupported||(!multihopSupported&&usedAsJump)}>通过 SSH 跳板</option></Select></label>
        {!routesSupported&&<p className="form-help">当前核心未提供跳板连接能力。</p>}
        {usedAsJump&&!multihopSupported&&<p className="form-help">此服务器正被用作跳板，需要保持直接连接。</p>}
        {viaJump&&<>
          {candidates.length>0&&<><label>搜索跳板<Input value={query} onChange={e=>setQuery(e.target.value)} placeholder="名称、地址或用户名" /></label><label>跳板服务器<Select value={jumpId} onChange={e=>setJumpId(e.target.value)} required><option value="">请选择跳板服务器</option>{options.map(s=><option key={s.id} value={s.id}>{s.name} · {s.username}@{s.host}:{s.port}{multihopSupported&&s.connectionRoute?.kind==="sshJump"?` · 经由 ${(serverPath(servers,s.id)?.length??1)-1} 台跳板`:""}</option>)}</Select></label></>}
          {!candidates.length&&<p className="form-help">还没有可用的跳板服务器，请先添加服务器。</p>}
          <Button type="button" onClick={onAdd}>添加跳板服务器</Button>
          <div className="route-preview"><strong>{chain&&chain.length>3?`经由 ${chain.length} 台跳板 → ${name||"目标服务器"}`:`本机 → ${chain?.map(n=>n.name).join(" → ")??"选择跳板"} → ${name||"目标服务器"}`}</strong><span>每台服务器分别认证，后继地址由上一台跳板访问。</span>{chain&&chain.length>3&&<details><summary>查看完整路径</summary><ol className="route-node-list">{chain.map(n=><li key={n.id}><strong>{n.name}</strong><span>{n.username}@{n.host}:{n.port}</span></li>)}</ol></details>}</div>
        </>}
      </section>}
      {!!dependents.length&&multihopSupported&&<details><summary>此修改影响 {dependents.length} 台依赖服务器的后续连接</summary><ul>{dependents.map(s=><li key={s.id}>{s.name}</li>)}</ul></details>}
      {server&&<p className="form-help">现有会话继续使用原来的连接。新连接使用修改后的配置。</p>}
    </fieldset></div>
    {error&&<p className="inline-error" role="alert">{error}</p>}
    <footer><Button type="button" onClick={close} disabled={busy}>{jumpOnly?"返回上一台服务器":"取消"}</Button><Button variant="primary" type="submit" className="primary" disabled={!valid||busy}>{busy?"正在保存":jumpOnly?"保存并选用":server?"保存服务器":"保存并打开"}</Button></footer>
  </form>{newGroup&&<GroupEditor onClose={()=>setNewGroup(false)} onSaved={group=>{setNewGroup(false);setGroupId(group.id);setGroupNotice("分组已创建");void refreshGroups();}}/>}</>;
}
