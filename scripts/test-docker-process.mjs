import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {pluginPeer,context,capabilities,until,packageRoot} from './docker-process.mjs';
const root=packageRoot();
const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-process-'));
const tickets=new Map(),submissions=[];let loseCommandReceipt=true;
const id='a'.repeat(64),tasks=new Map(),commands=[],diagnostics=[];let number=0,hold=false,holdMutation=false,loseReceipt=true,malformed=false,engineId='engine',peer;
const inspect={Id:id,Name:'/service-中文',Image:'sha256:'+'b'.repeat(64),Config:{Image:'demo:1',Env:['TOKEN=a=b'],Labels:{}},State:{Status:'running',Running:true,Health:{Status:'unhealthy',Log:[]}},Mounts:[],NetworkSettings:{Ports:{},Networks:{}}};
const output=command=>{
  if(command.includes("'version'"))return JSON.stringify({Server:{Os:'linux',Arch:'amd64',Version:'24.0.9'}});
  if(command.includes("'info'"))return JSON.stringify({ID:engineId,Name:'fixture',Containers:1,Images:1});
  if(command.includes("'ls'"))return id+'\n';
  if(command.includes("'inspect'")&&command.includes('"id":'))return JSON.stringify({id,name:'service-中文',image:'demo:1',status:inspect.State.Status,health:'unhealthy',exitCode:0,labels:{'com.docker.compose.project':'web','com.docker.compose.service':'api'},ports:{}});
  if(command.includes("'inspect'"))return JSON.stringify(inspect);
  if(command.includes("'stats'"))return JSON.stringify({CPUPerc:'150.5%',MemUsage:'10MiB / 1GiB',MemPerc:'1%',NetIO:'1kB / 2kB',BlockIO:'0B / 0B',PIDs:'2'});
  if(command.includes("'logs'"))return '2026-11-10T00:00:00Z alpha 中文\n'+Array.from({length:149},(_,i)=>'line '+i).join('\n')+'\noff-page UNIQUE 中文\n';
  if(command.includes("'stop'")){inspect.State.Status='exited';inspect.State.Running=false;}
  if(command.includes("'start'")||command.includes("'restart'")){inspect.State.Status='running';inspect.State.Running=true;}
  return id+'\n';
};
peer=pluginPeer(path.join(root,manifest.entry),async(method,p)=>{
  if(method==='commands.resolve')return {available:true};
  if(method==='commands.submit'){
    submissions.push(p);let receipt=tickets.get(p.requestId);
    if(!receipt){receipt={status:'pending',commandId:'command-'+p.requestId,viewId:'terminal-'+p.requestId};tickets.set(p.requestId,receipt);}
    if(loseCommandReceipt){loseCommandReceipt=false;throw Object.assign(new Error('lost command receipt'),{code:-32003});}
    return receipt;
  }
  if(method==='commands.get'){const receipt=tickets.get(p.requestId);assert(receipt);return {...receipt,status:'completed'};}
  if(method==='settings.get')return {values:{refreshSeconds:'30',logTail:'200'}};
  if(method==='session.state')return {state:'connected',context};
  if(method==='logging.write'||method==='messages.publish'){diagnostics.push({method,...p});return {};}
  if(method==='ui.updateView')return {};
  if(method==='ui.openView')return {viewId:'opened'};
  if(method==='session.exec.start'){
    const old=[...tasks.values()].find(t=>t.operationId===p.operationId);if(old)return {taskId:old.id,state:'running'};
    const taskId=String(++number),bytes=Buffer.from(output(p.command));commands.push(p.command);tasks.set(taskId,{id:taskId,operationId:p.operationId,bytes,command:p.command});
    if(loseReceipt&&p.command.includes("'stop'")){loseReceipt=false;throw Object.assign(new Error('Lost start receipt'),{code:-32003});}
    return {taskId,state:'running'};
  }
  if(method==='session.exec.poll'){
    const t=tasks.get(p.taskId);assert(t);
    if(hold||(holdMutation&&t.command.includes("'stop'")))return {state:'running',stdout:{dataBase64:'',nextOffset:'0'},stderr:{dataBase64:'',nextOffset:'0'},outputComplete:false};
    const start=Number(p.stdoutOffset),end=Math.min(start+8192,t.bytes.length);
    return {state:'completed',stdout:{dataBase64:malformed ? 'Zg=a' : t.bytes.subarray(start,end).toString('base64'),nextOffset:String(end)},stderr:{dataBase64:'',nextOffset:'0'},outputComplete:end===t.bytes.length,remoteExitConfirmed:true,exitCode:0,error:null};
  }
  if(method==='session.exec.release'){tasks.delete(p.taskId);return {};}
  throw new Error('Unexpected core call: '+method);
});
const request=(method,viewId,extra={})=>peer.request(method,{viewId,context,...extra});
const state=id=>request('ui.getView',id).then(r=>r.data);
const idle=id=>until(()=>state(id),s=>!s.busy);
try{
  await assert.rejects(peer.request('initialize',{dataDirectory:directory,capabilities:[]}),/核心缺少/);
  await peer.request('initialize',{dataDirectory:directory,capabilities});
  await request('ui.createView','browser',{viewType:'browser'});let browser=await idle('browser');assert.equal(browser.state,'ready');
  let list=(await request('ui.event','browser',{data:{action:'list',status:'abnormal',query:'中文'}})).data;
  assert.equal(list.total,1);assert.equal(list.rows[0].project,'web');
  await request('ui.createView','detail',{viewType:'container',resourceKey:JSON.stringify({id,engineId:'engine'})});await idle('detail');
  const terminal={action:'openTerminal',requestId:'terminal-once',shell:'/bin/sh'};
  await request('ui.event','detail',{data:terminal});await request('ui.event','detail',{data:terminal});
  const terminalResult=await until(async()=>(await request('ui.getView','detail')).data,v=>v.terminal?.status==='completed');
  assert.equal(terminalResult.terminal.viewId,'terminal-terminal-once');assert.equal(submissions.length,1,'lost submit receipt must query the original intent');
  assert.equal(submissions[0].name,'terminal.open.v1');assert.equal(submissions[0].sourceViewId,'detail');
  const launch=submissions[0].payload;assert.equal(launch.directoryIntegration,'off');assert(launch.launch.command.includes('DOCKER_HOST DOCKER_CONTEXT'));assert(launch.launch.command.includes("'engine'"));assert(launch.launch.command.includes(id));assert(launch.launch.command.includes("'/bin/sh'"));
  const malformedBefore=commands.length;
  malformed=true;await request('ui.event','detail',{data:{action:'refresh'}});let failed=await idle('detail');
  assert(JSON.stringify(failed).includes('核心输出编码无效'),JSON.stringify(failed));assert.equal(tasks.size,0,'bad encoding must release the task');
  assert.equal(commands.length,malformedBefore+1,'bad output must not replay a command');
  malformed=false;await request('ui.event','detail',{data:{action:'refresh'}});await idle('detail');
  const before=commands.length;for(let i=0;i<3;i++)await request('ui.getView','detail');assert.equal(commands.length,before,'mount/getView must not create commands');
  const mutation={action:'operate',requestId:'stop-once',intentSequence:'1',operation:'stop'};
  await request('ui.event','detail',{data:mutation});await request('ui.event','detail',{data:mutation});
  let detail=await idle('detail');assert.equal(detail.status,'exited');assert.equal(detail.operation.state,'succeeded');
  assert.equal(commands.filter(c=>c.includes("'stop'")).length,1);
  await assert.rejects(request('ui.event','detail',{data:{...mutation,operation:'restart'}}),/相同请求参数不同/);
  await request('ui.event','detail',{data:{action:'stats'}});await idle('detail');
  await request('ui.event','detail',{data:{action:'operate',operation:'start',requestId:'start-once',intentSequence:'2'}});await idle('detail');
  await request('ui.event','detail',{data:{action:'stats'}});detail=await idle('detail');assert.equal(detail.stats.CPUPerc,'150.5%');
  await request('ui.event','detail',{data:{action:'operate',operation:'remove',requestId:'reject-running-remove',intentSequence:'3'}});detail=await idle('detail');
  assert.equal(detail.operation.state,'failed','preflight rejection must not claim a sent mutation');assert.equal(commands.filter(c=>c.includes("'rm'")).length,0);
  await request('ui.createView','logs',{viewType:'logs',resourceKey:JSON.stringify({id,engineId:'engine'})});await idle('logs');
  const logs=(await request('ui.event','logs',{data:{action:'readLogs'}})).data;assert.equal(logs.total,151);assert(logs.rows[0].text.includes('中文'));
  const match=(await request('ui.event','logs',{data:{action:'readLogs',query:'unique',limit:100}})).data;assert.equal(match.total,1);assert(match.rows[0].text.includes('off-page'));
  const env=(await request('ui.event','detail',{data:{action:'section',section:'environment'}})).data;assert.equal(env.rows[0].value,'a=b');assert.equal(env.rows[0].secret,true);
  await assert.rejects(request('ui.event','browser',{data:{action:'setEndpoint',endpoint:'tcp://host:2375'}}),/unix/);
  holdMutation=true;await request('ui.event','detail',{data:{action:'operate',operation:'stop',requestId:'interrupted-stop',intentSequence:'4'}});
  await until(async()=>[...tasks.values()].some(t=>t.command.includes("'stop'")),Boolean);
  await request('ui.event','detail',{data:{action:'cancel'}});detail=await idle('detail');assert.equal(detail.operation.state,'unknown');holdMutation=false;
  assert(diagnostics.some(d=>d.method==='messages.publish'&&d.notify===false));assert(!JSON.stringify(diagnostics).includes('TOKEN'));
  hold=true;await request('ui.event','detail',{data:{action:'refresh'}});
  await until(async()=>tasks.size,n=>n>0);
  assert.equal((await request('ui.beforeClose','detail')).metadata.busy,true);
  await assert.rejects(request('ui.event','detail',{data:{action:'refresh'}}),/关闭/);
  await request('ui.disposeView','detail');await until(async()=>tasks.size,n=>n===0);hold=false;
  await request('ui.disposeView','logs');engineId='replacement-engine';
  await request('ui.event','browser',{data:{action:'setEndpoint',endpoint:'unix:///run/docker-test.sock'}});browser=await idle('browser');assert.equal(browser.engine.id,engineId);
  await request('ui.event','browser',{data:{action:'setEndpoint',endpoint:'unix:///var/run/docker.sock'}});browser=await idle('browser');assert.equal(browser.state,'ready');
  engineId='third-engine';await request('ui.event','browser',{data:{action:'refresh'}});browser=await idle('browser');assert.equal(browser.engine.id,engineId);
  await request('ui.disposeView','browser');
  await assert.rejects(state('detail'),/关闭/);
  await peer.close();assert.equal(peer.child.exitCode,0);
  console.log('Docker native process: UI v2, query reuse, filters, mutations once, stats, logs, freeze and actual task cleanup passed.');
}finally{
  if(peer.child.exitCode===null){peer.child.kill();await new Promise(r=>peer.child.once('exit',r));}
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-process-'));fs.rmSync(directory,{recursive:true,force:true});
}
