const labels = {
  opening:'正在启动', running:'执行中', committing:'正在提交', completed:'已结束',
  cancelled:'已取消', failed:'执行失败', unavailable:'结果不可用', unknown:'结果未知',
  rejected:'未接纳', pending:'等待接纳'
};
const errorText = error => error ? String(error.message || error.code || error) : '';
const brief = value => Array.from(value).slice(0,160).join('')+(Array.from(value).length>160?'…':'');
const time = value => Number.isFinite(value) ? new Date(value).toLocaleTimeString('zh-CN',{hour12:false}) : '时间未提供';

export function formatBytes(value) {
  if(typeof value!=='string' || !/^\d{1,20}$/.test(value))return null;
  const bytes=BigInt(value), units=['B','KiB','MiB','GiB','TiB','PiB','EiB'];
  let unit=1n,index=0;
  while(bytes>=unit*1024n && index<units.length-1){unit*=1024n;index++;}
  if(!index)return bytes+' B';
  let tenths=(bytes*10n+unit/2n)/unit;
  if(tenths>=10240n && index<units.length-1){unit*=1024n;index++;tenths=(bytes*10n+unit/2n)/unit;}
  return tenths/10n+(tenths%10n?'.'+tenths%10n:'')+' '+units[index];
}
export function outputText(page) {
  return [page?.terminal,page?.stdout,page?.stderr?'标准错误：\n'+page.stderr:''].filter(Boolean).join('\n');
}
export function preview(page) {
  const text=outputText(page);
  if(!text)return '暂时没有输出。';
  const tail=text.trimEnd().split('\n').slice(-3).join('\n'),encoder=new TextEncoder();
  let value='',bytes=0;
  for(const char of Array.from(tail).reverse()){
    const size=encoder.encode(char).length;if(bytes+size>1021)return '…'+value;
    value=char+value;bytes+=size;
  }
  return value;
}

export function presentTask(task,now) {
  const native=task.native || {},saved=task.evidence || {},download=task.kind==='download',upload=task.kind==='upload',fileTask=download || upload;
  const state=native.state || native.admission || 'pending';
  const unknown=['unknown','unavailable'].includes(state);
  const executionError=errorText(native.error), evidenceError=errorText(saved.error),cleanupError=errorText(task.cleanupError);
  const failed=['failed','rejected'].includes(state) || Boolean(native.exitCode) || Boolean(native.exitSignal) || Boolean(executionError);
  let label=labels[state] || '状态未知';
  if(download && state==='completed')label='下载已结束';
  if(task.phase==='finalizing')label='正在收尾';
  else if(task.cancelRequested && !task.done)label='已请求取消';
  if(native.exitCode!=null)label+=' · 退出码 '+native.exitCode;
  if(native.exitSignal)label+=' · 退出信号 '+native.exitSignal;
  const commit=task.file?.commitState || native.transfer?.commitState;
  if(upload)label=commit==='committed'?'已上传':commit==='unknown'?'结果待核对':commit==='notCommitted'?'未提交':state==='committing'?'正在提交':state==='running'?'上传中':label;
  const execution={label,tone:failed?'error':unknown || state==='cancelled' || (upload && ['unknown','notCommitted'].includes(commit))?'attention':'normal',error:executionError};
  const complete=task.done===true && saved.complete===true;
  const saveFailed=saved.state==='failed' || Boolean(evidenceError);
  const collecting=saved.state==='collecting' || task.phase==='collecting';
  const evidence={
    label:saveFailed?'证据保存失败':complete?download?'文件已保存':upload?'上传记录已保存':'完整输出已保存':task.phase==='finalizing'?'正在保存最终结果':task.done?'证据不完整':collecting?'正在保存证据':'证据尚未完成',
    tone:saveFailed?'error':task.done && !complete?'attention':'normal',error:evidenceError
  };
  const cleanup=cleanupError?{label:'资源清理失败',tone:'attention',error:cleanupError}:null;
  const accepted=native.launchAccepted===true || ['running','committing','completed'].includes(state) || native.remoteExitConfirmed===true || native.exitCode!=null;
  let remote=accepted?fileTask?'文件传输已接纳':'远端执行已启动':unknown?'远端接纳结果未知':native.launchQueued===true?'启动请求已排队，等待远端确认':native.execSent===true?'已请求启动，等待远端确认':'远端接纳待确认';
  if(state==='rejected')remote='远端未接纳';
  const steps=[
    {key:'recorded',label:'请求已记录',state:'observed'},
    {key:'remote',label:remote,state:unknown?'unknown':state==='rejected'?'error':accepted?'observed':'current'},
    {key:'evidence',label:evidence.label+(evidenceError?'：'+brief(evidenceError):''),state:saveFailed?'error':task.done?complete?'observed':'unknown':collecting || task.phase==='finalizing'?'current':'unknown'}
  ];
  let durationLabel='用时未提供';
  if(Number.isFinite(task.createdAt) && Number.isFinite(task.finishedAt ?? now)){
    const seconds=Math.max(0,Math.floor(((task.finishedAt ?? now)-task.createdAt)/1000));
    durationLabel=(task.done?'用时 ':'已持续 ')+(seconds>=60?Math.floor(seconds/60)+' 分 '+seconds%60:seconds)+' 秒';
  }
  const inputText=upload && task.input?task.input.localPath+' → '+task.input.remotePath:task.input?.command ?? task.input?.remotePath ?? (task.inputPreview!=null?task.inputPreview+(task.inputTruncated?'…':''):task.requestId);
  let file=null;
  if(fileTask && task.input){
    const remotePath=task.file?.remotePath ?? task.input.remotePath ?? '';
    // SFTP paths use /; backslashes can be part of a remote filename.
    const name=remotePath.split('/').pop() || '远程文件';
    const delivered=download && complete && Boolean(task.file?.localPath),bytes=native.result?.size ?? (upload?native.received:null),sizeLabel=formatBytes(bytes),totalLabel=upload?formatBytes(native.total):null;
    file={name,remotePath,delivered,sizeLabel,exactBytes:sizeLabel!==null?bytes:null,
      pathLabel:upload?'本地来源':delivered?'保存位置':'证据或残留位置',
      localPath:upload?task.file?.localPath || task.input.localPath || '尚未提供':delivered?task.file.localPath:task.evidence?.path || task.directory || '尚未交付完整文件',
      summary:(delivered?'文件已保存':execution.label+' · '+evidence.label)+(upload?' · '+(task.input.overwrite?'允许覆盖':'禁止覆盖'):'')+(sizeLabel?' · '+sizeLabel+(totalLabel?' / '+totalLabel:''):'')+' · '+durationLabel};
  }
  return {
    inputText,inputTruncated:!task.input && Boolean(task.inputTruncated),hasFullInput:Boolean(task.input),
    submittedAtLabel:time(task.createdAt),submittedAt:task.createdAt,durationLabel,
    execution,evidence,cleanup,steps,file,
    done:Boolean(task.done),download,upload,fileTask,kindLabel:upload?'上传文件':download?'下载文件':'执行命令',
    canCancel:!task.done && task.phase!=='finalizing' && !task.cancelRequested,
    requestId:task.requestId,taskId:task.taskId,
    targetLabel:task.target?(task.target.name || task.target.endpoint)+' · '+task.target.endpoint+' · '+(task.target.username || '账号未提供'):'服务器信息不可用',
    evidencePath:task.evidence?.path || task.directory || '尚未提供'
  };
}

