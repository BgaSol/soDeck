import {createRow,updateRow,paintDetail} from './task-view.js';
import {presentTask,preview} from './task-presentation.js';
import {createOutputReader} from './output-reader.js';
import {createConfigPopover} from './config-popover.js';
const $=id=>document.getElementById(id), pane=$('content');
const event=async data=>{const value=await sodeck.request('event',data);return value.data ?? value;};
let mounted=false,visible=false,epoch=0,timer,pending=false,again=false,snapshot,selected,returnPosition,config,observer,factSequence=0,reading=0,rotation=0,initial=true;
const rows=new Map(),previews=new Map(),inView=new Set(),animations=new Set();
const owner=()=>mounted && visible?epoch:null;
function setText(el,value){value=String(value ?? '');if(el.textContent!==value)el.textContent=value;}
function mergeTask(previous,next){
  if(!previous || previous.taskId!==next.taskId)return next;
  if(BigInt(previous.revision || '0')>BigInt(next.revision || '0'))return previous;
  return {...next,input:next.input ?? previous.input,directory:next.directory ?? previous.directory,file:next.file ?? previous.file,evidence:{...next.evidence,path:next.evidence?.path ?? previous.evidence?.path}};
}
const chronological=tasks=>[...tasks].sort((a,b)=>(a.createdAt||0)-(b.createdAt||0) || a.taskId.localeCompare(b.taskId));
function notice(value){setText($('notice'),value);$('notice').hidden=!value;}
function animate(el,offset=4) {
  if(owner()===null || matchMedia('(prefers-reduced-motion: reduce)').matches)return;
  const value=el.animate([{opacity:.5,transform:'translateY('+offset+'px)'},{opacity:1,transform:'none'}],{duration:180,easing:'cubic-bezier(.16,1,.3,1)'});
  animations.add(value);value.finished.catch(()=>{}).finally(()=>animations.delete(value));
}
function stopAnimations(){for(const value of animations)value.cancel();animations.clear();}
function theme(tokens){for(const [key,value]of Object.entries(tokens || {}))document.documentElement.style.setProperty(key==='color-scheme'?key:'--'+key,value);}
function belongs(task){return task && task.runId===snapshot?.runId && task.targetId===snapshot?.targetId;}
function accept(task) {
  if(!selected || task.taskId!==selected.taskId || !belongs(task))return;
  selected=mergeTask(selected,task);paintDetail(presentTask(selected,Date.now()));reader.update(selected);
}
const reader=createOutputReader({request:event,valid:()=>selected?owner():null,accept,onBusy:value=>{reading=Math.max(0,reading+(value?1:-1));}});
function closeDetail(restore=false) {
  factSequence++;reader.close();selected=null;$('detail').hidden=true;$('overview').hidden=false;$('evidence-details').open=false;
  for(const row of rows.values())row.button.removeAttribute('aria-current');
  if(restore) {
    const row=rows.get(returnPosition?.taskId),button=row?.button;
    pane.scrollTop=returnPosition?.scrollTop || 0;
    if(button?.isConnected) {pane.scrollTop+=button.getBoundingClientRect().top-returnPosition.top;button.focus({preventScroll:true});}
    else $('refresh').focus({preventScroll:true});
    animate($('overview'),-4);
  } else pane.scrollTop=0;
  returnPosition=null;
}
function reset() {
  epoch++;config?.close(false,false);closeDetail();previews.clear();inView.clear();observer?.disconnect();
  for(const row of rows.values())row.li.remove();rows.clear();initial=true;$('new-tasks').hidden=true;$('directory-details').open=false;
}
function anchor() {
  const top=pane.getBoundingClientRect().top;
  for(const [id,row]of rows)if(row.li.getBoundingClientRect().bottom>top)return {id,top:row.li.getBoundingClientRect().top};
}
function render(data) {
  if(typeof data?.runId!=='string' || !data.runId || typeof data.targetId!=='string' || !data.targetId || !data.target)throw new Error('目标状态快照无效，请刷新。');
  const changed=snapshot && (data.runId!==snapshot.runId || data.targetId!==snapshot.targetId);
  if(changed)reset();
  const previous=new Map((snapshot?.tasks || []).map(task=>[task.taskId,task]));
  const tasks=chronological((data.tasks || []).map(task=>changed?task:mergeTask(previous.get(task.taskId),task)));
  const position=!selected?anchor():null, scroll=pane.scrollTop, bottom=pane.scrollHeight-pane.clientHeight-scroll<48;
  const added=tasks.some(task=>!rows.has(task.taskId)),entering=[],mayEnter=!initial && !selected && bottom,now=Date.now();
  snapshot={...data,tasks};config.update(data.endpoint);
  const target=data.target.descriptor,error=data.target.error,availability={ready:'已连接',disconnected:'已断开',closing:'正在关闭',stale:'连接已失效',unavailable:'暂不可用'}[data.target.availability] || '状态未知';
  setText($('server'),target?target.name || target.endpoint:error?'目标信息暂不可用':'服务器未连接');
  setText($('connection'),availability);$('connection').dataset.ready=String(data.target.availability==='ready');
  setText($('server-detail'),error?'目标信息读取失败：'+(error.message || error.code):target?(target.username || '账号未提供')+'@'+target.endpoint+(data.target.targetSource==='lastConnected'?' · 最后连接信息':''):'请在 SoDeck 中连接服务器。');
  setText($('service'),error?'目标信息读取失败：'+(error.message || error.code):data.service==='ready'?'':data.service);$('service').hidden=!error && data.service==='ready';
  setText($('directory'),data.evidenceDirectory);
  const ids=new Set(tasks.map(task=>task.taskId));
  for(const [id,row]of rows)if(!ids.has(id)){observer?.unobserve(row.li);row.li.remove();rows.delete(id);previews.delete(id);inView.delete(id);}
  tasks.forEach((task,index)=>{
    let row=rows.get(task.taskId);
    if(!row){row=createRow(task.taskId,selectTask,cancel);rows.set(task.taskId,row);observer?.observe(row.li);if(mayEnter && entering.length<4)entering.push(row.li);}
    if($('tasks').children[index]!==row.li)$('tasks').insertBefore(row.li,$('tasks').children[index] || null);
    updateRow(row,presentTask(task,now),previews.get(task.taskId));if(selected?.taskId===task.taskId)row.button.setAttribute('aria-current','true');
  });
  $('empty').hidden=Boolean(tasks.length);
  const active=tasks.filter(task=>!task.done).length,total=Math.max(data.recordCount ?? tasks.length,tasks.length);
  setText($('task-count'),(total>tasks.length?'显示 '+tasks.length+' / '+total+' 条':tasks.length+' 条操作')+(active?' · '+active+' 条进行中':''));
  if(selected) {
    const task=tasks.find(task=>task.taskId===selected.taskId);
    if(task)selected=mergeTask(selected,task);
    paintDetail(presentTask(selected,Date.now()));reader.update(selected);
  } else if(initial || bottom) {
    pane.scrollTop=pane.scrollHeight;$('new-tasks').hidden=true;initial=false;
  } else {
    const row=rows.get(position?.id);pane.scrollTop=row?scroll+row.li.getBoundingClientRect().top-position.top:scroll;
    if(added)$('new-tasks').hidden=false;
  }
  for(const el of entering){const box=el.getBoundingClientRect(),viewport=pane.getBoundingClientRect();if(box.bottom>viewport.top && box.top<viewport.bottom)animate(el,0);}
}
function selectTask(id,button) {
  const task=snapshot?.tasks.find(value=>value.taskId===id);if(!task)return;
  notice('');
  config.close(false,false);$('server-info').open=false;$('directory-details').open=false;
  returnPosition={taskId:id,scrollTop:pane.scrollTop,top:button.getBoundingClientRect().top};
  factSequence++;reader.close();selected=task;$('overview').hidden=true;$('detail').hidden=false;$('new-tasks').hidden=true;
  paintDetail(presentTask(task,Date.now()));pane.scrollTop=0;$('detail-close').focus({preventScroll:true});reader.open(task);animate($('detail'),4);
  if(task.kind!=='exec')void loadFacts();
}
async function loadFacts() {
  if(!selected || owner()===null)return;
  const context=owner(),id=selected.taskId,seq=++factSequence;
  try {
    const value=await event({action:'task',taskId:id,outputMode:'none'});
    if(context===owner() && selected?.taskId===id && seq===factSequence){accept(value);if($('freshness').textContent==='任务事实可能已过期')$('freshness').hidden=true;}
  } catch(error) {if(context===owner() && selected?.taskId===id && seq===factSequence){notice('任务状态读取失败：'+(error.message || error));setText($('freshness'),'任务事实可能已过期');$('freshness').hidden=false;}}
}
async function loadPreviews() {
  const candidates=snapshot.tasks.filter(task=>task.kind==='exec' && inView.has(task.taskId) && previews.get(task.taskId)?.bytes!==task.outputInfo?.savedBytes);
  const rotate=list=>list.length?[...list.slice(rotation%list.length),...list.slice(0,rotation%list.length)]:[];
  const active=rotate(candidates.filter(task=>!task.done)),done=rotate(candidates.filter(task=>task.done));
  const batch=[...active.slice(0,done.length?3:4),...done.slice(0,Math.max(1,4-active.length))].slice(0,4);rotation++;
  for(const task of batch) {
    if(owner()===null || selected || reading)return;
    const context=owner();
    try {
      const value=await event({action:'task',taskId:task.taskId,tail:true});
      if(context!==owner() || selected || !belongs(value) || !rows.has(task.taskId))return;
      const cached={text:preview(value.output),bytes:value.output?.savedBytes ?? value.outputInfo?.savedBytes};
      previews.set(task.taskId,cached);updateRow(rows.get(task.taskId),presentTask(snapshot.tasks.find(t=>t.taskId===task.taskId),Date.now()),cached);
    } catch(error) {
      if(context===owner() && !selected && rows.has(task.taskId)){const cached={text:previews.get(task.taskId)?.text || '预览读取失败，请刷新重试。'};previews.set(task.taskId,cached);updateRow(rows.get(task.taskId),presentTask(task,Date.now()),cached);}
    }
  }
}
async function refresh(manual=false) {
  if(owner()===null)return;
  if(pending){if(manual)again=true;return;}
  clearTimeout(timer);pending=true;const context=owner();
  try {
    const result=await event({action:'snapshot'});
    if(context!==owner())return;
    render(result);$('freshness').hidden=true;
    if(selected)await loadFacts();else await loadPreviews();
  } catch(error) {if(context===owner()){notice(error.message || String(error));setText($('freshness'),'状态可能已过期');$('freshness').hidden=false;}}
  finally {pending=false;if(owner()!==null){timer=setTimeout(()=>void refresh(),again || context!==owner()?0:1500);again=false;}}
}
async function cancel(id,button) {
  const task=selected?.taskId===id?selected:snapshot?.tasks.find(task=>task.taskId===id);if(!task || owner()===null)return;
  const context=owner();button.disabled=true;
  try {const result=await event({action:'cancel',taskId:id});if(context===owner()){if(result.result)accept(result.result);notice(result.cancelRequested?'已请求取消，等待远端实际结果。':'操作已进入收尾或结束，请查看实际结果。');void refresh(true);}}
  catch(error){if(context===owner())notice(error.message || String(error));}
  finally {button.disabled=false;}
}
$('detail-close').onclick=()=>{closeDetail(true);void refresh(true);};
$('detail-cancel').onclick=()=>cancel(selected?.taskId,$('detail-cancel'));
$('refresh').onclick=()=>{void refresh(true);void config.readSettings();};
$('new-tasks').onclick=()=>{pane.scrollTop=pane.scrollHeight;$('new-tasks').hidden=true;};
pane.addEventListener('scroll',()=>{if(pane.scrollHeight-pane.clientHeight-pane.scrollTop<48)$('new-tasks').hidden=true;},{passive:true});
$('reveal').onclick=async()=>{try{await sodeck.request('data',{op:'pluginData',action:'reveal',relative:['evidence']});}catch(error){notice(error.message || String(error));}};
const motion=matchMedia('(prefers-reduced-motion: reduce)');motion.addEventListener('change',stopAnimations);
addEventListener('sodeck:mount',({detail})=>{
  epoch++;mounted=true;visible=detail.visible!==false;theme(detail.tokens);
  config ||= createConfigPopover({request:event,host:(method,params)=>sodeck.request(method,params),owner});
  observer=new IntersectionObserver(entries=>{for(const entry of entries){const id=entry.target.dataset.taskId;if(entry.isIntersecting)inView.add(id);else inView.delete(id);}},{root:pane});
  for(const row of rows.values())observer.observe(row.li);
  if(detail.data)render(detail.data);config.settings(detail.settings);sodeck.prepared();void refresh();void config.readSettings();
});
addEventListener('sodeck:themeChanged',({detail})=>{stopAnimations();config?.finishMotion();theme(detail.tokens);});
addEventListener('sodeck:settingsChanged',({detail})=>config?.settings(detail.settings));
addEventListener('sodeck:visibilityChanged',({detail})=>{
  epoch++;visible=detail.visible;clearTimeout(timer);config?.stop();stopAnimations();
  if(visible){reader.resume();void refresh(true);void config.readSettings();}else reader.suspend();
});
addEventListener('sodeck:unmount',()=>{
  epoch++;mounted=visible=false;clearTimeout(timer);config?.destroy();config=null;observer?.disconnect();observer=null;inView.clear();stopAnimations();reader.close();closeDetail();
});

