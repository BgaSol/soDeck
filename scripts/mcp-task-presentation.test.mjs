import assert from 'node:assert/strict';
import test from 'node:test';
import {presentTask,formatBytes,outputText,preview} from '../plugins/mcp/ui/task-presentation.js';

const now=1700000005000;
const task=(changes={})=>({taskId:'opaque',requestId:'request',kind:'exec',createdAt:now-5000,finishedAt:now,done:true,phase:'done',native:{state:'completed',exitCode:0},evidence:{state:'saved',complete:true},...changes});
const show=value=>presentTask(value,now);

test('execution, evidence and cleanup keep independent labels and emphasis',()=>{
  let value=show(task({native:{state:'completed',exitCode:7}}));
  assert.equal(value.execution.tone,'error');assert.match(value.execution.label,/退出码 7/);assert.equal(value.evidence.tone,'normal');assert.equal(value.evidence.label,'完整输出已保存');
  value=show(task({evidence:{state:'failed',complete:false,error:{message:'disk full'}}}));
  assert.equal(value.execution.tone,'normal');assert.match(value.execution.label,/退出码 0/);assert.equal(value.evidence.tone,'error');assert.equal(value.evidence.error,'disk full');
  value=show(task({cleanupError:{message:'release failed'}}));
  assert.equal(value.execution.tone,'normal');assert.equal(value.evidence.tone,'normal');assert.equal(value.cleanup.label,'资源清理失败');assert.equal(value.cleanup.error,'release failed');
  value=show(task({native:{state:'completed',exitSignal:'TERM'}}));
  assert.equal(value.execution.tone,'error');assert.match(value.execution.label,/退出信号 TERM/);
});

test('requested, queued, accepted and unknown are distinct observations',()=>{
  const running=native=>show(task({done:false,phase:'collecting',finishedAt:null,evidence:{state:'collecting',complete:false},native}));
  let value=running({state:'opening',execSent:true,launchQueued:false,launchAccepted:false});
  assert.match(value.steps[1].label,/已请求启动，等待远端确认/);assert.equal(value.steps[1].state,'current');
  assert(!value.steps.some(step=>step.label.includes('已启动')));
  value=running({state:'opening',execSent:true,launchQueued:true,launchAccepted:false});assert.match(value.steps[1].label,/排队，等待远端确认/);
  value=running({state:'running',launchAccepted:true});assert.equal(value.steps[1].state,'observed');assert.match(value.steps[1].label,/已启动/);
  value=running({state:'unavailable',lastResult:{state:'running',launchAccepted:true}});assert.equal(value.steps[1].state,'unknown');assert.match(value.steps[1].label,/未知/);
  value=running({state:'rejected'});assert.equal(value.steps[1].state,'error');assert(!value.steps.some(step=>step.label.includes('已启动')));
  assert.equal(value.steps[0].label,'请求已记录');assert(!value.steps[0].label.includes(':'));
});

test('cancellation and finalization do not imply a remote terminal result',()=>{
  const active=task({done:false,finishedAt:null,phase:'collecting',native:{state:'running'},evidence:{state:'collecting',complete:false}});
  let value=show({...active,cancelRequested:true});assert.equal(value.canCancel,false);assert.equal(value.execution.label,'已请求取消');assert.equal(value.durationLabel,'已持续 5 秒');
  value=show({...active,phase:'finalizing'});assert.equal(value.canCancel,false);assert.equal(value.execution.label,'正在收尾');assert.match(value.evidence.label,/保存最终结果/);
  value=show({...active,evidence:{complete:true}});assert.notEqual(value.evidence.label,'完整输出已保存','complete without stable done is not a final delivery receipt');
  value=show(task({native:{},evidence:{}}));assert(!value.execution.label.includes('成功'));assert.equal(value.evidence.label,'证据不完整');
});

test('IEC sizes keep decimal precision without converting large bytes to Number',()=>{
  for(const [bytes,label]of [['0','0 B'],['1023','1023 B'],['1024','1 KiB'],['25190','24.6 KiB'],['1048575','1 MiB'],['9007199254740993','8 PiB'],['18446744073709551615','16 EiB']])assert.equal(formatBytes(bytes),label);
  for(const value of [undefined,null,'','-1','1.2','1x',1024,'9'.repeat(21)])assert.equal(formatBytes(value),null);
});

