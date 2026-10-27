import {useState} from 'react';
import {Button,CodeValue,Disclosure,Icon,Inline,Notice,PropertyList,SelectField,Stack,StatusLabel,Text,TextField} from '@sodeck/ui';
import {ArrowLeftRight,Copy,Globe,Play,Square,Pencil} from 'lucide-react';
import {core,errorMessage} from '@sodeck/sdk';
import type {Snapshot} from './manager';
import {formatTarget} from './protocol/address';
import {Path} from './path';
import type {Diagnostic} from './run';
export const labels={stopped:'已停止',starting:'正在连接',ready:'就绪',reconnecting:'正在重连','needs-attention':'需要处理',stopping:'正在停止',failed:'启动失败'};
export const bytes=(n:number)=>n<1024?`${n} B`:n<1048576?`${(n/1024).toFixed(1)} KiB`:`${(n/1048576).toFixed(1)} MiB`;
export function Details({item,act,start,edit,remove,invoke}:{item:NonNullable<Snapshot['selected']>;act(command:string,args?:Record<string,unknown>):Promise<unknown>;start(id:string,restart?:boolean,reconnect?:boolean):Promise<void>;edit():void;remove():void;invoke<T>(command:string,args?:Record<string,unknown>):Promise<T>}) {
  const {rule,run}=item,[host,setHost]=useState(''),[port,setPort]=useState('80'),[protocol,setProtocol]=useState('http'),[copied,setCopied]=useState(''),[diagnostics,setDiagnostics]=useState<Diagnostic[]>([]),[count,setCount]=useState(0),[error,setError]=useState('');
  const running=run&&!['stopped','failed'].includes(run.status),ready=run?.status==='ready'&&run.address,address=ready?formatTarget(ready):rule.bindMode==='auto'?'启动后自动分配':`127.0.0.1:${rule.port}`;
  const active=run?.rule??rule;
  const perform=async(action:()=>Promise<unknown>)=>{setError('');try{await action();}catch(e){setError(errorMessage(e));}};
  const copy=(text:string)=>perform(async()=>{await core.call('clipboard.writeText',{text});setCopied('已复制');});
  const loadDiagnostics=(start=0)=>perform(async()=>{const page=await invoke<{items:Diagnostic[];total:number}>('network.diagnostics',{id:rule.id,start});setDiagnostics(start?old=>[...old,...page.items]:page.items);setCount(page.total);});
  return <Stack gap="md" className="network-details">
    {error&&<Notice kind="error" onDismiss={()=>setError('')}>{error}</Notice>}
    <Stack gap="xs"><Inline className="network-detail-heading"><Icon icon={rule.type==='port-forward'?ArrowLeftRight:Globe}/><Text size="title">{rule.name}</Text></Inline><Text size="small" tone="muted">{rule.type==='port-forward'?'端口转发':'HTTP / SOCKS5 代理'} · 仅本机可访问</Text></Stack>
    <Inline className="network-detail-actions"><StatusLabel tone={ready?'success':run?.error?'warning':'neutral'}>{labels[run?.status??'stopped']}</StatusLabel><Inline gap="xs">
      <Button size="compact" variant="quiet" icon={Pencil} onClick={edit}>编辑</Button>
      {run?.status==='needs-attention'?<><Button size="compact" onClick={()=>void perform(()=>act('network.stop',{id:rule.id}))}>停止</Button><Button size="compact" variant="primary" onClick={()=>void start(rule.id,false,true)}>重新连接</Button></>:<Button size="compact" variant={running?'secondary':'primary'} icon={running?Square:Play} disabled={run?.status==='stopping'} onClick={()=>void (running?perform(()=>act('network.stop',{id:rule.id})):start(rule.id))}>{run?.status==='starting'?'取消连接':running?'停止':'启动'}</Button>}
    </Inline></Inline>
    {run?.error&&<Notice kind={run.status==='ready'?'status':'warning'}>{run.error.message}（{run.error.code}）{run.commandId&&run.status==='needs-attention'&&['AUTH_REQUIRED','AUTH_FAILED','HOST_KEY_REQUIRED','HOST_KEY_CHANGED'].includes(run.error.code)&&<Button onClick={()=>void perform(()=>core.call('commands.retry',{operationId:run.commandId}))}>继续认证</Button>}</Notice>}
    {run&&rule.revision!==run.rule.revision&&<Notice action={running&&<Button size="compact" onClick={()=>void start(rule.id,true)}>应用并重启</Button>}>新配置将在下次启动生效。现在应用会断开此规则的连接。</Notice>}
    <CodeValue outlined label="本地入口" value={address} caption={active.type==='port-forward'?`转发至 ${formatTarget({host:active.targetHost,port:active.targetPort})}`:'HTTP 与 SOCKS5 共用此入口'} action={<Button size="compact" variant="quiet" icon={Copy} disabled={!ready} onClick={()=>void copy(address)} aria-label="复制地址">{copied||'复制'}</Button>}/>
    <PropertyList><dt>活动连接</dt><dd>{run?.connections??0}</dd><dt>发送 / 接收</dt><dd>{bytes(run?.bytesRead??0)} / {bytes(run?.bytesWritten??0)}</dd>{!!run?.cleanupPending&&<><dt>清理中</dt><dd>{run.cleanupPending} 项</dd></>}</PropertyList>
    <Stack gap="none">
    <Disclosure title="连接路径"><Stack gap="md"><Path route={run?.path}/><Button size="compact" variant="quiet" onClick={()=>void perform(()=>core.call('ui.servers.edit',{serverId:rule.serverId}))}>编辑服务器连接</Button></Stack></Disclosure>
    <Disclosure title="连接检查"><Stack>
      {rule.type==='network-proxy'&&<><TextField label="测试目标主机" value={host} onChange={e=>setHost(e.target.value)} placeholder="远端可访问的 IP 或域名"/><Inline><TextField label="测试端口" inputMode="numeric" value={port} onChange={e=>setPort(e.target.value)}/><SelectField label="检查协议" value={protocol} onChange={e=>setProtocol(e.target.value)}><option value="http">HTTP CONNECT</option><option value="socks">SOCKS5</option></SelectField></Inline></>}
      <Inline><Button disabled={run?.status!=='ready'||run.check?.state==='checking'} onClick={()=>void perform(()=>act('network.check',{id:rule.id,host,port:Number(port),protocol}))}>{run?.check?.state==='checking'?'正在检查':'检查连接'}</Button><Text role="status">{run?.check?.state==='succeeded'?'目标 TCP 连接成功':run?.check?.state==='failed'?`检查失败 · ${run.check.code}`:''}</Text></Inline>
      <Text tone="muted" size="small">通过实际本地入口检查远端目标 TCP 连接；不验证网页内容、TLS 或业务登录。</Text>
    </Stack></Disclosure>
    {rule.type==='network-proxy'&&<Disclosure title="浏览器与 curl 使用指南"><Stack className="network-guide"><Text>浏览器的代理主机填写 127.0.0.1，端口填写 {ready?ready.port:rule.bindMode==='auto'?'启动后分配的端口':rule.port}。HTTP 和 HTTPS 使用同一入口；确认目标未被“内网不使用代理”等例外绕过。</Text><Text>SOCKS5 客户端使用 socks5h，让最终 SSH 节点解析域名。</Text>{['http','socks5h'].map(scheme=><Stack key={scheme}><code>{`curl.exe --proxy ${scheme}://${address} --noproxy "" http://目标主机:端口/`}</code><Button size="compact" disabled={!ready} onClick={()=>void copy(`curl.exe --proxy ${scheme}://${address} --noproxy "" http://目标主机:端口/`)}>复制 {scheme==='http'?'HTTP':'SOCKS5'} 示例</Button></Stack>)}<Text tone="muted">示例仅供复制，请替换目标地址。停止规则后，请恢复客户端的代理设置，或重新启动规则。SoDeck 不更改系统与浏览器代理。</Text></Stack></Disclosure>}
    <Disclosure title="最近诊断" onOpen={()=>void loadDiagnostics()}><Stack>{diagnostics.map((d,i)=><Text key={i} size="small">{new Date(d.time).toLocaleTimeString()} · {d.stage} · {d.code}{d.target?` · ${formatTarget(d.target)}`:''}</Text>)}{!diagnostics.length&&<Text tone="muted">暂无异常记录</Text>}{diagnostics.length<count&&<Button onClick={()=>void loadDiagnostics(diagnostics.length)}>更多诊断</Button>}</Stack></Disclosure>
    </Stack>
    <Inline><Button size="compact" variant="danger" onClick={remove}>{running?'停止并删除':'删除规则'}</Button></Inline>
  </Stack>;
}
