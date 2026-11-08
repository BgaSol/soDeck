import {SharedState} from './state.js';
const source=p=>JSON.stringify([p.pluginId,p.pluginInstanceId,p.viewId]);
const fresh=v=>Number.isSafeInteger(v?.observedAt)&&v.observedAt<=Date.now()+5000&&Number.isSafeInteger(v.staleAfterMs)&&v.staleAfterMs>0&&Date.now()-v.observedAt<=v.staleAfterMs;
export function capacityBytes(value){if(typeof value!=='string'||!/^\d{1,20}$/.test(value))return '未知';const n=BigInt(value);for(const [unit,scale] of [['TiB',1099511627776n],['GiB',1073741824n],['MiB',1048576n],['KiB',1024n]])if(n>=scale)return `${n/scale}.${n%scale*10n/scale} ${unit}`;return `${n} B`;}
export function connectFileState(tree,mount,data,save){
  const client=new SharedState(mount);let projection,enabled=mount.settings?.values?.followTerminal!==false,visible=mount.visible!==false,disposed=false,handled='',lastActive='',epoch=0,resolving=0;
  const offsets=new Map(),mapped=new Map(),queue=[];let provider='',followJob;
  const cancel=()=>{if(followJob)tree.cancelFollow();followJob=null;};
  const toggle=document.createElement('button');toggle.className='sd-control follow-terminal';toggle.textContent='跟随终端';toggle.title='跟随活动终端报告的目录';tree.toolbar.append(toggle);
  const providers=document.createElement('select');providers.className='mount-provider';providers.setAttribute('aria-label','挂载信息来源');providers.hidden=true;tree.toolbar.append(providers);
  const updateToggle=()=>{toggle.setAttribute('aria-pressed',String(enabled));toggle.disabled=!client.available||mount.capabilities?.fileLocation!==1;};updateToggle();
  toggle.onclick=async()=>{const next=!enabled;toggle.disabled=true;try{await save(next);}finally{updateToggle();}};
  providers.onchange=()=>{provider=providers.value;++epoch;queue.length=0;mapped.clear();update();};
  tree.onManual=()=>{followJob=null;handled=candidateKey();};
  function records(){return projection?.records||[];}
  function candidate(){
    if(!projection||projection.scope.state!=='active')return null;
    const active=records().find(r=>r.topic==='workspace.presentation.v1'&&r.producer.kind==='core'&&r.key==='current')?.value?.activeEditorViewId;
    if(active!==lastActive){lastActive=active;handled='';}
    const r=records().find(r=>r.topic==='terminal.location.v1'&&r.key==='current'&&r.producer.pluginId==='sodeck.terminal'&&r.producer.viewId===active);
    if(!r||r.value?.state!=='known'||typeof r.value.path!=='string'||!/^\d{1,20}$/.test(r.value.outputOffset)||r.value.filesystemView!=='session-default')return null;
    const key=source(r.producer),offset=BigInt(r.value.outputOffset),previous=offsets.get(key);if(previous!==undefined&&offset<previous)return null;offsets.set(key,offset);
    return {key:JSON.stringify([active,r.value.terminalId,r.value.pathStyle,r.value.path]),value:r.value};
  }
  function candidateKey(){return candidate()?.key||'';}
  function update(){
    if(disposed)return;
    if(!watch?.valid)projection=null;
    const next=candidate();
    if(!enabled||!visible||!next){cancel();if(!next)handled='';}
    else if(next.key!==handled){handled=next.key;const job=tree.followLocation(next.value);followJob=job;void job.catch(error=>{if(error.code!=='cancelled')tree.message('无法跟随终端目录：'+error.message,true);}).finally(()=>{if(followJob===job)followJob=null;});}
    decorate();
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
  const watch=client.available?client.watch({topics:['workspace.presentation.v1','terminal.location.v1','filesystem.mounts.v1','filesystem.capacity.v1']},value=>{projection=value;update();}):null;
  const timer=setInterval(()=>{if(!watch?.valid){projection=null;update();}else decorate();},1000);
  return {settings(snapshot){const next=snapshot?.values?.followTerminal!==false;if(next!==enabled){enabled=next;handled='';cancel();}updateToggle();update();},visibility(value){visible=value;handled='';cancel();update();},invalidate(){++epoch;projection=null;mapped.clear();queue.length=0;cancel();watch?.dispose();update();},dispose(){disposed=true;++epoch;clearInterval(timer);tree.onManual=null;cancel();client.dispose();toggle.remove();providers.remove();}};
}
