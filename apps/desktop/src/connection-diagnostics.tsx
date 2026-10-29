import {useEffect,useRef,useState} from 'react';
import {Button,Dialog,DialogBody,DialogActions,Notice,Stack,Text} from '@sodeck/ui';
import {core,errorMessage,diagnosticPhaseText,diagnosticState,diagnosticSummary,type DiagnosticRef,type DiagnosticSummary,type DiagnosticPhase,type DiagnosticPhases,type NodePage,type RouteNode} from '@sodeck/sdk';

const endpoint=(n:RouteNode)=>`${n.host.includes(':')?`[${n.host}]`:n.host}:${n.port}`;
function evidence(p:DiagnosticPhase){const e=p.error;return e?[e.operation?`操作：${e.operation==='read'?'读取回复':e.operation==='write'?'发送请求':e.operation}`:'',e.expectedBytes!==undefined?`预期 ${e.expectedBytes} 字节，本片段收到 ${e.receivedBytes??0} 字节`:'',e.osErrorCode!==undefined?`系统错误码 ${e.osErrorCode}`:'',e.sshErrorCode!==undefined?`SSH 错误码 ${e.sshErrorCode}`:''].filter(Boolean).join(' · '):'';}
export function ConnectionDiagnostics({reference,onClose}:{reference:DiagnosticRef;onClose():void}) {
  const [record,setRecord]=useState<DiagnosticSummary>(),[nodes,setNodes]=useState<RouteNode[]>([]),[phases,setPhases]=useState<DiagnosticPhase[]>([]);
  const [error,setError]=useState(''),[copy,setCopy]=useState(''),[limit,setLimit]=useState(64),[refresh,setRefresh]=useState(0);
  const failed=useRef<HTMLDetailsElement>(null),located=useRef(false),key=JSON.stringify(reference);
  useEffect(()=>{located.current=false;setRecord(undefined);setNodes([]);setPhases([]);setError('');setCopy('');setLimit(64);},[key]);
  useEffect(()=>{
    let alive=true,timer:ReturnType<typeof setTimeout>|undefined;
    const load=async()=>{
      try {
        const next=await core.call<DiagnosticSummary>('connections.diagnostics.get',{reference});
        const rows:DiagnosticPhase[]=[];let cursor:string|null=null;
        do{const page:DiagnosticPhases=await core.call('connections.diagnostics.phases',{reference,revision:next.revision,cursor});rows.push(...page.phases);cursor=page.nextCursor;}while(cursor&&alive);
        const route:RouteNode[]=[];cursor=null;
        if(next.route)do{const page:NodePage=await core.call('connections.route.nodes',{reference:next.route.reference,cursor});if(page.offset!==route.length)throw Error('路径页次不一致');route.push(...page.nodes);cursor=page.nextCursor;}while(cursor&&alive);
        if(!alive)return;
        setRecord(next);setPhases(rows);setNodes(route);setError('');
        if(next.state==='running'||next.state==='waiting'||next.state==='succeeded'&&!next.lifecycle)timer=setTimeout(()=>void load(),next.state==='running'?700:2000);
      }catch(e){if(alive){if((e as {code?:string}).code==='DIAGNOSTIC_CHANGED'){timer=setTimeout(()=>void load(),200);}else{setError(errorMessage(e));}}}
    };void load();return()=>{alive=false;clearTimeout(timer);};
  },[key,refresh]);
  useEffect(()=>{if(record&&failed.current&&!located.current){located.current=true;failed.current.scrollIntoView({block:'nearest'});}},[record]);
  const round=record?.round??1,latest=phases.filter(p=>p.round===round),earlier=phases.filter(p=>p.round>0&&p.round<round);
  const failureIndex=record?.error?.stepIndex;
  const renderPhase=(p:DiagnosticPhase)=><li key={p.id}><span>{diagnosticPhaseText(p)}</span><small>{p.durationMs} ms{evidence(p)?` · ${evidence(p)}`:''}</small></li>;
  const report=()=>{
    if(!record)return '';
    const rows=[`SoDeck 连接诊断 · API 6.1`,new Date(record.started).toISOString(),`来源：${record.source} · ${record.purpose}`,`尝试：${record.reference.attemptId}`,diagnosticSummary(record),`耗时：${record.durationMs} ms · 记录：${record.coverage}`,record.reused?'复用已有连接；未重新握手':'',record.lifecycle?`后续状态：${record.lifecycle.state==='closed'?'已关闭':record.lifecycle.stepIndex===null?'连接中断，具体断点未知':`第 ${record.lifecycle.stepIndex+1} 站载体中断`}`:''];
    for(const n of nodes){rows.push(`${n.stepIndex+1}. ${n.kind==='ssh'?'SSH':'SOCKS5'} ${endpoint(n)} · 由 ${n.stepIndex===0?'本机':nodes[n.stepIndex-1]?endpoint(nodes[n.stepIndex-1]):'上一站'}访问`);const list=latest.filter(p=>p.stepIndex===n.stepIndex);rows.push(...(list.length?list.map(p=>`  ${diagnosticPhaseText(p)} · ${p.durationMs} ms ${evidence(p)}`):['  '+(record.reused?'复用已有连接':record.state==='running'||record.state==='waiting'?'等待执行':'未执行')]));}
    for(const p of phases.filter(p=>p.stepIndex===null))rows.push(diagnosticPhaseText(p));
    if(earlier.length){rows.push('此前候选轮次');for(const p of earlier)rows.push(`第 ${p.round} 轮 · ${p.stepIndex===null?'准备':`第 ${p.stepIndex+1} 站`} · ${diagnosticPhaseText(p)}`);}
    return rows.filter(Boolean).join('\n');
  };
  const edit=async()=>{try{const n=nodes.find(n=>n.stepIndex===failureIndex);await core.call('ui.connectionHome.open',{operationId:crypto.randomUUID(),intent:n?.kind==='socks5'?{kind:'editProxy',proxyId:n.proxyId}:{kind:'editServer',serverId:n?.kind==='ssh'?n.serverId:record?.serverId}});onClose();}catch(e){setError(errorMessage(e));}};
  const start=failureIndex!==undefined&&failureIndex>=limit?Math.max(0,failureIndex-4):0;
  return <Dialog title="连接详情" subtitle={record?.route?.targetName??'本次连接'} wide onClose={onClose}><DialogBody><Stack>
    {error&&<Notice kind="error" action={<Button onClick={()=>setRefresh(n=>n+1)}>重新读取</Button>}>{error}</Notice>}
    {!record&&!error&&<p role="status">正在读取连接过程</p>}
    {record&&<><strong role="status">{diagnosticSummary(record)}</strong><Text tone="muted">{record.source} · {record.purpose==='network-data'?'网络池连接':'会话连接'} · {new Date(record.started).toLocaleTimeString()} · {record.durationMs} ms</Text>
      {record.coverage==='partial'&&<Notice kind="warning">部分较早的阶段细节已释放，以下记录不完整。</Notice>}
      {record.lifecycle&&<Notice kind={record.lifecycle.state==='disconnected'?'warning':'status'}>{record.lifecycle.state==='closed'?'该连接后来已关闭。':record.lifecycle.stepIndex===null?'连接后来中断，具体断点未知。':`连接后来在第 ${record.lifecycle.stepIndex+1} 站的载体通道中断。`} 上方保留最初建连结果。</Notice>}
      {record.reused&&<Notice>本次复用已有连接，没有重新执行握手和登录。</Notice>}
      <details open={record.error?.stepIndex===undefined&&record.state==='failed'||undefined}><summary>准备与交付</summary><ol className="diagnostic-phases">{phases.filter(p=>p.stepIndex===null).map(renderPhase)}</ol></details>
      <ul className="diagnostic-nodes">{nodes.slice(start,start+limit).map(n=>{
        const list=latest.filter(p=>p.stepIndex===n.stepIndex),fault=list.some(p=>p.state==='failed'),waiting=list.some(p=>p.state==='waiting');
        const done=list.some(p=>p.stage===(n.kind==='ssh'?'authenticate':'proxyConnect')&&p.state==='succeeded')&&!fault&&!waiting;
        const status=record.reused?'复用已有连接':fault?'失败':waiting?'等待确认':done?'已完成':list.some(p=>p.state==='running')?'进行中':list.some(p=>p.state==='canceled')?'已取消':record.state==='running'||record.state==='waiting'?'等待执行':'未执行';
        return <li key={n.stepId}><details ref={fault?failed:undefined} open={fault||waiting||undefined}><summary>{fault?'×':done?'✓':'—'} {n.stepIndex+1}/{record.route?.stepCount} · {n.kind==='ssh'?'SSH':'SOCKS5'} {endpoint(n)} · {status}</summary><p>由{n.stepIndex===0?'本机':nodes[n.stepIndex-1]?.host??'上一站'}访问{n.host==='127.0.0.1'||n.host==='::1'?'；回环地址指该来源机器自身':''}</p><ol className="diagnostic-phases">{list.map(renderPhase)}</ol>{n.kind==='socks5'&&!list.some(p=>p.stage==='proxyConnect')&&<p>代理连接下一站：{record.state==='running'||record.state==='waiting'?'等待执行':'未执行'}</p>}</details></li>;
      })}</ul>
      {(start>0||nodes.length>start+limit)&&<Button onClick={()=>setLimit(n=>n+64)}>显示更多节点</Button>}
      {record.error&&record.state==='failed'&&<Notice kind="warning">检查建议：{record.error.stage?.startsWith('proxy')?'检查上述访问来源机器上的代理服务、端口及协议设置；SSH 转发通道打开并不能证明 SOCKS5 服务已经可用。':record.error.code==='AUTH_FAILED'?'检查失败节点的用户名与登录凭据。':'根据失败阶段检查对应端点及配置。'} 未取得直接证据时，不能确定具体根因。</Notice>}
      {!!earlier.length&&<details><summary>此前地址候选尝试</summary><ol className="diagnostic-phases">{earlier.map(p=><li key={p.id}>第 {p.round} 轮 · {p.stepIndex===null?'准备':`第 ${p.stepIndex+1} 站`} · {diagnosticPhaseText(p)}</li>)}</ol></details>}
      {record.previous&&<Button variant="quiet" onClick={()=>void core.call('ui.connectionDiagnostics.open',{reference:record.previous}).catch(e=>setError(errorMessage(e)))}>查看{record.continuation?'确认前':'前一次'}尝试</Button>}
      <details><summary>技术信息</summary><p>错误码：{record.error?.code??'无'} · 状态：{diagnosticState[record.state]}</p><p>尝试 {record.reference.attemptId} · 代次 {record.reference.attemptGeneration}</p><p>网络池成员 {record.memberId??'不适用'}</p><p>配置修订 {record.route?.routeRevision??'未形成有效路径'}</p></details>
      <Text tone="muted" size="small">复制报告包含上方主机地址和端口，不包含账号、密码或私钥。</Text>{copy&&<p role="status">{copy}</p>}
    </>}
  </Stack></DialogBody><DialogActions><Button onClick={onClose} data-dialog-default>关闭</Button>{record&&<>{record.state==='failed'&&<Button onClick={()=>void edit()}>编辑失败节点</Button>}<Button variant="primary" onClick={()=>void core.call('clipboard.writeText',{text:report()}).then(()=>setCopy('诊断报告已复制')).catch(e=>setCopy(errorMessage(e)))}>复制诊断报告</Button></>}</DialogActions></Dialog>;
}
