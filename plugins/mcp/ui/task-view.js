const $ = id => document.getElementById(id);
function text(el,value){value=String(value ?? '');if(el.textContent!==value)el.textContent=value;}
function node(tag,cls,value){const el=document.createElement(tag);if(cls)el.className=cls;if(value!=null)el.textContent=value;return el;}
function fact(el,value){text(el,value?.label);el.dataset.tone=value?.tone || 'normal';}
function steps(el,values){
  values.forEach((value,index)=>{
    if(!el.children[index])el.append(node('li'));
    const item=el.children[index];text(item,value.label);item.dataset.state=value.state;
  });
}
function issue(el,value,showLabel=true){
  text(el,value?(showLabel?value.label+(value.error?'：'+value.error:''):value.error):'');
  el.hidden=!value || (!showLabel && !value.error);el.dataset.tone=value?.tone || 'normal';
}
export function createRow(taskId,select,cancel){
  const li=node('li','record'),outgoing=node('div','message outgoing'),sent=node('div','bubble'),kind=node('span','kind'),command=node('code','command'),date=node('time','meta');
  sent.append(kind,command);outgoing.append(date,sent);
  const incoming=node('div','message incoming'),reply=node('div','bubble'),result=node('p','result'),excerpt=node('p','preview'),process=node('ol','steps'),issues=node('div','task-issues');
  const executionIssue=node('p'),evidenceIssue=node('p'),cleanupIssue=node('p');
  issues.append(executionIssue,evidenceIssue,cleanupIssue);
  const actions=node('div','actions'),button=node('button','text-button'),stop=node('button','text-button cancel','请求取消'),meta=node('span','meta'),duration=node('span'),receipt=node('span');
  meta.append(duration,receipt);button.type=stop.type='button';button.setAttribute('aria-controls','detail');
  button.onclick=()=>select(taskId,button);stop.onclick=()=>cancel(taskId,stop);
  actions.append(button,stop);reply.append(result,excerpt,process,issues,actions);incoming.append(reply,meta);li.append(outgoing,incoming);li.dataset.taskId=taskId;
  return {li,kind,command,date,result,excerpt,process,executionIssue,evidenceIssue,cleanupIssue,button,stop,duration,receipt};
}
export function updateRow(row,model,cached){
  const execution=model.execution,evidence=model.evidence;
  row.li.dataset.tone=execution.tone;
  text(row.kind,model.kindLabel);text(row.command,model.inputText);text(row.date,model.submittedAtLabel);
  if(Number.isFinite(model.submittedAt))row.date.dateTime=new Date(model.submittedAt).toISOString();
  fact(row.result,execution);text(row.duration,model.durationLabel);
  const receipt=model.done && evidence.tone==='normal';
  row.receipt.hidden=!receipt;text(row.receipt,receipt?' · '+evidence.label:'');
  text(row.excerpt,model.fileTask?(model.inputText?.split('/').pop() || '远程文件'):cached?.text || '输出预览尚未读取');
  row.process.hidden=model.done;if(!model.done)steps(row.process,model.steps);
  issue(row.executionIssue,execution,false);
  issue(row.evidenceIssue,model.done && evidence.tone!=='normal'?evidence:null);
  issue(row.cleanupIssue,model.cleanup);
  text(row.button,model.fileTask?'查看文件信息':'查看输出');
  row.stop.hidden=!model.canCancel;row.stop.setAttribute('aria-label','请求取消 '+model.requestId);
  const name=model.inputText.replace(/\s+/g,' ').trim(),short=Array.from(name).slice(0,120).join('');
  row.button.setAttribute('aria-label','查看'+(model.upload?'上传':model.download?'下载':'命令')+'详情 '+model.requestId+' · '+model.targetLabel+' · '+execution.label+' · '+short+(name.length>short.length?'…':''));
}
export function paintDetail(model){
  text($('detail-title'),model.fileTask?'文件详情':'命令详情');text($('detail-command'),model.hasFullInput?model.inputText:'正在读取完整详情…');
  fact($('detail-state'),model.execution);text($('detail-time'),model.durationLabel);
  $('command-info').hidden=model.fileTask;$('file-receipt').hidden=!model.fileTask;$('detail-output-section').hidden=model.fileTask;
  $('detail-state').hidden=model.fileTask;$('detail-time').hidden=model.fileTask;
  $('detail-cancel').hidden=!model.canCancel;$('detail-cancel').setAttribute('aria-label','请求取消 '+model.requestId);
  issue($('detail-error'),model.execution,false);
  issue($('detail-evidence'),model.evidence,!model.fileTask);
  issue($('detail-cleanup'),model.cleanup);
  text($('detail-server'),model.targetLabel);text($('detail-created'),'提交于 '+model.submittedAtLabel);
  text($('detail-request'),model.requestId);text($('detail-id'),model.taskId);text($('detail-path'),model.evidencePath);steps($('detail-chain'),model.steps);
  $('detail-size-field').hidden=$('detail-size').hidden=!model.file?.exactBytes;
  if(model.file){
    const file=model.file;text($('file-name'),file.name);text($('file-summary'),file.summary);$('file-summary').dataset.tone=model.evidence.tone==='normal'?model.execution.tone:model.evidence.tone;
    text($('file-remote'),file.remotePath || '尚未提供');text($('file-path-label'),file.pathLabel);text($('file-local'),file.localPath);
    text($('detail-size'),file.exactBytes+' 字节');
  }
}

