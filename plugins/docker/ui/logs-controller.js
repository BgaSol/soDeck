import {createLogMinimap} from './logs-minimap.js';
import {LogLayout} from './logs-layout.js';
const $ = id => document.getElementById(id);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export function installLogPage(releaseIncumbent=()=>{}) {
  let detailIntent=0;
  function detailPanel(open) {
    if(!open&&$('log-detail').hidden)return;
    if(open&&!$('log-detail').hidden&&!$('log-detail').inert)return;
    const node=$('log-detail'),intent=++detailIntent,from=node.hidden?0:node.getBoundingClientRect().width;
    const opacity=node.hidden?0:getComputedStyle(node).opacity;
    SoDeckMotion.cancelMotion(node);node.hidden=false;node.inert=!open;node.setAttribute('aria-hidden',String(!open));
    const target=open?node.getBoundingClientRect().width:0;
    const animation=SoDeckMotion.animateMotion(node,[{width:`${from}px`,opacity},{width:`${target}px`,opacity:open?1:0}],'layout');
    if(!open){if(animation)animation.finished.catch(()=>{}).finally(()=>{if(intent===detailIntent)node.hidden=true;});else node.hidden=true;}
  }
  let active=false, worker, serial=0, snapshot, capture, generation=-1, enabled=false, visible=true, frozen=false, following=true, selected, total=0, pollTimer, paintTimer, painting=false, paintAgain=false, epoch=0, composing=false, exporting=false, cancelExport=false, map, pins=[], anchors=[], holding, restored=false, starting=false, revision=-1;
  const pending=new Map(), attached=new Set(), replay=new Map(), queues=new Map(), layout=new LogLayout();
  let flushing=false, pollCount=0,conditionVersion=0,positionVersion=0,minimapSetting,readingAnchor,resizeAnchor; const pollWaiters=[];
  const event = params => sodeck.request('event', params);
  const data = params => sodeck.request('data', params);
  const localTime=value=>new Date(value).toLocaleString('zh-CN',{hour12:false,year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'});
  const error = message => { SoDeckMotion.setMotionText($('log-notice'),message||''); $('log-notice').hidden=!message; };
  function ask(type, params={}) { if(!worker) return Promise.reject(new Error('日志页面已卸载')); const requestId=++serial; return new Promise((resolve,reject)=>{pending.set(requestId,{resolve,reject}); worker.postMessage({type,requestId,...params});}); }
  const conditions = () => ({query:$('log-query').value,only:$('log-only').checked,levels:$('log-level').value?[$('log-level').value]:[],sources:$('log-source').value?[$('log-source').value]:[]});
  function controls() { for(const element of $('logs').querySelectorAll('button,input,select')) element.disabled=!enabled||frozen; $('log-source').disabled=!enabled||frozen||!!snapshot?.tty; $('log-export').disabled=!enabled||frozen||exporting; $('log-export-cancel').hidden=!exporting; }
  function theme(tokens) { for(const [key,value] of Object.entries(tokens||{})) document.documentElement.style.setProperty('--'+key,value); document.documentElement.style.colorScheme=tokens?.['color-scheme']||'light'; map?.theme(); }
  function schedule() { if(paintTimer||!active||!visible||frozen) return; paintTimer=setTimeout(()=>{paintTimer=null; void paint().catch(e=>error(e.message));},120); }
  async function releasePins() { const old=pins; pins=[]; anchors=[]; for(const p of old) await data({op:'sessionStream',action:'unpin',...p}).catch(()=>{}); }
  function protect() {
    if(!following||!active||!enabled||frozen) return holding||Promise.resolve(); positionVersion++;following=false; $('log-latest').hidden=false;
    holding=(async()=>{ anchors=await ask('anchor'); for(const range of anchors) { try { const p=await data({op:'sessionStream',action:'pin',id:range.id,start:String(range.start),end:String(range.end)}); pins.push({id:range.id,pinId:p.pinId}); } catch(e) { error('当前内容可继续阅读，恢复窗口未固定：'+e.message); } } schedule(); })().catch(e=>{if(active)error(e.message);}).finally(()=>holding=null);
    return holding;
  }
  async function latest() { await holding; await releasePins(); await ask('latest'); following=true; selected=undefined; $('log-detail').hidden=true; $('log-latest').hidden=true; await paint(); }
  function navigation() { const anchor=layout.anchor($('log-viewport').scrollTop-$('log-lines').offsetTop); return {version:1,following,query:conditions(),anchor:anchor.id,pixel:anchor.pixel,wrap:$('log-wrap').checked,minimap:$('log-map').checked,showSource:$('log-show-source').checked,pins,anchors}; }
  async function remember() { await holding; if(active&&enabled&&!frozen) await event({action:'logs.navigation',state:navigation()}); }
  function applyNavigation(nav) {
    if(nav?.version!==1) return; following=nav.following!==false; $('log-query').value=nav.query?.query||''; $('log-only').checked=!!nav.query?.only;
    const level=nav.query?.levels?.[0]||'';if(level&&![...$('log-level').options].some(o=>o.value===level))$('log-level').add(new Option(level,level));$('log-level').value=level; $('log-source').value=nav.query?.sources?.[0]||''; $('log-wrap').checked=nav.wrap!==false; $('log-map').checked=nav.minimap!==false; $('log-show-source').checked=!!nav.showSource;
    $('log-viewport').classList.toggle('no-wrap',!$('log-wrap').checked); map?.show($('log-map').checked); pins=nav.pins||[]; anchors=nav.anchors||[]; $('log-latest').hidden=following;
  }
  function resetControls() {
    readingAnchor=resizeAnchor=undefined;
    $('log-detail').inert=true;$('log-detail').setAttribute('aria-hidden','true');
    $('log-range').value='latest'; $('log-query').value=''; $('log-only').checked=false;
    $('log-level').value=''; $('log-source').value=''; $('log-wrap').checked=true; $('log-show-source').checked=false;
    $('log-export-one').checked=false; $('log-export-context').checked=false; $('log-custom').hidden=true;
    $('log-from').value=''; $('log-to').value=''; $('log-detail').hidden=true; $('log-latest').hidden=true;
    $('log-viewport').classList.remove('no-wrap'); $('log-viewport').scrollTop=0; $('log-lines').replaceChildren(); error('');
    filterChips();
  }
  function filterChips() {
    const root=$('log-conditions'),nodes=[],key=JSON.stringify([conditions().levels,conditions().sources,conditions().only,enabled,frozen]);if(root.dataset.conditions===key)return;root.dataset.conditions=key;
    for(const [id,label,reset] of [['log-level','级别',()=>{$('log-level').value='';}],['log-source','来源',()=>{$('log-source').value='';}],['log-only','只看匹配',()=>{$('log-only').checked=false;}]]) {
      const input=$(id),value=id==='log-only'?input.checked:input.value;if(!value)continue;
      const button=document.createElement('button');button.type='button';button.className='log-filter-chip';button.textContent=label+(id==='log-only'?'':'：'+input.selectedOptions[0].textContent)+' · 清除';button.disabled=!enabled||frozen;button.onclick=()=>{reset();void query();};nodes.push(button);
    }
    root.replaceChildren(...nodes);root.hidden=!nodes.length;
  }
  function lineNode(row) {
    const line=document.createElement('div'); line.className='log-line'+(row.context?' is-context':''); line.dataset.id=row.id; line.dataset.index=row.index;
    const time=document.createElement('button'); time.type='button'; time.className='log-time'; const clock=row.time?new Date(row.time):new Date(row.receivedAt); time.textContent=clock.toLocaleTimeString('zh-CN',{hour12:false})+(row.time?'.'+String(clock.getMilliseconds()).padStart(3,'0'):'');if(row.day){const day=document.createElement('span');day.className='log-day';day.textContent=row.day;time.prepend(day);} time.title=row.time?'源时间：'+row.time:'接收时间（此记录没有源时间）'; time.onclick=()=>void detail(row.id); time.disabled=!enabled||frozen; line.append(time);
    const body=document.createElement('div'); body.className='log-body';
    const prefix=/^\s*(?:\[\d[^\]]*\]\s*)?\[?(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL)\b\]?/i.exec(row.text),levelStart=prefix?prefix[0].lastIndexOf(prefix[1]):-1,levelEnd=levelStart+(prefix?.[1].length||0);
    if(row.level&&!prefix) { const badge=document.createElement('span'); badge.className='log-level level-'+row.level; badge.textContent=row.level; body.append(badge); }
    if($('log-show-source').checked) { const source=document.createElement('span'); source.className='log-origin'; source.textContent=row.source; body.append(source); }
    const piece=(a,b)=>{const fragment=document.createDocumentFragment(),from=Math.max(a,levelStart),to=Math.min(b,levelEnd);if(!row.level||from>=to){fragment.append(document.createTextNode(row.text.slice(a,b)));return fragment;}fragment.append(document.createTextNode(row.text.slice(a,from)));const level=document.createElement('span');level.className='level-'+row.level;level.textContent=row.text.slice(from,to);fragment.append(level,document.createTextNode(row.text.slice(to,b)));return fragment;};
    const text=document.createElement('span'); text.className='log-text'; let cursor=0; for(const [a,b] of row.matches) { text.append(piece(cursor,a)); const mark=document.createElement('mark'); mark.append(piece(a,b)); text.append(mark); cursor=b; } text.append(piece(cursor,row.text.length)); body.append(text);
    if(row.length>row.text.length) { const full=document.createElement('button'); full.textContent='查看全文'; full.onclick=()=>void detail(row.id); body.append(full); }
    if(row.incomplete) { const flag=document.createElement('span'); flag.className='muted'; flag.textContent=' · 长记录未完整保留'; body.append(flag); }
    if($('log-only').checked) { const context=document.createElement('button'); context.className='log-context'; context.textContent='上下文'; context.onclick=()=>void protect().then(()=>ask('context',{id:row.id})).then(()=>paint()); body.append(context); }
    line.append(body); return line;
  }
  function visibleAnchor() {
    const viewport=$('log-viewport'),bounds=viewport.getBoundingClientRect(),top=bounds.top;
    const line=[...$('log-lines').querySelectorAll('.log-line')].find(node=>{const r=node.getBoundingClientRect();return r.bottom>top&&r.top<bounds.bottom;});
    return line?{id:line.dataset.id,pixel:top-line.getBoundingClientRect().top}:undefined;
  }
  async function paint() {
    if(!worker||!visible||frozen) return; if(painting) {paintAgain=true;return;} painting=true; const local=epoch,version=conditionVersion,position=positionVersion;
    const stale=()=>local!==epoch||version!==conditionVersion||position!==positionVersion;
    try {
      const viewport=$('log-viewport'), container=$('log-lines'), selection=getSelection();
      const oldAnchor=resizeAnchor||(!following&&visibleAnchor())||layout.anchor(viewport.scrollTop-container.offsetTop); const info=await ask('view',{start:0,count:0}); if(stale()) return;
      total=info.total;
      filterChips();
      $('log-latest').textContent=info.newCount?`${info.newCount.toLocaleString()}条新日志 · 查看最新`:'查看最新';
      if(selection?.toString()&&container.contains(selection.anchorNode)) return;
      if(revision!==info.revision) { const next=await ask('layout'); if(stale()) return; layout.reset(next.ids); revision=next.revision; }
      // Plan the new slice without scrolling the old DOM while Worker requests are pending.
      const anchoredTop=following?null:layout.restore(oldAnchor);
      const top=following?Math.max(0,layout.offset(total)-viewport.clientHeight):Math.max(0,anchoredTop??(viewport.scrollTop-container.offsetTop));
      const start=Math.max(0,layout.index(top)-8), end=Math.min(total,layout.index(top+viewport.clientHeight)+20);
      const result=await ask('view',{start,count:Math.max(30,end-start)}); if(stale()) return;
      const before=document.createElement('div'), after=document.createElement('div'); before.className=after.className='log-spacer'; before.style.height=layout.offset(start)+'px'; after.style.height=(layout.offset(total)-layout.offset(start+result.rows.length))+'px';
      const nodes=result.rows.map(lineNode); container.replaceChildren(before,...nodes,after); for(let i=0;i<nodes.length;i++) layout.measure(start+i,nodes[i].getBoundingClientRect().height);
      before.style.height=layout.offset(start)+'px'; after.style.height=(layout.offset(total)-layout.offset(start+nodes.length))+'px';
      if(following) viewport.scrollTop=viewport.scrollHeight; else { const anchor=layout.restore(oldAnchor); if(anchor!==null) viewport.scrollTop=anchor+container.offsetTop; }
      const filtered=info.windowTotal!==total,first=info.first||info.firstReceivedAt,last=info.last||info.lastReceivedAt,range=first!=null?` · ${info.first?'日志':'接收'}时间 ${localTime(first)}${total>1?`–${info.last?'':'接收 '}${localTime(last)}`:''}`:'';
      $('log-scope').textContent=`${filtered?'当前结果':'当前窗口'} · ${total.toLocaleString()}条${filtered?`（已读${info.windowTotal.toLocaleString()}条）`:''}${range}`;
      $('log-matches').textContent=conditions().query?`${info.matches.toLocaleString()}处`:''; $('log-latest').textContent=info.newCount?`${info.newCount.toLocaleString()}条新日志 · 查看最新`:'查看最新';
      $('log-empty').hidden=total>0; $('log-empty').textContent=capture?.loading?'正在读取日志…':conditions().query||conditions().levels.length||conditions().sources.length?'当前已读窗口没有匹配；可调整条件或读取其他时间段':'当前窗口没有日志，可选择更早时间段';
      const selectedLevel=$('log-level').value; for(const value of info.levels) if(![...$('log-level').options].some(o=>o.value===value)) $('log-level').add(new Option(value,value)); $('log-level').value=selectedLevel;
      if(info.gap) error('部分内容已退出缓冲或长记录未完整保留；搜索与缩略仅覆盖当前窗口，可按时间重新读取。');
      if(!following) {
        const line=nodes.find(node=>node.dataset.id===oldAnchor.id);
        if(line) viewport.scrollTop+=line.getBoundingClientRect().top-viewport.getBoundingClientRect().top+oldAnchor.pixel;
        resizeAnchor=undefined;
      }
      const summary=await ask('summary',{height:Math.ceil(viewport.clientHeight)}); if(stale()) return;
      map.update({...summary,ids:layout.ids},layout.index(Math.max(0,viewport.scrollTop-container.offsetTop)),layout.index(Math.max(0,viewport.scrollTop+viewport.clientHeight-container.offsetTop)));
      if(!following&&viewport.clientWidth===viewportWidth&&devicePixelRatio===viewportScale)readingAnchor=visibleAnchor();
    } finally { painting=false; if(paintAgain) {paintAgain=false;schedule();} }
  }
  async function jump(index,id) { const local=epoch;positionVersion++;await protect();const next=await ask('layout');if(local!==epoch)return;layout.reset(next.ids);revision=next.revision;if(id)index=next.ids.indexOf(id);if(index<0)return;const viewport=$('log-viewport'); viewport.scrollTop=layout.offset(index)+$('log-lines').offsetTop; await paint(); }
  async function detail(id) { await protect(); selected=id; const row=await ask('detail',{id}); if(!row) return; detailPanel(true); $('log-detail-time').textContent=(row.time?'日志时间 ':'接收时间 ')+localTime(row.time||row.receivedAt)+' · '+row.source;$('log-detail-time').title=row.time?'原始源时间：'+row.time:''; $('log-detail-raw').textContent=row.text; $('log-json').hidden=true; try {const json=JSON.parse(row.text);$('log-json').hidden=false;$('log-detail-json').textContent=JSON.stringify(json,null,2);}catch{} $('log-detail-close').focus(); layout.reset(layout.ids,true); schedule(); }
  async function query() { if(composing||!enabled||frozen) return;const version=++conditionVersion,local=epoch;positionVersion++;await protect();if(version!==conditionVersion||local!==epoch)return;const position=positionVersion;await ask('query',{conditions:conditions()});if(version!==conditionVersion||local!==epoch)return;revision=-1;if(position===positionVersion)$('log-viewport').scrollTop=0; await paint(); void remember(); }
  async function navigate(direction) { await protect(); const target=await ask('navigate',{id:selected,direction}); if(target) {selected=target.id;await jump(target.index,target.id);$('log-lines').querySelector(`[data-id="${CSS.escape(target.id)}"]`)?.scrollIntoView({block:'center'});} }
  async function attachResources(current) {
    if(current.generation!==generation) { for(const id of attached) await data({op:'sessionStream',action:'detach',id}).catch(()=>{}); attached.clear(); queues.clear(); replay.clear(); generation=current.generation; await ask('reset',{generation});await ask('query',{conditions:conditions()}); layout.reset([],true); revision=-1; total=0; selected=undefined; $('log-lines').replaceChildren(); }
    for(const resource of current.resources||[]) {
      if(!resource.id) {error('日志启动回执待确认，正在查询原请求；不会重新执行命令。');continue;}
      if(attached.has(resource.id)) continue;
      await ask('register',{resource,combined:!!snapshot.tty}); const pin=anchors.find(r=>r.id===resource.id);
      const completion=new Promise(resolve=>replay.set(resource.id,resolve)); attached.add(resource.id);
      try { await data({op:'sessionStream',action:'attach',id:resource.id,...(pin?{offset:String(pin.start)}:{})}); } catch(e) { attached.delete(resource.id); replay.get(resource.id)?.(); replay.delete(resource.id); error('日志恢复失败：'+e.message); }
      resource.replay=completion;
    }
  }
  async function start(complete=false) {
    if(!enabled||frozen||starting) return; starting=true; $('log-reload').disabled=true;
    try {
      const sourceNow=Date.parse(snapshot?.engine?.systemTime), now=Math.floor((Number.isFinite(sourceNow)?sourceNow:Date.now())/1000), mode=$('log-range').value;
      let begin=now-Number(mode==='latest'?900:mode), end=now; if(mode==='custom') {begin=Math.floor(new Date($('log-from').value).getTime()/1000);end=Math.floor(new Date($('log-to').value).getTime()/1000);}
      if(!Number.isFinite(begin)||!Number.isFinite(end)||begin>=end) throw new Error('请选择有效的开始和结束时间');
      await holding; await releasePins(); const answer=await event({action:'logs.start',requestId:crypto.randomUUID(),intentSequence:capture?.nextIntent||'1',start:begin,end,now,...(mode==='custom'?{}:{windowSeconds:mode==='latest'?900:Number(mode)}),live:mode==='latest',complete});
      if(answer.accepted) {following=true;restored=true;$('log-latest').hidden=true;error('');} await poll();
    } catch(e) {error(e.message);} finally {starting=false;controls();}
  }
  async function poll() {
    if(!active||!enabled||frozen||flushing) return; const local=epoch; pollCount++;
    try {
      snapshot=await event({action:'snapshot'}); if(local!==epoch||frozen) return;
      const setting=snapshot.showLogMinimap!==false;if(setting!==minimapSetting){minimapSetting=setting;$('log-map').checked=setting;map?.show(setting);}
      $('title').textContent=(snapshot.name||'容器')+' · 日志'; $('state').textContent=snapshot.status==='running'?'运行中':snapshot.status==='exited'?'已停止':snapshot.state;
      $('log-origin-label').textContent=(snapshot.engine?.name||'Engine')+' · '+snapshot.endpoint;
      capture=await event({action:'logs.status'}); if(local!==epoch||frozen) return;
      if(!restored) {applyNavigation(capture.navigation);await ask('query',{conditions:conditions()});restored=true;}
      await attachResources(capture);
      const live=capture.resources?.find(r=>r.end==null); $('log-status').textContent=live?({opening:'正在连接日志',running:'持续接收',completed:'日志流已结束',cancelled:'读取已停止',failed:'日志连接失败'}[live.status.state]||'持续接收'):capture.resources?.length?'查看历史':'等待读取';
      const finite=(capture.resources||[]).filter(r=>r.end!=null); $('log-coverage').textContent=finite.some(r=>r.tail)?'尾部预览 · 未覆盖完整时间范围':finite.some(r=>!r.status.finished)?'所选范围仍在读取':finite.some(r=>r.status.state!=='completed')?'所选范围未完整读取':finite.length?'已读取所选范围':'';
      if(capture.error) error(capture.error); for(const r of capture.resources||[]) if(r.status.error?.message) error('日志读取未完成：'+r.status.error.message);
      if(snapshot.tty) $('log-source').value=''; controls(); schedule();
      if(!capture.resources?.length&&!capture.loading&&!starting&&snapshot.state==='ready'&&generation===0) void start();
    } catch(e) {if(local===epoch)error(e.message);} finally {pollCount--;if(!pollCount)for(const resolve of pollWaiters.splice(0))resolve();clearTimeout(pollTimer);if(local===epoch&&active&&enabled&&!frozen&&!flushing)pollTimer=setTimeout(()=>void poll(),1000);}
  }
  async function exportLogs() {
    if(exporting||!enabled||frozen) return; await protect(); exporting=true; cancelExport=false; controls(); let id;
    try {
      const view=await ask('view',{count:0}), header=`SoDeck Docker日志\n服务器：${snapshot.context.serverId}\nEngine：${snapshot.engine?.name} (${snapshot.engine?.id})\n容器：${snapshot.name} (${snapshot.containerId})\n范围：${$('log-scope').textContent}\n条件：${JSON.stringify(conditions())}\n覆盖：${$('log-coverage').textContent}${view.gap?'；存在未保留内容':''}\n以下为原始Docker输出（含源时间戳；TTY来源合并）。`;
      if($('log-export-one').checked&&!selected) throw new Error('先选择一条日志记录');
      const exported=await ask('exportBegin',{id:$('log-export-one').checked?selected:undefined,header,context:$('log-export-context').checked});
      const begun=await data({op:'contentExport',action:'begin',requestId:crypto.randomUUID(),name:(snapshot.name||'container').replace(/[\\/:*?"<>|]/g,'_')+'-logs.txt',byteLength:String(exported.length)}); id=begun.id;
      let status; do {await delay(250);if(cancelExport||!active)throw new Error('导出已取消');status=await data({op:'contentExport',action:'status',id});}while(status.state==='waitingInput');
      if(status.state==='cancelled') return; if(status.state!=='receiving')throw new Error(status.error||'导出未就绪');
      let chunk; do {if(cancelExport||!active)throw new Error('导出已取消');chunk=await ask('exportChunk');const request={op:'contentExport',action:'append',id,offset:String(chunk.offset),data:[...chunk.bytes]};let result;for(let retry=0;retry<3;retry++){try{result=await data(request);break}catch(e){if(retry===2)throw e}}if(result.state==='failed')throw new Error(result.error);$('log-export-progress').textContent=`导出 ${Math.round((chunk.offset+chunk.bytes.length)/exported.length*100)}%`;}while(!chunk.done);
      const finished=await data({op:'contentExport',action:'finish',id});if(finished.state!=='completed')throw new Error(finished.error||'导出未完成');error('日志已导出');
    } catch(e) {if(id)await data({op:'contentExport',action:'cancel',id}).catch(()=>{});error(e.message);} finally {if(worker)await ask('exportEnd');exporting=false;$('log-export-progress').textContent='';controls();}
  }
  function stop() { detailIntent++;SoDeckMotion.cancelMotion($('log-detail'));clearTimeout(pollTimer);clearTimeout(paintTimer);pollTimer=paintTimer=null;cancelExport=true;map?.dispose();map=null;worker?.terminate();worker=null;for(const p of pending.values())p.reject(new Error('日志页面已卸载'));pending.clear();for(const resolve of replay.values())resolve();replay.clear();attached.clear();queues.clear();epoch++; }
  const lifecycle=(name,handler)=>addEventListener('sodeck:'+name,e=>{if(!active)return;e.stopImmediatePropagation();Promise.resolve(handler(e.detail)).catch(e=>error(e.message));});
  addEventListener('sodeck:mount',e=>{
    if(e.detail.data.kind!=='logs') {if(active)stop();active=false;document.body.classList.remove('logs-page');return;} e.stopImmediatePropagation();releaseIncumbent(); stop();active=true;document.body.classList.add('logs-page');for(const name of ['browser','container','resource'])$(name).hidden=true;$('logs').hidden=false;
    snapshot=e.detail.data;enabled=false;frozen=!!e.detail.readOnly;flushing=false;visible=e.detail.visible!==false;following=true;restored=false;generation=-1;pins=[];anchors=[];holding=null;starting=false;revision=-1;conditionVersion++;composing=false;selected=undefined;minimapSetting=snapshot.showLogMinimap!==false;resetControls();theme(e.detail.tokens);
    worker=new Worker(new URL('./logs-worker.js',import.meta.url),{type:'module'});worker.onmessage=e=>{const p=pending.get(e.data.requestId);if(!p)return;pending.delete(e.data.requestId);e.data.error?p.reject(new Error(e.data.error)):p.resolve(e.data.data);};worker.onerror=()=>error('日志索引任务已停止，请重新打开日志页');
    map=createLogMinimap($('log-main'),(index,id)=>void jump(index,id).catch(e=>error(e.message)),()=>void protect());map.show(snapshot.showLogMinimap!==false);$('log-map').checked=snapshot.showLogMinimap!==false;controls();$('title').textContent=(snapshot.name||'容器')+' · 日志';
    const local=epoch;void (async()=>{if(snapshot.logCapture){capture=snapshot.logCapture;applyNavigation(capture.navigation);await ask('query',{conditions:conditions()});await attachResources(capture);await Promise.all((capture.resources||[]).map(r=>r.replay));if(!following&&anchors.length)await ask('restore',{ranges:anchors});restored=true;}if(local===epoch)sodeck.mounted();})().catch(e=>{error(e.message);if(local===epoch)sodeck.mounted();});
  });
  lifecycle('surfaceReady',async()=>{enabled=true;frozen=false;controls();await event({action:'resumeInput'});await poll();if(!following&&capture?.navigation?.anchor){await paint();const index=await ask('locate',{id:capture.navigation.anchor});if(index>=0){$('log-viewport').scrollTop=layout.offset(index)+$('log-lines').offsetTop+(capture.navigation.pixel||0);schedule();}else error('原阅读位置已退出保留窗口，可按时间重新读取。');}});
  lifecycle('data',message=>{
    const frame=message.data,id=frame?.metadata?.streamId;if(!id||!attached.has(id))return;
    const local=epoch, prior=queues.get(id)||Promise.resolve();const next=prior.then(async()=>{if(local!==epoch)return;if(frame.kind==='reset'||frame.kind==='gap')await ask('resetSource',{id,offset:frame.offset,gap:frame.kind==='gap',pinned:anchors.some(r=>r.id===id)});if(frame.kind==='data')await ask('append',{id,bytes:frame.data,end:frame.offset});if(frame.kind==='end')await ask('end',{id});if(frame.packet)await data({op:'sessionStream',action:'ack',id,packet:frame.packet});if(frame.kind==='caughtUp'||frame.kind==='end'){replay.get(id)?.();replay.delete(id);}schedule();}).catch(e=>{error(e.message);replay.get(id)?.();replay.delete(id);void data({op:'sessionStream',action:'detach',id}).catch(()=>{});attached.delete(id);});queues.set(id,next);
  });
  lifecycle('themeChanged',e=>{theme(e.tokens);schedule();}); lifecycle('visibilityChanged',e=>{visible=e.visible!==false;if(visible){void poll();schedule();}else map?.pause();});
  lifecycle('flush',async e=>{try{if(exporting)throw new Error('日志正在导出，请完成或取消导出后移动');flushing=true;clearTimeout(pollTimer);await remember();if(pollCount)await new Promise(resolve=>pollWaiters.push(resolve));frozen=!!e.freeze;map?.pause();controls();sodeck.flushed(e.id);if(!e.freeze){flushing=false;void poll();}}catch(err){flushing=false;sodeck.flushed(e.id,err.message);}});
  lifecycle('transferCancelled',async()=>{frozen=false;flushing=false;enabled=true;controls();await event({action:'resumeInput'});await poll();});
  lifecycle('contextChanged',()=>{error('连接已改变，保留已读日志；重新读取前核对容器身份');void poll();});
  lifecycle('settingsChanged',()=>{if(snapshot.showLogMinimap!==undefined){$('log-map').checked=snapshot.showLogMinimap;map.show(snapshot.showLogMinimap);}void poll();});
  lifecycle('unmount',()=>{stop();active=false;enabled=false;});
  $('log-viewport').addEventListener('scroll',()=>{if(!active)return;positionVersion++;if(painting){paintAgain=true;return;}const viewport=$('log-viewport');if(!following&&!resizeAnchor&&viewport.clientWidth===viewportWidth&&devicePixelRatio===viewportScale)readingAnchor=visibleAnchor();schedule();});
  $('log-viewport').addEventListener('wheel',e=>{if(e.deltaY<0)void protect();},{passive:true});
  $('log-viewport').addEventListener('keydown',e=>{if(['ArrowUp','PageUp','Home'].includes(e.key))void protect();});
  $('log-viewport').addEventListener('pointerdown',()=>{if(active)void protect();});$('log-query').addEventListener('compositionstart',()=>composing=true);$('log-query').addEventListener('compositionend',()=>{composing=false;void query();});$('log-query').oninput=()=>void query();
  for(const id of ['log-only','log-level','log-source'])$(id).onchange=()=>void query();$('log-prev-match').onclick=()=>void navigate(-1);$('log-next-match').onclick=()=>void navigate(1);$('log-latest').onclick=()=>void latest();$('log-hold').onclick=()=>void protect();
  $('log-reload').onclick=()=>void start();$('log-complete').onclick=()=>void start(true);$('log-stop').onclick=()=>void event({action:'logs.stop'}).then(poll);$('log-range').onchange=()=>{$('log-custom').hidden=$('log-range').value!=='custom';if($('log-range').value!=='custom')void start();};$('log-read-custom').onclick=()=>void start(true);
  $('log-earlier').onclick=async()=>{await protect();const end=Math.min(...(capture?.resources||[]).filter(r=>r.end!=null).map(r=>r.start));if(!Number.isFinite(end)||end<=0){error('已到当前历史范围起点，可选择时间范围重新读取');return;}await event({action:'logs.loadEarlier',end});await poll();};
  const reflow=()=>{positionVersion++;layout.reset(layout.ids,true);schedule();void remember();};$('log-wrap').onchange=()=>{$('log-viewport').classList.toggle('no-wrap',!$('log-wrap').checked);reflow();};$('log-map').onchange=()=>{map?.show($('log-map').checked);reflow();};$('log-show-source').onchange=reflow;
  $('log-detail-close').onclick=()=>{detailPanel(false);$('log-viewport').focus();reflow();};$('log-copy').onclick=async()=>{const row=await ask('detail',{id:selected});if(row)try{await sodeckClipboard.writeText(row.raw,()=>active&&enabled&&!frozen);SoDeckMotion.feedbackMotion($('log-copy'));}catch(e){error(e.message);}};$('log-export').onclick=()=>void exportLogs();$('log-export-cancel').onclick=()=>cancelExport=true;
  let viewportWidth=0,viewportScale=devicePixelRatio;
  const resize=new ResizeObserver(()=>{
    const viewport=$('log-viewport'),width=viewport.clientWidth;if(!active||!width)return;
    // Height changes (status/notice/hidden page) do not invalidate measured line heights.
    if(viewportWidth&&(width!==viewportWidth||devicePixelRatio!==viewportScale)){if(!following)resizeAnchor=resizeAnchor||readingAnchor||visibleAnchor();layout.reset(layout.ids,true);const top=layout.restore(resizeAnchor);if(!following&&top!==null)viewport.scrollTop=top+$('log-lines').offsetTop;}
    viewportWidth=width;viewportScale=devicePixelRatio;positionVersion++;schedule();
  });resize.observe($('log-viewport'));
  addEventListener('keydown',e=>{if(!active||!enabled||frozen)return;if((e.ctrlKey||e.metaKey)&&e.key==='f'){e.preventDefault();$('log-query').focus();}if(e.key==='F3'){e.preventDefault();void navigate(e.shiftKey?-1:1);}if(e.key==='Escape'&&!$('log-detail').hidden){detailPanel(false);$('log-viewport').focus();reflow();}});
}