test('file receipt uses actual delivery and keeps literal remote filenames',()=>{
  const file=task({kind:'download',input:{remotePath:'/var/log/a\\b.log'},file:{remotePath:'/var/log/a\\b.log',localPath:'/evidence/download.bin'},native:{state:'completed',result:{size:'25190'}}});
  let value=show(file);assert.equal(value.file.name,'a\\b.log');assert(value.file.delivered);assert.equal(value.file.sizeLabel,'24.6 KiB');assert.equal(value.file.exactBytes,'25190');assert.match(value.file.summary,/文件已保存 · 24.6 KiB · 用时 5 秒/);
  value=show({...file,evidence:{state:'failed',complete:false,path:'/evidence/partial',error:{message:'disk full'}}});assert(!value.file.delivered);assert.equal(value.file.localPath,'/evidence/partial');assert.match(value.file.summary,/证据保存失败/);
  value=show({...file,native:{state:'completed',result:{size:'unknown'}}});assert.equal(value.file.sizeLabel,null);assert.equal(value.file.exactBytes,null);assert(!value.file.summary.includes('字节'));
});

test('upload commit facts stay separate from record saving and source delivery',()=>{
  const upload=task({kind:'upload',input:{localPath:'C:/work/file.bin',remotePath:'/tmp/file.bin',overwrite:true},file:{commitState:'committed',localPath:'C:/work/file.bin'},native:{state:'completed',received:'25190',total:'25190'},evidence:{state:'failed',error:{message:'disk full'},complete:false}});
  let value=show(upload);assert(value.fileTask);assert(value.upload);assert.equal(value.execution.label,'已上传');assert.equal(value.execution.tone,'normal');assert.equal(value.evidence.tone,'error');assert.equal(value.file.pathLabel,'本地来源');assert(!value.file.delivered);assert.match(value.file.summary,/已上传 · 证据保存失败 · 允许覆盖 · 24.6 KiB \/ 24.6 KiB/);assert.equal(value.inputText,'C:/work/file.bin → /tmp/file.bin');
  value=show({...upload,file:{...upload.file,commitState:'unknown'}});assert.equal(value.execution.label,'结果待核对');assert.equal(value.execution.tone,'attention');
  value=show({...upload,file:{...upload.file,commitState:'notCommitted'},native:{state:'failed',error:{message:'exists'}}});assert.equal(value.execution.label,'未提交');assert.equal(value.execution.tone,'error');
});

test('task presentation is a pure current projection with a supplied clock',()=>{
  const original=task({input:{command:'printf hello'},target:{name:'prod',endpoint:'192.0.2.1:22',username:'ops'}});
  const before=structuredClone(original);Object.freeze(original.native);Object.freeze(original.evidence);Object.freeze(original);
  const value=show(original);assert.deepEqual(original,before);assert.equal(value.inputText,'printf hello');assert.equal(value.hasFullInput,true);assert.equal(value.durationLabel,'用时 5 秒');
  const active=show(task({done:false,finishedAt:null,inputPreview:'journal',inputTruncated:true}));assert.equal(active.inputText,'journal…');assert(active.inputTruncated);assert.equal(active.hasFullInput,false);
});

test('all output sources share formatting and previews bound UTF-8 before DOM insertion',()=>{
  assert.equal(outputText({terminal:'PTY prompt\nanswer'}),'PTY prompt\nanswer');
  assert.equal(outputText({stdout:'out',stderr:'err'}),'out\n标准错误：\nerr');
  const value=preview({stdout:'old\n'+('中'.repeat(500))+'\nlast\nend'});
  assert(Buffer.byteLength(value,'utf8')<=1024);assert(value.split('\n').length<=3);assert(!value.includes('\ufffd'));assert(value.endsWith('last\nend'));
  assert.equal(preview({stdout:'1\n2\n3\n4'}),'2\n3\n4');assert.match(preview({stderr:'permission denied'}),/标准错误/);assert.equal(preview({}),'暂时没有输出。');
});

