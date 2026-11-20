const $ = id => document.getElementById(id);
const labels = {opening:'正在启动',running:'执行中',committing:'正在提交',completed:'已结束',cancelled:'已取消',failed:'执行失败',unavailable:'结果不可用',unknown:'结果未知',rejected:'未接纳',pending:'等待接纳'};
export const input = task => task.input?.command ?? task.input?.remotePath ?? (task.inputPreview != null ? task.inputPreview + (task.inputTruncated ? '…' : '') : task.requestId);
export const time = value => value ? new Date(value).toLocaleTimeString('zh-CN',{hour12:false}) : '时间未提供';
export function duration(task, now = Date.now()) {
  if (!task.createdAt) return '用时未提供';
  const seconds = Math.max(0,Math.floor(((task.finishedAt || now)-task.createdAt)/1000));
  return (task.done?'用时 ':'已持续 ')+(seconds>=60?Math.floor(seconds/60)+' 分 '+seconds%60:seconds)+' 秒';
}
export function facts(task) {
  const native = task.native || {}, state = native.state || native.admission || 'pending';
  const problem = ['failed','unavailable','unknown','rejected'].includes(state) || Boolean(native.exitCode) || Boolean(native.exitSignal) || Boolean(task.evidence?.error) || Boolean(task.cleanupError) || (task.done && !task.evidence?.complete);
  let label = labels[state] || '状态未知';
  if (task.phase === 'finalizing') label = '正在收尾';
  else if (task.cancelRequested && !task.done) label = '已请求取消';
  if (native.exitCode != null) label += ' · 退出码 '+native.exitCode;
  if (native.exitSignal) label += ' · 退出信号 '+native.exitSignal;
  const saved = task.kind === 'download' ? '文件已保存' : '完整输出已保存';
  const evidence = task.evidence?.complete ? saved : task.evidence?.state === 'failed' || task.evidence?.error ? '证据保存失败' : task.done ? '证据不完整' : '正在保存';
  return {label,problem,evidence,cancellable:!task.done && task.phase!=='finalizing' && !task.cancelRequested};
}
export function chain(task) {
  const native = task.native || {}, state = native.state || native.admission;
  const started = native.launchAccepted === true || ['running','committing','completed'].includes(state) || native.remoteExitConfirmed === true || native.exitCode != null;
  const admission = state==='unknown'?'远端接纳结果未知':native.launchQueued===true?'启动请求已排队，等待远端确认':native.execSent===true?'启动请求已发送，等待远端确认':'远端接纳待确认';
  return [
    '请求已记录 · '+time(task.createdAt),
    started ? task.kind==='download'?'远程文件传输已接纳':'远端执行已启动' : admission,
    task.cancelRequested && !task.done ? '已请求取消，等待实际结果' : task.phase==='finalizing' ? '正在保存最终结果与清理' : facts(task).evidence
  ];
}
export function preview(page) {
  const text = [page?.terminal,page?.stdout,page?.stderr?'标准错误：\n'+page.stderr:''].filter(Boolean).join('\n');
  if (!text) return '暂时没有输出。';
  const tail = text.trimEnd().split('\n').slice(-3).join('\n');
  const encoder = new TextEncoder(); let value = '', bytes = 0;
  for (const char of Array.from(tail).reverse()) { const size = encoder.encode(char).length; if (bytes+size>1021) return '…'+value; value=char+value; bytes+=size; }
  return value;
}
export function mergeTask(previous, next) {
  if (!previous || previous.taskId !== next.taskId) return next;
  if (BigInt(previous.revision || '0') > BigInt(next.revision || '0')) return previous;
  return {...next,input:next.input ?? previous.input,directory:next.directory ?? previous.directory,file:next.file ?? previous.file,evidence:{...next.evidence,path:next.evidence?.path ?? previous.evidence?.path}};
}
export const chronological = tasks => [...tasks].sort((a,b)=>(a.createdAt||0)-(b.createdAt||0) || a.taskId.localeCompare(b.taskId));
export function setText(el, value) { value=String(value ?? ''); if(el.textContent!==value)el.textContent=value; }
function node(tag, cls, value) { const el=document.createElement(tag);if(cls)el.className=cls;if(value!=null)el.textContent=value;return el; }
function steps(el, task) {
  chain(task).forEach((value,index)=>{ if(!el.children[index])el.append(node('li'));setText(el.children[index],value); });
}
export function createRow(task, select, cancel) {
  const li=node('li','record'), outgoing=node('div','message outgoing'), sent=node('div','bubble'), kind=node('span','kind'), command=node('code','command'), date=node('time','meta');
  sent.append(kind,command);outgoing.append(sent,date);
  const incoming=node('div','message incoming'), reply=node('div','bubble'), result=node('p','result'), excerpt=node('p','preview'), process=node('ol','steps'), actions=node('div','actions'), button=node('button','text-button',task.kind==='download'?'查看文件信息':'查看输出'), stop=node('button','text-button cancel','请求取消'), meta=node('span','meta');
  button.type=stop.type='button';button.setAttribute('aria-controls','detail');button.onclick=()=>select(task.taskId,button);stop.onclick=()=>cancel(task.taskId,stop);
  actions.append(button,stop);reply.append(result,excerpt,process,actions);incoming.append(reply,meta);li.append(outgoing,incoming);li.dataset.taskId=task.taskId;
  return {li,kind,command,date,result,excerpt,process,button,stop,meta};
}
export function updateRow(row, task, cached) {
  const value=facts(task);
  row.li.classList.toggle('problem',value.problem);
  setText(row.kind,task.kind==='download'?'下载文件':'执行命令');setText(row.command,input(task));setText(row.date,time(task.createdAt));
  if(task.createdAt)row.date.dateTime=new Date(task.createdAt).toISOString();
  setText(row.result,value.label);setText(row.meta,duration(task)+' · '+value.evidence);
  setText(row.excerpt,task.kind==='download'?(task.inputPreview?.split(/[\\/]/).pop() || '下载远程文件'):cached?.text || '输出预览尚未读取');
  row.process.hidden=Boolean(task.done);if(!task.done)steps(row.process,task);
  row.stop.hidden=!value.cancellable;row.stop.setAttribute('aria-label','请求取消 '+task.requestId);
  const name=input(task).replace(/\s+/g,' ').trim(), short=Array.from(name).slice(0,120).join('');
  row.button.setAttribute('aria-label','查看'+(task.kind==='exec'?'命令':'下载')+'详情 '+task.requestId+' · '+(task.target?.name || task.target?.endpoint || '服务器信息待确认')+' · '+value.label+' · '+short+(name.length>short.length?'…':''));
}
export function paintDetail(task) {
  const value=facts(task), download=task.kind==='download', target=task.target;
  setText($('detail-title'),download?'文件详情':'命令详情');setText($('detail-command'),task.input?input(task):'正在读取完整详情…');
  setText($('detail-state'),value.label);$('detail-state').dataset.problem=String(value.problem);setText($('detail-time'),duration(task));
  $('command-info').hidden=download;$('file-receipt').hidden=!download;$('detail-output-section').hidden=download;
  setText($('detail-evidence'),value.evidence);$('detail-cancel').hidden=!value.cancellable;$('detail-cancel').setAttribute('aria-label','请求取消 '+task.requestId);
  $('detail-evidence').hidden=download;
  setText($('detail-server'),target?(target.name || target.endpoint)+' · '+target.endpoint+' · '+(target.username || '账号未提供'):'服务器信息不可用');
  setText($('detail-created'),'提交于 '+time(task.createdAt));setText($('detail-request'),task.requestId);setText($('detail-id'),task.taskId);setText($('detail-path'),task.evidence?.path || task.directory || '尚未提供');
  const errors=[task.native?.error,task.evidence?.error,task.cleanupError].filter(Boolean).map(e=>e.message || String(e));setText($('detail-error'),errors.join('\n'));$('detail-error').hidden=!errors.length;steps($('detail-chain'),task);
  if(download) {
    const remote=task.file?.remotePath ?? task.input?.remotePath ?? '';
    const delivered=Boolean(task.done && task.evidence?.complete && task.file?.localPath);
    const bytes=task.native?.bytesCopied ?? task.native?.completedBytes;
    setText($('file-name'),remote.split(/[\\/]/).pop() || '远程文件');
    setText($('file-summary'),(delivered?'文件已保存':value.label+' · '+value.evidence)+(bytes!=null?' · '+bytes+' 字节':'')+' · '+duration(task));
    setText($('file-remote'),remote || '尚未提供');setText($('file-path-label'),delivered?'保存位置':'证据或残留位置');setText($('file-local'),delivered?task.file.localPath:task.evidence?.path || task.directory || '尚未交付完整文件');
    $('detail-state').hidden=true;$('detail-time').hidden=true;
  } else { $('detail-state').hidden=false;$('detail-time').hidden=false; }
}

