const $ = id => document.getElementById(id);
const names = { running:'运行中',paused:'已暂停',exited:'已停止',created:'未启动',restarting:'重启中',dead:'异常',removing:'删除中' };
const operations = { start:'启动',stop:'停止',restart:'重启',pause:'暂停',unpause:'恢复',remove:'删除容器' };
const sections = { basic:'配置',state:'状态与健康',environment:'环境变量',ports:'端口',mounts:'挂载',networks:'网络',labels:'标签' };
let snapshot, ready=false, frozen=false, visible=true, epoch=0, revision, timer, selected='',offset=0,nextOffset,section='basic',fieldOffset=0,fieldNext,logOffset=0,logNext,rows=[],fields=[],logRows=[],operationRequest,rendering=false,renderAgain=false,restoreScroll;
const listHistory=[],fieldHistory=[],logHistory=[];
const pending = new Set();
const event = data => {const promise=sodeck.request('event',data);pending.add(promise);return promise.finally(()=>pending.delete(promise));};
function notice(message) { $('notice').textContent=message||''; $('notice').hidden=!message; }
function tokens(values) { for(const [key,value] of Object.entries(values||{})) key==='color-scheme'?document.documentElement.style.colorScheme=value:document.documentElement.style.setProperty('--'+key,value); }
function navigation() { return { query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked,selected,offset,listHistory,section,fieldOffset,fieldHistory,logOffset,logHistory,logQuery:$('log-query').value,tail:Number($('tail').value),scrollTop:$('scroll').scrollTop }; }
async function remember() { if(ready&&!frozen) await call({action:'navigation',state:navigation()}); }
async function call(data) {
  if(!ready||frozen) return;
  const p=event(data); pending.add(p);
  try { const result=await p; notice(''); if(result?.revision) {snapshot=result;renderHeader();} return result; }
  catch(error) {notice(error.message||String(error));}
  finally {pending.delete(p);}
}
function renderHeader() {
  if(!snapshot) return;
  const kind=snapshot.kind;
  for(const id of ['browser','container','logs']) $(id).hidden=id!==kind;
  $('title').textContent=kind==='browser'?'Docker':(snapshot.name||'容器')+(kind==='logs'?' · 日志':'');
  $('state').textContent=['stale','gone'].includes(snapshot.state)?({stale:'数据待刷新',gone:'已删除'}[snapshot.state]):snapshot.status?names[snapshot.status]||snapshot.status:({ready:'已连接',loading:'读取中'}[snapshot.state]||'等待连接');
  $('engine-name').textContent=snapshot.engine?.name||'Engine 尚不可访问';
  $('engine-version').textContent=snapshot.engine?.version?'Docker '+snapshot.engine.version+' · '+snapshot.engine.arch:'';
  $('endpoint').textContent=snapshot.endpoint;
  if(document.activeElement!==$('socket')) $('socket').value=snapshot.endpoint;
  $('refresh').disabled=!!snapshot.busy||frozen||!ready;
  $('cancel').hidden=!snapshot.busy;
  $('pause').hidden=kind!=='browser';$('pause').textContent=snapshot.paused?'恢复刷新':'暂停刷新';
  $('busy').textContent=snapshot.busy?'请求处理中':'';
  $('freshness').textContent=snapshot.observedAt?'采集于 '+new Date(snapshot.observedAt).toLocaleTimeString():'尚未完成采集';
  notice(snapshot.error||(snapshot.partial?'当前数据不完整或已截断；请缩小范围后刷新':''));
  $('container-id').textContent=snapshot.containerId||'';
  const op=snapshot.operation;
  $('operation').hidden=!op; $('operation').textContent=op?(operations[op.action]||op.action)+' · '+({checking:'检查中',running:'执行中',succeeded:'命令成功',failed:'失败',unknown:'结果未确认',skipped:'无需操作'}[op.state]||op.state)+'：'+op.message:'';
  for(const button of document.querySelectorAll('[data-operation]')) {
    const action=button.dataset.operation, state=snapshot.status;
    button.disabled=!!snapshot.busy||!ready||frozen||snapshot.state==='gone'||!snapshot.engine
      ||(action==='remove'&&!['exited','created','dead'].includes(state))||(action==='pause'&&state!=='running')||(action==='unpause'&&state!=='paused');
  }
  $('read-stats').disabled=!!snapshot.busy||!ready||frozen||snapshot.state==='gone';
  if(snapshot.stats) {
    $('stats').replaceChildren();
    for(const [key,label] of Object.entries({CPUPerc:'CPU',MemUsage:'内存 / 限制',MemPerc:'内存占比',NetIO:'网络接收 / 发送',BlockIO:'块读取 / 写入',PIDs:'进程数',unavailable:'状态'})) {
      if(snapshot.stats[key]!=null) appendField($('stats'),label,snapshot.stats[key]);
    }
  }
}
function appendField(target,key,value) {
  const dt=document.createElement('dt'),dd=document.createElement('dd');dt.textContent=key;dd.textContent=value;target.append(dt,dd);
}
function filters() { return {query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked}; }
function renderList() {
  const focus=document.activeElement?.dataset?.id;
  const focusable=rows.some(row=>row.id===selected)?selected:rows[0]?.id;
  $('list').replaceChildren();
  for(const row of rows) {
    const button=document.createElement('button');button.type='button';button.className='container-row';button.dataset.id=row.id;button.setAttribute('role','option');button.setAttribute('aria-selected',String(selected===row.id));button.tabIndex=focusable===row.id?0:-1;
    const health=row.health==='unhealthy'?' · 不健康':row.health==='healthy'?' · 健康':row.health==='starting'?' · 健康检查中':'';
    for(const [className,text] of [['row-name',row.name],['row-status',(names[row.status]||row.status)+health],['row-image',row.image],['row-project',[row.project,row.service].filter(Boolean).join(' / ')]]) {
      const span=document.createElement('span');span.className=className+(row.health==='unhealthy'&&className==='row-status'?' health-error':'');span.textContent=text;button.append(span);
    }
    button.onclick=()=>{selected=row.id;renderList();void remember();};
    button.ondblclick=()=>void open('openContainer',row.id);
    button.onkeydown=e=>{
      if(e.key==='Enter'){e.preventDefault();void open('openContainer',row.id);}
      if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();const index=rows.findIndex(r=>r.id===row.id);selected=rows[Math.max(0,Math.min(rows.length-1,index+(e.key==='ArrowDown'?1:-1)))].id;renderList();$('list').querySelector('[data-id="'+selected+'"]')?.focus();}
    };
    $('list').append(button);
  }
  if(focus) $('list').querySelector('[data-id="'+focus+'"]')?.focus();
  $('open-detail').disabled=$('open-log').disabled=!rows.some(r=>r.id===selected)||!ready||frozen;
  $('empty').hidden=rows.length>0;
}
async function renderBody() {
  if(!snapshot||!ready||frozen) return;
  if(rendering){renderAgain=true;return;}
  rendering=true;const generation=epoch, current=String(snapshot.revision);
  try {
    if(snapshot.kind==='browser') {
      const data=await event({action:'list',offset,limit:50,...filters()});if(generation!==epoch||frozen)return;
      rows=data.rows;nextOffset=data.nextOffset;renderList();$('count').textContent=data.total+' 个';
      $('page-label').textContent=data.total?String(offset+1)+'–'+(offset+rows.length):'0';
      $('previous').disabled=offset===0;$('next').disabled=nextOffset==null;
    } else if(snapshot.kind==='container') {
      if(!$('sections').children.length) for(const [key,label] of Object.entries(sections)){
        const button=document.createElement('button');button.textContent=label;button.dataset.section=key;button.onclick=()=>{section=key;fieldOffset=0;fieldHistory.length=0;void renderBody();};$('sections').append(button);
      }
      const data=await event({action:'section',section,offset:fieldOffset,limit:50});if(generation!==epoch||frozen)return;
      fields=data.rows;fieldNext=data.nextOffset;renderFields();
      $('fields-page').textContent=data.total?String(fieldOffset+1)+'–'+(fieldOffset+fields.length)+' / '+data.total:'无记录';
      $('fields-prev').disabled=fieldOffset===0;$('fields-next').disabled=fieldNext==null;
    } else {
      const data=await event({action:'readLogs',query:$('log-query').value,offset:logOffset,limit:100});if(generation!==epoch||frozen)return;
      logRows=data.rows;logNext=data.nextOffset;renderLogs();
      $('logs-page').textContent=data.total?String(logOffset+1)+'–'+(logOffset+logRows.length)+' / '+data.total:($('log-query').value?'无匹配日志':'无日志');
      $('logs-prev').disabled=logOffset===0;$('logs-next').disabled=logNext==null;
    }
    if(restoreScroll!==undefined){$('scroll').scrollTop=restoreScroll;restoreScroll=undefined;}
    revision=current;
  } catch(error){if(generation===epoch)notice(error.message||String(error));}
  finally {rendering=false;if(renderAgain){renderAgain=false;void renderBody();}}
}
function renderFields() {
  for(const button of $('sections').children)button.setAttribute('aria-current',String(button.dataset.section===section));
  $('show-env-label').hidden=section!=='environment';$('fields').replaceChildren();
  for(const field of fields) appendField($('fields'),field.key,field.secret&&!$('show-env').checked?'值已隐藏':field.value);
}
function renderLogs() {
  $('log-content').textContent=logRows.map(r=>'['+r.stream+'] '+r.text).join('\n');
}
async function poll(force=false) {
  if(!ready||frozen||!visible) return;
  const generation=epoch;
  try {const data=await event({action:'snapshot'});if(generation!==epoch||frozen)return;snapshot=data;renderHeader();if(force||revision!==String(data.revision))await renderBody();}
  catch(error){if(generation===epoch)notice(error.message||String(error));}
}
function schedule() {clearInterval(timer);timer=setInterval(()=>void poll(),1000);}
async function open(action,id) {await call({action,id,requestId:crypto.randomUUID()});}
$('open-detail').onclick=()=>void open('openContainer',selected);
$('open-log').onclick=()=>void open('openLogs',selected);
$('detail-logs').onclick=()=>void open('openLogs',snapshot.containerId);
$('refresh').onclick=()=>{logOffset=fieldOffset=0;logHistory.length=fieldHistory.length=0;void call({action:'refresh',tail:Number($('tail').value)}).then(()=>poll(true));};
$('cancel').onclick=()=>void call({action:'cancel'}).then(()=>poll(true));
$('pause').onclick=()=>void call({action:'pauseRefresh',paused:!snapshot.paused});
$('read-stats').onclick=()=>void call({action:'stats'});
$('endpoint-form').onsubmit=e=>{e.preventDefault();void call({action:'setEndpoint',endpoint:$('socket').value.trim()});};
for(const id of ['query','status-filter','health-filter','group']) $(id).addEventListener(id==='query'?'input':'change',()=>{offset=0;listHistory.length=0;revision=undefined;void renderBody();void remember();});
$('previous').onclick=()=>{offset=listHistory.pop()??0;void renderBody();};
$('next').onclick=()=>{listHistory.push(offset);offset=nextOffset;void renderBody();};
$('fields-prev').onclick=()=>{fieldOffset=fieldHistory.pop()??0;void renderBody();};
$('fields-next').onclick=()=>{fieldHistory.push(fieldOffset);fieldOffset=fieldNext;void renderBody();};
$('logs-prev').onclick=()=>{logOffset=logHistory.pop()??0;void renderBody();};
$('logs-next').onclick=()=>{logHistory.push(logOffset);logOffset=logNext;void renderBody();};
$('show-env').onchange=renderFields;$('log-query').oninput=()=>{logOffset=0;logHistory.length=0;void renderBody();};
async function copy(text){const generation=epoch;try{await sodeckClipboard.writeText(text,()=>generation===epoch&&ready&&!frozen);}catch(error){notice(error.message);}}
$('copy-id').onclick=()=>void copy(snapshot.containerId);
$('copy-logs').onclick=()=>void copy($('log-content').textContent);
for(const button of document.querySelectorAll('[data-operation]')) button.onclick=()=>{
  const action=button.dataset.operation;
  operationRequest={action,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent,trigger:button};
  $('confirm-title').textContent='确认'+operations[action];
  $('confirm-object').textContent=(snapshot.name||'容器')+' · '+snapshot.containerId+' · '+(snapshot.engine?.name||'Engine');
  $('confirm-impact').textContent=action==='remove'?'删除已停止容器，保留挂载卷和 bind 数据。容器本身的可写层会被删除。':action==='restart'?'重启可能中断服务，已停止容器也会被启动。':'对当前容器执行'+operations[action]+'。取消本地等待不会回滚 Engine 已接受的操作。';
  $('confirm-submit').textContent=operations[action];$('confirm-submit').disabled=false;$('confirm').showModal();
};
$('confirm').addEventListener('close',()=>{
  const request=operationRequest;operationRequest=undefined;request?.trigger.focus();
  if($('confirm').returnValue==='submit'&&request)void call({action:'operate',operation:request.action,requestId:request.requestId,intentSequence:request.intentSequence}).then(()=>poll(true));
});
addEventListener('sodeck:mount',e=>{
  epoch++;snapshot=e.detail.data;ready=false;frozen=!!e.detail.readOnly;visible=e.detail.visible!==false;revision=undefined;tokens(e.detail.tokens);
  const nav=snapshot?.navigation||{};selected=nav.selected||'';offset=nav.offset||0;section=nav.section||'basic';fieldOffset=nav.fieldOffset||0;logOffset=nav.logOffset||0;
  for(const [history,key] of [[listHistory,'listHistory'],[fieldHistory,'fieldHistory'],[logHistory,'logHistory']])history.splice(0,history.length,...(nav[key]||[]));
  restoreScroll=nav.scrollTop||0;
  $('query').value=nav.query||'';$('status-filter').value=nav.status||'running';$('health-filter').value=nav.health||'all';$('group').checked=!!nav.group;$('log-query').value=nav.logQuery||'';
  if(nav.tail||snapshot?.tail)$('tail').value=String(nav.tail||snapshot.tail);renderHeader();sodeck.mounted();schedule();
});
addEventListener('sodeck:surfaceReady',()=>{ready=true;frozen=false;void call({action:'resumeInput'}).then(()=>poll(true));});
addEventListener('sodeck:themeChanged',e=>tokens(e.detail.tokens));
addEventListener('sodeck:visibilityChanged',e=>{visible=e.detail.visible!==false;if(visible)void poll(true);});
addEventListener('sodeck:contextChanged',()=>{epoch++;revision=undefined;void poll(true);});
addEventListener('sodeck:settingsChanged',()=>void poll(true));
addEventListener('sodeck:transferCancelled',()=>{frozen=false;void call({action:'resumeInput'}).then(()=>poll(true));});
addEventListener('sodeck:flush',async e=>{
  const writable=ready&&!frozen;frozen=true;
  try{await Promise.allSettled([...pending]);if(writable)await event({action:'navigation',state:navigation()});frozen=!!e.detail.freeze;if($('confirm').open){$('confirm').returnValue='cancel';$('confirm').close();}sodeck.flushed(e.detail.id);}
  catch(error){sodeck.flushed(e.detail.id,error.message);}
});
addEventListener('sodeck:unmount',()=>{epoch++;ready=false;clearInterval(timer);});
