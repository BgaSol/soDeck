const source=p=>JSON.stringify([p.pluginId,p.pluginInstanceId,p.viewId]);
const fresh=v=>Number.isSafeInteger(v?.observedAt)&&v.observedAt<=Date.now()+5000&&Number.isSafeInteger(v.staleAfterMs)&&v.staleAfterMs>0&&Date.now()-v.observedAt<=v.staleAfterMs;
export function capacityBytes(value){if(typeof value!=='string'||!/^\d{1,20}$/.test(value))return '未知';const n=BigInt(value);for(const [unit,scale] of [['TiB',1099511627776n],['GiB',1073741824n],['MiB',1048576n],['KiB',1024n]])if(n>=scale)return `${n/scale}.${n%scale*10n/scale} ${unit}`;return `${n} B`;}
export function connectMounts(tree,mount,{client,data}){
  let projection,visible=mount.visible!==false,disposed=false,epoch=0,resolving=0,provider='';const mapped=new Map(),queue=[];
  const providers=document.createElement('select');providers.className='mount-provider';providers.setAttribute('aria-label','挂载信息来源');providers.hidden=true;tree.toolbar.append(providers);
  providers.onchange=()=>{provider=providers.value;++epoch;queue.length=0;mapped.clear();decorate();};
  const records=()=>projection?.records||[];
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
  const watch=client.watch({topics:['filesystem.mounts.v1','filesystem.capacity.v1']},value=>{projection=value;decorate();});
  const timer=setInterval(()=>{if(!watch.valid)projection=null;decorate();},1000);
  return {visibility(value){visible=value;decorate();},dispose(){disposed=true;++epoch;queue.length=0;clearInterval(timer);watch.dispose();client.dispose();providers.remove();tree.setMounts(new Map());}};
}
