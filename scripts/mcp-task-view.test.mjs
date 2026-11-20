import assert from 'node:assert/strict';
import test from 'node:test';
import {facts,chain,preview,mergeTask,chronological} from '../plugins/mcp/ui/task-view.js';

test('task feedback preserves execution, saving, cancellation and cleanup facts',()=>{
  const task={taskId:'opaque',native:{state:'completed',exitCode:7},done:true,evidence:{complete:true}};
  assert(facts(task).problem);assert.match(facts(task).label,/退出码 7/);assert.equal(facts(task).evidence,'完整输出已保存');
  assert.match(facts({...task,done:false,cancelRequested:true}).label,/已请求取消/);
  assert(!facts({...task,done:false,cancelRequested:true}).cancellable);
  assert.match(facts({...task,done:false,phase:'finalizing'}).label,/收尾/);
  assert(facts({...task,native:{state:'completed',exitCode:0},cleanupError:{message:'release failed'}}).problem);
  assert.match(chain({...task,native:{admission:'unknown'}})[1],/结果未知/);
  assert(!chain({...task,native:{admission:'pending'}}).some(value=>value.includes('已启动')));
  const queued=chain({...task,native:{state:'opening',execSent:true,launchQueued:true,launchAccepted:false}});
  assert.match(queued[1],/排队，等待远端确认/);assert(!queued.some(value=>value.includes('已启动')));
  assert.match(chain({...task,native:{state:'opening',execSent:true,launchAccepted:true}})[1],/已启动/);
  const requested=chain({...task,native:{state:'opening',execSent:true,launchQueued:false,launchAccepted:false}});assert.match(requested[1],/已请求启动，等待远端确认/);assert(!requested[1].includes('已发送'));
  const signalled=facts({...task,native:{state:'completed',exitSignal:'TERM'}});assert(signalled.problem);assert.match(signalled.label,/退出信号 TERM/);
});

test('output preview bounds UTF-8 bytes and lines before DOM insertion',()=>{
  const value=preview({stdout:'old\n'+('中'.repeat(500))+'\nlast\nend'});
  assert(Buffer.byteLength(value,'utf8')<=1024);assert(value.split('\n').length<=3);assert(!value.includes('\ufffd'));
  assert.equal(preview({stdout:'1\n2\n3\n4'}),'2\n3\n4');
  assert.match(preview({stderr:'permission denied'}),/标准错误/);
  assert.equal(preview({}),'暂时没有输出。');
  assert.equal(preview({terminal:'PTY prompt\nanswer'}),'PTY prompt\nanswer');
});

test('reading facts never regress revisions or discard loaded full input',()=>{
  const prior={taskId:'a',revision:'9007199254740993',input:{command:'full command'},evidence:{path:'/evidence/a'}};
  assert.equal(mergeTask(prior,{taskId:'a',revision:'9007199254740992'}),prior);
  const merged=mergeTask(prior,{taskId:'a',revision:'9007199254740994',done:true,evidence:{complete:true}});
  assert.equal(merged.input.command,'full command');assert.equal(merged.evidence.path,'/evidence/a');assert(merged.evidence.complete);
  assert.deepEqual(chronological([{taskId:'b',createdAt:2,done:false},{taskId:'c',createdAt:1,done:true},{taskId:'a',createdAt:2,done:true}]).map(t=>t.taskId),['c','a','b']);
});
