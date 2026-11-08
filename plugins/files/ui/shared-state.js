import {SharedState} from './state.js';
const source=p=>JSON.stringify([p.pluginId,p.pluginInstanceId,p.viewId]);
const fresh=v=>Number.isSafeInteger(v?.observedAt)&&v.observedAt<=Date.now()+5000&&Number.isSafeInteger(v.staleAfterMs)&&v.staleAfterMs>0&&Date.now()-v.observedAt<=v.staleAfterMs;
export function terminalProvider(records,active){
  const descriptor=records.find(r=>r.present&&r.topic==='terminal.session.v1'&&r.key==='current'&&r.producer.kind==='plugin'&&r.producer.viewId===active&&r.value?.schemaVersion===1&&r.value.viewId===active);
  if(descriptor){const location=descriptor.value.capabilities?.location;return {session:descriptor.value,location:location?.supported?records.find(r=>r.present&&r.topic==='terminal.location.v1'&&r.key==='current'&&source(r.producer)===source(descriptor.producer)&&r.value?.terminalId===descriptor.value.ptyId)?.value:null};}
  // Legacy providers predate the capability descriptor.
  const legacy=records.find(r=>r.present&&r.topic==='terminal.location.v1'&&r.key==='current'&&r.producer.pluginId==='sodeck.terminal'&&r.producer.viewId===active);
  return legacy?{location:legacy.value}:null;
}
export function capacityBytes(value){if(typeof value!=='string'||!/^\d{1,20}$/.test(value))return '未知';const n=BigInt(value);for(const [unit,scale] of [['TiB',1099511627776n],['GiB',1073741824n],['MiB',1048576n],['KiB',1024n]])if(n>=scale)return `${n/scale}.${n%scale*10n/scale} ${unit}`;return `${n} B`;}
export function connectFileState(tree,mount,data,save){
  const client=new SharedState(mount);let projection,enabled=mount.settings?.values?.followTerminal!==false,visible=mount.visible!==false,disposed=false,handled='',lastActive='',epoch=0,resolving=0;
  const offsets=new Map(),mapped=new Map(),queue=[];let provider='',followJob;
  const cancel=()=>{if(followJob){tree.cancelFollow();handled='';}followJob=null;};
  const toggle=document.createElement('button');toggle.className='sd-control follow-terminal';toggle.textContent='跟随终端';toggle.title='跟随活动终端报告的目录';tree.toolbar.append(toggle);
  const providers=document.createElement('select');providers.className='mount-provider';providers.setAttribute('aria-label','挂载信息来源');providers.hidden=true;tree.toolbar.append(providers);
  const status=document.createElement('details');status.className='follow-status';
  const summary=document.createElement('summary');summary.setAttribute('aria-live','polite');
  const detail=document.createElement('p'),locate=document.createElement('button');locate.className='sd-control';locate.textContent='定位到终端目录';status.append(summary,detail,locate);tree.toolbar.parentElement?.append(status);
  let manual=false,followError='',busySince=0,waitingSince=Date.now();
  locate.onclick=()=>{handled='';manual=false;followError='';update();};
  const updateToggle=()=>{toggle.setAttribute('aria-pressed',String(enabled));toggle.disabled=!client.available||mount.capabilities?.fileLocation!==1;};updateToggle();
  toggle.onclick=async()=>{const next=!enabled;toggle.disabled=true;try{await save(next);}finally{updateToggle();}};
  providers.onchange=()=>{provider=providers.value;++epoch;queue.length=0;mapped.clear();update();};
  tree.onManual=()=>{followJob=null;handled=candidateKey()||handled;manual=true;followError='';update();};
  function records(){return projection?.records||[];}
  const presentation=()=>records().find(r=>r.topic==='workspace.presentation.v1'&&r.producer.kind==='core'&&r.key==='current')?.value;
  const terminal=()=>terminalProvider(records(),presentation()?.activeEditorViewId);
  const current=()=>terminal()?.location;
  function candidate(){
    if(!projection||projection.scope.state!=='active'||projection.scope.connected===false)return null;
    const active=presentation()?.activeEditorViewId;
    if(active!==lastActive){lastActive=active;waitingSince=Date.now();handled='';manual=false;followError='';}
    const selected=terminal();
    const r=records().find(r=>r.topic==='terminal.location.v1'&&r.key==='current'&&r.producer.viewId===active&&r.value===selected?.location);
    if(!r||r.value?.state!=='known'||typeof r.value.path!=='string'||!/^\d{1,20}$/.test(r.value.outputOffset)||!['session-default','shell-reported'].includes(r.value.filesystemView))return null;
    if(r.value.filesystemView==='shell-reported'&&mount.capabilities?.fileLocationProof!==1)return null;
    const key=source(r.producer),offset=BigInt(r.value.outputOffset),previous=offsets.get(key);if(previous!==undefined&&offset<previous)return null;offsets.set(key,offset);
    return {key:JSON.stringify([active,r.value.terminalId,r.value.pathStyle,r.value.path]),value:r.value};
  }
  function candidateKey(){return candidate()?.key||'';}
  function update(){
    if(disposed)return;
    if(!watch?.valid)projection=null;
    const next=candidate();
    if(!enabled||!visible||!next){cancel();}
    else if(next.key!==handled){handled=next.key;manual=false;followError='';const job=tree.followLocation(next.value);followJob=job;void job.catch(error=>{if(followJob===job&&error.code!=='cancelled')followError=error.message;}).finally(()=>{if(followJob===job){followJob=null;feedback();}});}
    feedback();
    decorate();
  }
  function feedback(){
    const v=current(),p=presentation(),next=candidate();
    if(v?.activity==='busy'){if(!busySince)busySince=Date.now();}else busySince=0;
    let title,explanation='仅跟随当前活动终端。手动浏览不会向终端输入命令。';
    if(!enabled)title='手动浏览';
    else if(!client.available||mount.capabilities?.fileLocation!==1||v?.reasonCode==='old-core'||v?.filesystemView==='shell-reported'&&mount.capabilities?.fileLocationProof!==1){title='当前版本不支持自动目录识别';explanation='在设置 → 更新中更新核心、终端和文件插件，然后重启。';}
    else if(!projection||projection.scope.connected===false)title='已暂停：连接已断开';
    else if(!p?.activeEditorViewId||!terminal()&&p?.activeEditorPluginId!=='sodeck.terminal')title='已暂停：当前不是终端';
    else if(terminal()?.session?.capabilities?.location?.supported===false){title='当前终端未提供目录';explanation=terminal().session.capabilities.location.reason||'这个终端会话不提供目录定位，可继续手动浏览。';}
    else if(v?.activity==='busy'){title=Date.now()-busySince<600&&summary.textContent?summary.textContent:'终端忙碌，保留当前位置';explanation='命令仍在运行，或尚未收到外层 shell 的提示符报告。退出嵌套 SSH、容器或子 shell 后自动恢复。';}
    else if(followError){title='无法浏览终端目录';explanation=followError+'。可重试定位，或继续手动浏览。';}
    else if(next){title=manual?'已手动浏览，下次切换目录时跟随':followJob?'正在定位终端目录…':'跟随：'+(v.title||'活动终端');explanation=`${v.path}${v.shell?' · '+v.shell:''}${v.observedAt?' · 最近确认 '+new Date(v.observedAt).toLocaleTimeString():''}`;}
    else if(v?.reason==='终端已退出')title='已暂停：终端已退出';
    else if(!v&&Date.now()-waitingSince>10000){title='当前终端未提供目录';explanation='请在设置 → 更新中检查终端插件，或新建终端重试。';}
    else if(v?.reasonCode==='starting'||!v)title='正在识别终端目录…';
    else {title='当前终端无法自动识别目录';const reasons={'prepare-failed':'无法准备临时集成脚本，已使用普通终端。','unsupported-shell':'自动识别目前支持远端 Linux Bash/Zsh。','shell-probe-failed':'无法识别远端 shell，已使用普通终端。','shell-probe-timeout':'识别远端 shell 超时，已使用普通终端。','hook-conflict':'已有 DEBUG 钩子或调试选项，保留原配置。可使用兼容模式。','handshake-timeout':'未收到启动报告，可能是启动配置禁用了集成。','manual-setup':'兼容模式需要已有 OSC 7 脚本和正确的主机名。','disabled':'终端目录识别已关闭。','invalid-report':'目录报告的路径无效或过长。'};explanation=(reasons[v?.reasonCode]||v?.reason||'当前目录报告不可用。')+' 普通终端和文件浏览仍可使用；修改终端设置后新建终端重试。';}
    if(summary.textContent!==title)summary.textContent=title;detail.textContent=explanation;locate.hidden=!enabled||!next;locate.disabled=!!followJob;
  }
  function decorate(){
    if(!projection){tree.setMounts(new Map());return;}
    const scans=records().filter(r=>r.topic==='filesystem.mounts.v1'&&r.value?.kind==='scanStatus'&&r.producer.kind==='plugin');
    const choices=scans.map(r=>({id:JSON.stringify([r.producer.pluginId,r.producer.viewId]),title:r.producer.pluginId+(scans.filter(s=>s.producer.pluginId===r.producer.pluginId).length>1?' · '+r.producer.viewId:'')})).sort((a,b)=>a.title.localeCompare(b.title));
    providers.hidden=choices.length<2;const selected=provider;providers.replaceChildren();
    for(const choice of [{id:'',title:'选择挂载来源'},...choices]){const option=document.createElement('option');option.value=choice.id;option.textContent=choice.title;providers.append(option);}providers.value=selected;
    if(!provider&&choices.length===1)provider=choices[0].id;
    const candidates=scans.filter(r=>JSON.stringify([r.producer.pluginId,r.producer.viewId])===provider);const scan=candidates.length===1?candidates[0]:null;
    if(!scan){tree.setMounts(new Map());return;}
    const owner=source(scan.producer),mounts=records().filter(r=>r.topic==='filesystem.mounts.v1'&&r.value?.kind==='mount'&&source(r.producer)===owner);
    const liveKeys=new Set(mounts.map(r=>owner+':'+r.value.mountId));for(const key of mapped.keys())if(!liveKeys.has(key))mapped.delete(key);
    const display=new Map();
    for(const record of mounts){const m=record.value,key=owner+':'+m.mountId;if(!mapped.has(key)&&!queue.some(x=>x.key===key)){mapped.set(key,null);queue.push({key,value:m,epoch});}
      const path=mapped.get(key);if(!path)continue;
      const capacity=records().find(r=>r.topic==='filesystem.capacity.v1'&&source(r.producer)===owner&&r.value?.mountId===m.mountId)?.value;
      const current=fresh(m)&&projection.scope.connected;const hasCapacity=current&&fresh(capacity);
      const label=(m.readOnly===true?'只读挂载':'挂载')+(current?'':' · 已过期');
      const detail=[label,m.filesystemType,m.device,scan.value.coverage==='complete'?'完整扫描':'部分扫描',scan.value.publishState==='limited'?'发布缓存超限':scan.value.publishState==='synced'?'':'正在同步',hasCapacity?`可用 ${capacityBytes(capacity.availableBytes)} / ${capacityBytes(capacity.totalBytes)}`:capacity?'容量已过期':'容量未知'].filter(Boolean).join(' · ');
      display.set(path,{label:label+(hasCapacity?' · 可用 '+capacityBytes(capacity.availableBytes):''),detail});
    }
    tree.setMounts(display);pump();
  }
  function pump(){while(!disposed&&visible&&resolving<2&&queue.length){const job=queue.shift();resolving++;void data('resolveLocation',job.value).then(result=>{if(!disposed&&job.epoch===epoch&&mapped.has(job.key))mapped.set(job.key,result.path.replace(/\/$/,'')||'/');}).catch(()=>{}).finally(()=>{resolving--;if(!disposed){decorate();pump();}});}}
  const watch=client.available?client.watch({topics:['workspace.presentation.v1','terminal.session.v1','terminal.location.v1','filesystem.mounts.v1','filesystem.capacity.v1']},value=>{projection=value;update();}):null;
  const timer=setInterval(()=>{if(!watch?.valid){projection=null;update();}else {feedback();decorate();}},1000);
  return {settings(snapshot){const next=snapshot?.values?.followTerminal!==false;if(next!==enabled){enabled=next;handled='';cancel();}updateToggle();update();},visibility(value){visible=value;handled='';cancel();update();},invalidate(){++epoch;projection=null;mapped.clear();queue.length=0;cancel();watch?.dispose();update();},dispose(){disposed=true;++epoch;clearInterval(timer);tree.onManual=null;cancel();client.dispose();toggle.remove();providers.remove();status.remove();}};
}
