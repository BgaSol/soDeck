import {installLogPage} from './logs-controller.js';
import {installBrowserPage} from './browser.js';
installBrowserPage(()=>{clearTimeout(timer);timer=null;ready=false;frozen=true;epoch++;});
installLogPage(()=>{clearTimeout(timer);timer=null;ready=false;frozen=true;epoch++;});
const $ = id => document.getElementById(id);
const names = { running:'运行中',paused:'已暂停',exited:'已停止',created:'未启动',restarting:'重启中',dead:'异常',removing:'删除中' };
const operations = { start:'启动',stop:'停止',restart:'重启',pause:'暂停',unpause:'恢复',remove:'删除容器' };
const sections = { basic:'配置',state:'状态与健康',environment:'环境变量',ports:'端口',mounts:'挂载',networks:'网络',labels:'标签' };
SoDeckMotion.observeMotion(document);
let snapshot, ready=false, frozen=false, visible=true, epoch=0, revision, timer, selected='',offset=0,nextOffset,section='basic',fieldOffset=0,fieldNext,rows=[],fields=[],operationRequest,rendering=false,renderAgain=false,restoreScroll;
const listHistory=[],fieldHistory=[];
const pending = new Set();
let terminalOpening=false;
let shellOptions=false,containerShell='',operationSubmitting=false;
const confirmPanel=$('confirm');
const event = data => {const promise=sodeck.request('event',data);pending.add(promise);return promise.finally(()=>pending.delete(promise));};
function notice(message) { SoDeckMotion.setMotionText($('notice'),message||''); $('notice').hidden=!message; }
function tokens(values) { for(const [key,value] of Object.entries(values||{})) key==='color-scheme'?document.documentElement.style.colorScheme=value:document.documentElement.style.setProperty('--'+key,value); }
function navigation() { return { query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked,selected,offset,listHistory,section,fieldOffset,fieldHistory,scrollTop:$('scroll').scrollTop,shell:containerShell }; }
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
  for(const id of ['browser','container','logs','resource']) $(id).hidden=id!==kind;
  $('browser-more').hidden=true;
  $('title').textContent=kind==='browser'?'Docker':(snapshot.name||'容器')+(kind==='logs'?' · 日志':'');
  $('state').textContent=['stale','gone'].includes(snapshot.state)?({stale:'数据待刷新',gone:'已删除'}[snapshot.state]):snapshot.status?names[snapshot.status]||snapshot.status:({ready:'已连接',loading:'读取中'}[snapshot.state]||'等待连接');
  $('engine-name').textContent=snapshot.engine?.name||'Engine 尚不可访问';
  $('engine-version').textContent=snapshot.engine?.version?'Docker '+snapshot.engine.version+' · '+snapshot.engine.arch:'';
  $('connection').hidden=kind!=='browser';$('count').hidden=kind!=='browser';
  $('endpoint').textContent=snapshot.endpoint;
  $('refresh').disabled=!!snapshot.busy||frozen||!ready;
  $('cancel').hidden=!snapshot.busy;
  $('pause').hidden=kind!=='browser';$('pause').textContent=snapshot.paused?'恢复刷新':'暂停刷新';
  $('busy').textContent=snapshot.busy?'请求处理中':'';
  $('freshness').textContent=snapshot.observedAt?'采集于 '+new Date(snapshot.observedAt).toLocaleTimeString():'尚未完成采集';
  notice(snapshot.error||(snapshot.partial?'当前数据不完整或已截断；请缩小范围后刷新':''));
  $('container-id').textContent=snapshot.containerId||'';
  const terminal=snapshot.terminal;
  $('open-terminal').disabled=terminalOpening||!!snapshot.busy||!ready||frozen||snapshot.status!=='running'||snapshot.state!=='ready'||terminal?.status==='pending';
  $('terminal-shell').disabled=$('open-terminal').disabled;
  if(!containerShell&&document.activeElement!==$('terminal-shell'))$('terminal-shell').value=snapshot.defaultShell||'';
  $('terminal-result').hidden=!terminal;
  $('terminal-result').textContent=terminal?({pending:'终端请求待确认',completed:'终端页面已创建；容器连接结果请在终端查看',failed:'终端请求失败',invalidated:'终端请求已失效',unknown:'终端执行结果未确认'}[terminal.status]||terminal.status)+(terminal.error?.message?'：'+terminal.error.message:''):'';
  const op=snapshot.operation;
  $('operation').hidden=!op; $('operation').textContent=operationText(op);
  for(const button of document.querySelectorAll('[data-operation]')) {
    const action=button.dataset.operation, state=snapshot.status;
    button.hidden=!available(action,state)||(snapshot.engine?.os==='windows'&&['pause','unpause'].includes(action));
    button.disabled=!writable()
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
function writable() {return ready&&!frozen&&!operationSubmitting&&!snapshot?.busy&&snapshot?.state==='ready'&&!!snapshot?.engine;}
function available(action,state) {return ({start:['exited','created','dead'],stop:['running','paused','restarting'],restart:['running'],pause:['running'],unpause:['paused'],remove:['exited','created','dead']}[action]||[]).includes(state);}
function identity(row) {return {id:row.id,engineId:snapshot.engine.id,endpoint:snapshot.endpoint,expectedContext:structuredClone(snapshot.context)};}
function operationText(op) {return op?(op.name?op.name+' · ':'')+(operations[op.action]||op.action)+' · '+({checking:'检查中',running:'执行中',succeeded:'命令成功',failed:'失败',unknown:'结果未确认',skipped:'无需操作'}[op.state]||op.state)+'：'+op.message:'';}
function terminalText(terminal) {return terminal?({pending:'终端请求待确认',completed:'终端页面已创建；连接结果请在终端查看',failed:'终端请求失败',invalidated:'终端请求已失效',unknown:'终端执行结果未确认'}[terminal.status]||terminal.status)+(terminal.error?.message?'：'+terminal.error.message:''):'';}
function actionButton(text,handler,key) {const button=document.createElement('button');button.type='button';button.textContent=text;button.dataset.focus=key;button.onclick=handler;return button;}
function closeConfirmation() {
  const request=operationRequest,restoreFocus=confirmPanel.contains(document.activeElement);operationRequest=undefined;confirmPanel.hidden=true;$('confirm-home').append(confirmPanel);
  if(!request||!restoreFocus)return;
  request.trigger?.focus();
}
function beginOperation(action,row,trigger) {
  if(!writable())return;
  const request={action,target:identity(row),name:row.name,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent,trigger};
  if(['start','unpause'].includes(action)){void submitOperation(request);return;}
  operationRequest=request;
  $('confirm-title').textContent='确认'+operations[action];
  $('confirm-object').textContent=row.name+' · '+row.id+' · '+(snapshot.engine.name||'Engine');
  $('confirm-impact').textContent=action==='remove'?'删除容器可写层，保留挂载卷和 bind 数据。':action==='pause'?'容器进程将暂停，服务可能无法响应。':'服务可能中断；取消本地等待不会回滚 Engine 已接受的操作。';
  $('confirm-submit').textContent=operations[action];$('confirm-submit').disabled=false;
  const target=$('actions');target.append(confirmPanel);confirmPanel.hidden=false;SoDeckMotion.enterMotion(confirmPanel);$('confirm-cancel').focus();
}
async function submitOperation(request) {operationSubmitting=true;renderHeader();renderListIfBrowser();try{await call({action:'operate',operation:request.action,requestId:request.requestId,intentSequence:request.intentSequence,...request.target});}finally{operationSubmitting=false;await poll(true);}}
async function openTerminal(row,shell) {
  if(!writable()||terminalOpening||snapshot.terminal?.status==='pending')return;
  terminalOpening=true;renderHeader();renderListIfBrowser();
  try {await call({action:'openTerminal',requestId:crypto.randomUUID(),shell,...identity(row)});}
  finally {terminalOpening=false;renderHeader();await poll(true);}
}
function renderListIfBrowser(){}
function appendField(target,key,value) {
  const dt=document.createElement('dt'),dd=document.createElement('dd');dt.textContent=key;dd.textContent=value;target.append(dt,dd);
}
function filters() { return {query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked}; }
async function renderBody() {
  if(!snapshot||!ready||frozen) return;
  if(rendering){renderAgain=true;return;}
  rendering=true;const generation=epoch, current=String(snapshot.revision);
  try {
    if(snapshot.kind==='container') {
      if(!$('sections').children.length) for(const [key,label] of Object.entries(sections)){
        const button=document.createElement('button');button.textContent=label;button.dataset.section=key;button.onclick=()=>{section=key;fieldOffset=0;fieldHistory.length=0;void renderBody();};$('sections').append(button);
      }
      const data=await event({action:'section',section,offset:fieldOffset,limit:50});if(generation!==epoch||frozen)return;
      fields=data.rows;fieldNext=data.nextOffset;renderFields();
      $('fields-page').textContent=data.total?String(fieldOffset+1)+'–'+(fieldOffset+fields.length)+' / '+data.total:'无记录';
      $('fields-prev').disabled=fieldOffset===0;$('fields-next').disabled=fieldNext==null;
    } else if(snapshot.kind==='resource') {
      const data=await event({action:'section',section:'basic',offset:fieldOffset,limit:50});if(generation!==epoch||frozen)return;
      $('resource-id').textContent=snapshot.resource?.id||snapshot.containerId;$('resource-fields').replaceChildren();for(const f of data.rows)appendField($('resource-fields'),f.key,f.value);
      fieldNext=data.nextOffset;$('resource-page').textContent=data.total?`${fieldOffset+1}–${fieldOffset+data.rows.length} / ${data.total}`:'无记录';$('resource-prev').disabled=!fieldOffset;$('resource-next').disabled=fieldNext==null;
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
async function poll(force=false) {
  if(!ready||frozen||!visible) return;
  const generation=epoch;
  try {const data=await event({action:'snapshot'});if(generation!==epoch||frozen)return;snapshot=data;renderHeader();if(force||revision!==String(data.revision))await renderBody();}
  catch(error){if(generation===epoch)notice(error.message||String(error));}
}
function schedule() {clearInterval(timer);timer=setInterval(()=>void poll(),1000);}
async function open(action,row) {await call({action,...identity(row),requestId:crypto.randomUUID()});}
$('detail-logs').onclick=()=>void open('openLogs',{id:snapshot.containerId});
$('refresh').onclick=()=>{fieldOffset=0;fieldHistory.length=0;void call({action:'refresh',}).then(()=>poll(true));};
$('cancel').onclick=()=>void call({action:'cancel'}).then(()=>poll(true));
$('pause').onclick=()=>void call({action:'pauseRefresh',paused:!snapshot.paused});
$('read-stats').onclick=()=>void call({action:'stats'});
$('fields-prev').onclick=()=>{fieldOffset=fieldHistory.pop()??0;void renderBody();};
$('fields-next').onclick=()=>{fieldHistory.push(fieldOffset);fieldOffset=fieldNext;void renderBody();};
$('resource-prev').onclick=()=>{fieldOffset=fieldHistory.pop()??0;void renderBody();};
$('resource-next').onclick=()=>{fieldHistory.push(fieldOffset);fieldOffset=fieldNext;void renderBody();};
$('copy-resource-id').onclick=()=>void copy(snapshot.resource?.id||snapshot.containerId);
$('show-env').onchange=renderFields;
async function copy(text){const generation=epoch;try{await sodeckClipboard.writeText(text,()=>generation===epoch&&ready&&!frozen);}catch(error){notice(error.message);}}
$('terminal-form').onsubmit=event=>{
  event.preventDefault();if($('open-terminal').disabled)return;
  void openTerminal({id:snapshot.containerId},$('terminal-shell').value);
};
$('copy-id').onclick=()=>void copy(snapshot.containerId);
for(const button of document.querySelectorAll('[data-operation]'))button.onclick=()=>beginOperation(button.dataset.operation,{id:snapshot.containerId,name:snapshot.name},button);
$('confirm-cancel').onclick=closeConfirmation;
$('confirm-form').onsubmit=e=>{e.preventDefault();if(!operationRequest||!writable())return;const request=operationRequest;closeConfirmation();void submitOperation(request);};
addEventListener('keydown',e=>{if(e.key==='Escape'){const menu=document.activeElement?.closest('details[open]');closeConfirmation();for(const details of document.querySelectorAll('details[open]'))details.open=false;menu?.querySelector('summary')?.focus();}});
addEventListener('sodeck:mount',e=>{
  closeConfirmation();epoch++;snapshot=e.detail.data;ready=false;frozen=!!e.detail.readOnly;visible=e.detail.visible!==false;revision=undefined;tokens(e.detail.tokens);
  const nav=snapshot?.navigation||{};selected=nav.selected||'';offset=nav.offset||0;section=nav.section||'basic';fieldOffset=nav.fieldOffset||0;
  for(const [history,key] of [[listHistory,'listHistory'],[fieldHistory,'fieldHistory']])history.splice(0,history.length,...(nav[key]||[]));
  restoreScroll=nav.scrollTop||0;
  containerShell=nav.shell||'';shellOptions=false;
  $('query').value=nav.query||'';$('status-filter').value=nav.status||'running';$('health-filter').value=nav.health||'all';$('group').checked=!!nav.group;
  renderHeader();sodeck.mounted();schedule();
});
addEventListener('sodeck:surfaceReady',()=>{ready=true;frozen=false;void call({action:'resumeInput'}).then(()=>poll(true));});
addEventListener('sodeck:themeChanged',e=>tokens(e.detail.tokens));
addEventListener('sodeck:visibilityChanged',e=>{visible=e.detail.visible!==false;if(visible)void poll(true);});
addEventListener('sodeck:contextChanged',()=>{closeConfirmation();selected='';epoch++;revision=undefined;void poll(true);});
addEventListener('sodeck:settingsChanged',()=>void poll(true));
addEventListener('sodeck:transferCancelled',()=>{frozen=false;void call({action:'resumeInput'}).then(()=>poll(true));});
addEventListener('sodeck:flush',async e=>{
  const writable=ready&&!frozen;frozen=true;
  try{await Promise.allSettled([...pending]);if(writable)await event({action:'navigation',state:navigation()});frozen=!!e.detail.freeze;closeConfirmation();sodeck.flushed(e.detail.id);}
  catch(error){sodeck.flushed(e.detail.id,error.message);}
});
addEventListener('sodeck:unmount',()=>{epoch++;ready=false;clearInterval(timer);});
