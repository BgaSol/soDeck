import {invoke as observedInvoke} from './ui-observation';
import {useCallback,useEffect,useRef,useState} from 'react';
import {ArrowLeft,ArrowLeftRight,Plus,Globe,Play,Square,Copy} from 'lucide-react';
import {Button,Dialog,DialogActions,EmptyState,IconButton,Inline,Notice,ResourceItem,ScrollArea,Stack,StatusBar,StatusLabel,Surface,Text,Toolbar} from '@sodeck/ui';
import {diagnosticReference,sameDiagnostic,openConnectionDiagnostics,type DiagnosticRef,context,core,errorMessage,onCoreNotification,onThemeError,retryTheme,connectionCommandTimeout,openDialog} from '@sodeck/sdk';
import type {Snapshot} from './manager';
import type {Rule} from './rules';
import {Details,labels} from './details';

type SavedView={schema:1;selected?:string;start:number;detail?:boolean;scrollTop?:number};
export function View() {
  const ctx=context(),restore=(ctx.snapshot as SavedView|undefined)?.schema===1?ctx.snapshot as SavedView:undefined;
  const [snapshot,setSnapshot]=useState<Snapshot>(),[selected,setSelected]=useState(restore?.selected),[start,setStart]=useState(restore?.start??0),[detail,setDetail]=useState(restore?.detail??false),[removing,setRemoving]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[themeError,setThemeError]=useState<string>();
  const [errorRef,setErrorRef]=useState<DiagnosticRef>();
  const state=useRef({selected,start,detail,scrollTop:restore?.scrollTop??0});state.current={...state.current,selected,start,detail};
  const live=useRef(false),visible=useRef(true),fetching=useRef(false),timer=useRef<ReturnType<typeof setTimeout>>(undefined),list=useRef<HTMLDivElement>(null),locate=useRef(false),epoch=useRef(0);
  const invoke=useCallback(<T,>(command:string,args:Record<string,unknown>={})=>observedInvoke<T>(command,args),[]);
  const poll=useRef(async()=>{});poll.current=async()=>{
    if(!live.current||!visible.current||fetching.current)return;fetching.current=true;clearTimeout(timer.current);
    const version=epoch.current,requested={selected:state.current.selected,start:state.current.start,locate:locate.current};
    try {const s=await invoke<Snapshot>('network.snapshot',requested);if(live.current&&epoch.current===version){setSnapshot(s);if(requested.locate){locate.current=false;setStart(s.start);requestAnimationFrame(()=>list.current?.querySelector('[aria-pressed="true"]')?.scrollIntoView({block:'nearest'}));}}}
    catch(e){if(live.current)setError(errorMessage(e));setErrorRef(diagnosticReference(e));}finally{fetching.current=false;if(live.current&&visible.current)timer.current=setTimeout(()=>void poll.current(),500);}
  };
  useEffect(()=>onThemeError(setThemeError),[]);
  useEffect(()=>{if(notice){const timeout=setTimeout(()=>setNotice(''),4000);return()=>clearTimeout(timeout);}},[notice]);
  useEffect(()=>{
    let alive=true;const off=onCoreNotification(event=>{
      if(event.type==='view.presentation'){visible.current=(event.payload as {visible:boolean}).visible;if(visible.current)void poll.current();else clearTimeout(timer.current);}
      if(event.type==='view.prepareMove'){visible.current=false;clearTimeout(timer.current);void core.call('view.snapshotChunk',{operationId:(event.payload as {operationId:string}).operationId,index:0,total:1,data:JSON.stringify({schema:1,...state.current})}).catch(e=>setError(errorMessage(e)));}
      if(event.type==='view.moveCancelled'){visible.current=true;void poll.current();}
    });
    void (async()=>{try{const s=await core.call<Snapshot>('view.initialize');if(!alive)return;setSnapshot(s);await core.call('view.ready');const p=await core.call<{visible:boolean}>('views.presentation',{viewId:ctx.viewId});if(!alive)return;visible.current=p.visible;live.current=true;await poll.current();}catch(e){if(alive)setError(errorMessage(e));}})();
    return()=>{alive=false;live.current=false;clearTimeout(timer.current);off();};
  },[]);
  useEffect(()=>{epoch.current++;void poll.current();},[selected,start]);
  useEffect(()=>{if(!detail&&list.current)list.current.scrollTop=state.current.scrollTop;},[detail,snapshot?.total===0]);
  const act=async(command:string,args:Record<string,unknown>={})=>{setError('');try{return await invoke(command,args);}catch(e){setError(errorMessage(e));throw e;}finally{await poll.current();}};
  const launch=async(id:string,restart=false,reconnect=false)=>{
    setError('');try{await core.call('commands.execute',{commandId:reconnect?'network.reconnect':'network.start',viewId:ctx.viewId,operationId:crypto.randomUUID(),args:{id,restart}},await connectionCommandTimeout(core,ctx.serverId,ctx.workspaceId));}
    catch(e){setError(errorMessage(e));}finally{await poll.current();}
  };
  const editRule=async(type:Rule['type']='network-proxy',id?:string)=>{
    if(busy)return;setBusy(true);setError('');let token:string|undefined;
    try{
      const session=await invoke<{editorToken:string}>('network.editor.open',{requestId:crypto.randomUUID(),type,...(id?{id}:{})});token=session.editorToken;
      const result=await openDialog<{ruleId:string;revision:number;action:'save'|'start'|'restart'}>('network.rule-editor',{editorToken:token});
      if(result&&live.current){epoch.current++;state.current.selected=result.ruleId;locate.current=true;setSelected(result.ruleId);setNotice('配置已保存');await poll.current();if(result.action!=='save')await launch(result.ruleId,result.action==='restart');}
    }catch(e){if(live.current)setError(errorMessage(e));}
    finally{if(token)await invoke('network.editor.close',{editorToken:token}).catch(()=>{});if(live.current)setBusy(false);}
  };
  const current=snapshot?.selected??snapshot?.items.find(item=>item.rule.id===selected);
  return <Surface variant="panel" className="network-view" aria-label="网络隧道">
    <Toolbar className="network-toolbar">
      {detail?<Button size="compact" variant="quiet" icon={ArrowLeft} onClick={()=>setDetail(false)}>返回列表</Button>:<><Text>名称</Text><Button size="compact" variant="quiet" icon={Plus} disabled={busy||!snapshot} onClick={()=>void editRule()}>创建</Button></>}
    </Toolbar>
    {themeError&&<Notice kind="error" action={<Button onClick={()=>void retryTheme()}>重试主题同步</Button>}>{themeError}</Notice>}
    {error&&!(detail&&sameDiagnostic(errorRef,current?.run?.error?.diagnosticRef))&&<Notice kind="error" action={errorRef&&<Button onClick={()=>void openConnectionDiagnostics(core,errorRef).catch(e=>{setError(errorMessage(e));setErrorRef(undefined);})}>查看连接详情</Button>} onDismiss={()=>setError('')}>{error}</Notice>}
    {!snapshot?<ScrollArea className="network-empty"><EmptyState compact title={error?'暂时无法读取隧道':'正在读取网络规则'} icon={ArrowLeftRight}>{error&&<Button size="compact" onClick={()=>void poll.current()}>重试读取</Button>}</EmptyState></ScrollArea>:detail?<ScrollArea className="network-detail-scroll">{current?<Details key={current.rule.id} item={current} act={act} start={(id,restart,reconnect)=>launch(id,restart||Boolean(current.run&&!['stopped','failed','needs-attention'].includes(current.run.status)),reconnect)} edit={()=>void editRule(current.rule.type,current.rule.id)} remove={()=>setRemoving(true)} invoke={invoke}/>:<EmptyState compact title="规则已不存在"/>}</ScrollArea>:!snapshot.total?<ScrollArea className="network-empty"><EmptyState compact title="暂无网络隧道" icon={ArrowLeftRight}>从右上角创建内网代理或端口转发。</EmptyState></ScrollArea>:<ScrollArea ref={list} className="network-list" role="region" aria-label="规则列表" onScroll={e=>{state.current.scrollTop=e.currentTarget.scrollTop;}}><Stack gap="none">{snapshot.items.map(({rule,run})=>{
      const running=run&&!['stopped','failed'].includes(run.status),ready=run?.status==='ready'&&run.address,address=ready?`127.0.0.1:${ready.port}`:rule.bindMode==='auto'?'自动分配端口':`配置端口 ${rule.port}`;
      const target=rule.type==='port-forward'?`${rule.targetHost}:${rule.targetPort}`:undefined;
      return <ResourceItem key={rule.id} title={rule.name} description={target?`端口转发 · ${target}`:'HTTP / SOCKS5 代理'} subtitle={target?`${address} → ${target}`:address} icon={rule.type==='port-forward'?ArrowLeftRight:Globe} selected={rule.id===selected} onSelect={()=>{setSelected(rule.id);setDetail(true);}}
        status={<StatusLabel tone={run?.status==='ready'?'success':run?.error?'warning':'neutral'}>{labels[run?.status??'stopped']}</StatusLabel>}
        actions={<>{ready&&<IconButton label={`复制入口 ${rule.name}`} size="compact" variant="quiet" icon={Copy} onClick={()=>void core.call('clipboard.writeText',{text:address}).then(()=>setNotice('入口已复制')).catch(e=>setError(errorMessage(e)))}/>}<IconButton label={`${running?'停止':'启动'} ${rule.name}`} variant="quiet" size="compact" icon={running?Square:Play} disabled={run?.status==='stopping'} onClick={()=>void (running?act('network.stop',{id:rule.id}).catch(()=>{}):launch(rule.id))}/></>}/>;
    })}</Stack>{snapshot.total>20&&<Inline className="network-pagination"><Button size="compact" disabled={start===0} onClick={()=>setStart(Math.max(0,start-20))}>上一页</Button><Text size="small">{snapshot.start+1}–{snapshot.start+snapshot.items.length} / {snapshot.total}</Text><Button size="compact" disabled={(snapshot.nextStart??snapshot.start+snapshot.items.length)>=snapshot.total} onClick={()=>setStart(snapshot.nextStart??snapshot.start+snapshot.items.length)}>下一页</Button></Inline>}</ScrollArea>}
    <StatusBar><span role="status">{notice||(snapshot?`${snapshot.total} 条 · ${snapshot.runningCount} 运行中`:'正在读取')}</span><span>网络隧道 {ctx.pluginVersion??'0.2.2'}</span></StatusBar>
    {removing&&current&&<Dialog title="删除这条规则？" subtitle="如正在运行，将先停止并断开所属连接。" onClose={()=>setRemoving(false)}><DialogActions><Button onClick={()=>setRemoving(false)}>取消</Button><Button variant="danger" onClick={()=>{void act('network.delete',{id:current.rule.id,expectedRevision:current.rule.revision}).then(()=>{setRemoving(false);setSelected(undefined);setDetail(false);}).catch(()=>{});}}>停止并删除</Button></DialogActions></Dialog>}
  </Surface>;
}
