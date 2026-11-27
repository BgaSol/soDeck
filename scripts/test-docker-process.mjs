import {withProcessFacts,processFacts} from './process-fixture.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {pluginPeer,context,until,packageRoot} from './docker-process.mjs';
const windows=process.argv.includes('--windows')||process.argv.includes('--windows-engine'),engineOs=process.argv.includes('--windows-engine')?'windows':'linux',shell=engineOs==='windows'?'cmd.exe':'/bin/sh';
const environment=windows?{kind:'windows',launcher:'powershell.exe',exitMode:'native'}:{kind:'posix'},rootDir=engineOs==='windows'?'C:\\ProgramData\\Docker':'/srv/docker-data';
const root=packageRoot();
const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-process-'));
const directoryRecords=new Map(),directoryWrites=[];let loseDirectoryReply=true;
const tickets=new Map(),submissions=[],streams=new Map(),streamRequests=[],streamLookups=[];let loseCommandReceipt=true,loseStreamReceipts=2,tunnelOutcome='accepted';
const id='a'.repeat(64),tasks=new Map(),commands=[],diagnostics=[];let number=0,hold=false,holdMutation=false,loseReceipt=true,malformed=false,engineId='engine',peer;
const programs=new Map(),streamBodies=new Map();
const inspect={Id:id,Name:'/service-中文',Image:'sha256:'+'b'.repeat(64),Config:{Image:'demo:1',Env:['TOKEN=a=b'],Labels:{}},State:{Status:'running',Running:true,Health:{Status:'unhealthy',Log:[]}},Mounts:[],NetworkSettings:{Ports:{'80/tcp':[{HostIp:'0.0.0.0',HostPort:'8080'},{HostIp:'::',HostPort:'8080'}]},Networks:{}}};
const unusedImage='sha256:'+'d'.repeat(64),networkId='c'.repeat(64),systemNetwork='e'.repeat(64);
let volumeExists=true,volumeCreatedAt='2026-11-16T00:00:00Z',holdPull=false,streamConfirmed=false;
const volume=name=>({Name:name,Driver:'local',Scope:'local',CreatedAt:volumeCreatedAt,Options:null,Labels:null});
const output=command=>{
  if(command.includes("'context' 'show'"))return 'default\n';
  if(command.includes("'context' 'inspect'"))return JSON.stringify({Host:windows?'npipe:////./pipe/dockerDesktopLinuxEngine':'unix:///var/run/docker.sock'});
  if(command.includes("'version'"))return JSON.stringify({Client:{Os:windows?'windows':'linux',Version:'29.9.2'},Server:{Os:engineOs,Arch:'amd64',Version:'24.0.9'}});
  if(command.includes("'info'"))return JSON.stringify({ID:engineId,Name:'fixture',Containers:1,Images:1,DockerRootDir:rootDir,SystemTime:'2026-11-15T10:00:00.123456789Z'});
  if(command.includes("'image' 'ls'"))return [inspect.Image,unusedImage,inspect.Image].join('\n');
  if(command.includes("'network' 'ls'"))return [networkId,systemNetwork].join('\n');
  if(command.includes("'volume' 'ls'"))return volumeExists?'data\nused-data\n':'used-data\n';
  if(command.includes("'image' 'inspect'")){const values=[inspect.Image,unusedImage].filter(id=>command.includes("'"+id+"'"));return values.map(id=>JSON.stringify(command.includes('"tags":')?{id,tags:['demo:'+ (id===unusedImage?'unused':'1')],digests:[],size:100,created:'2026-11-16'}:{Id:id,RepoTags:['demo:1'],RepoDigests:[]})).join('\n');}
  if(command.includes("'network' 'inspect'"))return [networkId,systemNetwork].filter(id=>command.includes("'"+id+"'")).map(id=>JSON.stringify(command.includes('"id":')?{id,name:id===systemNetwork?'bridge':'custom',driver:'bridge',scope:'local'}:{Id:id,Name:id===systemNetwork?'bridge':'custom',Containers:{}})).join('\n');
  if(command.includes("'volume' 'inspect'"))return ['data','used-data'].filter(name=>command.includes("'"+name+"'")).map(name=>JSON.stringify(command.includes('"observation":')?{id:name,name,driver:'local',scope:'local',created:volume(name).CreatedAt,observation:volume(name)}:volume(name))).join('\n');
  if(command.includes("'volume' 'rm'")){volumeExists=false;return 'data\n';}
  if(command.includes("'ls'"))return id+'\n';
  if(command.includes("'inspect'")&&command.includes('"mounts":'))return JSON.stringify({id,name:inspect.Name,imageId:inspect.Image,mounts:inspect.Mounts});
  if(command.includes("'inspect'")&&command.includes('"HostConfig":'))return JSON.stringify(inspect);
  if(command.includes("'inspect'")&&command.includes('"id":'))return JSON.stringify({id,name:'service-中文',image:'demo:1',imageId:inspect.Image,status:inspect.State.Status,health:'unhealthy',exitCode:0,labels:{'com.docker.compose.project':'web','com.docker.compose.service':'api','com.docker.compose.project.working_dir':windows?'C:\\compose':'/srv/compose'}});
  if(command.includes("'inspect'"))return JSON.stringify(inspect);
  if(command.includes("'stats'"))return JSON.stringify({CPUPerc:'150.5%',MemUsage:'10MiB / 1GiB',MemPerc:'1%',NetIO:'1kB / 2kB',BlockIO:'0B / 0B',PIDs:'2'});
  if(command.includes("'logs'"))return '2026-11-10T00:00:00Z alpha 中文\n'+Array.from({length:149},(_,i)=>'line '+i).join('\n')+'\noff-page UNIQUE 中文\n';
  if(command.includes("'stop'")){inspect.State.Status='exited';inspect.State.Running=false;}
  if(command.includes("'start'")||command.includes("'restart'")){inspect.State.Status='running';inspect.State.Running=true;}
  return id+'\n';
};
peer=pluginPeer(path.join(root,manifest.entry),withProcessFacts(async(method,p)=>{
  if(method==='state.get')return {record:directoryRecords.get(p.topic)||{revision:'0',present:false}};
  if(method==='state.put'){assert.deepEqual(p.expectedContext,context);assert.equal(p.topic,'workspace.location.v1');const old=directoryRecords.get(p.topic);assert.equal(p.expectedRevision,old?.revision||'0');directoryWrites.push(p);const revision=String(directoryWrites.length);directoryRecords.set(p.topic,{revision,present:true,value:p.value});if(loseDirectoryReply){loseDirectoryReply=false;throw Object.assign(new Error('lost directory reply'),{code:-32003});}return {revision};}
  if(method==='session.program.prepare'){
    assert.deepEqual(p.expectedContext,context);
    assert.deepEqual(p.environment,environment);
    assert.equal(p.program.file,windows?'docker.exe':'docker');
    if(p.program.checks.length===2)assert.match(p.program.checks[1].expectedStdout,/^[a-f0-9]{64}$/,'only a complete container ID can receive a container startup check');
    const command=p.program.argv.map(a=>"'"+a.replaceAll("'","'\\''")+"'").join(' ')+' checks='+JSON.stringify(p.program.checks);
    programs.set(command,p.program);
    return {command,length:command.length,mode:p.mode,programOutput:p.mode==='terminal'?null:'startup-v1'};
  }
  if(method==='session.environment.start'){
    const taskId=String(++number),bytes=Buffer.from(JSON.stringify({environment,values:{DOCKER_HOST:'',DOCKER_CONTEXT:'',DOCKER_CONFIG:''}}));
    tasks.set(taskId,{id:taskId,operationId:p.operationId,bytes,command:'environment'});return {taskId,state:'running'};
  }
  if(method==='commands.resolve')return {available:true};
  if(method==='commands.submit'){
    submissions.push(p);let receipt=tickets.get(p.requestId);
    if(!receipt){receipt={...(p.name==='network.tcpForward.ensure.v1'?{result:{version:1,outcome:tunnelOutcome,disposition:'created',ruleId:'rule',runId:'run',state:'starting'}}:{}),status:'pending',commandId:'command-'+p.requestId,viewId:'terminal-'+p.requestId};tickets.set(p.requestId,receipt);}
    if(loseCommandReceipt){loseCommandReceipt=false;throw Object.assign(new Error('lost command receipt'),{code:-32003,data:{dispatch:'unknown'}});}
    return receipt;
  }
  if(method==='commands.get'){const receipt=tickets.get(p.requestId);assert(receipt);return {...receipt,status:'completed'};}
  if(method==='settings.get')return {values:{refreshSeconds:'30',showLogMinimap:true}};
  if(method==='session.target.describe')return {state:'connected',context,targetSource:'current',target:{name:'host',protocol:'ssh',host:'host',port:22,endpoint:'host:22',username:'ops'},closing:false,capabilities:['exec','files']};
  if(method==='logging.write'||method==='messages.publish'){diagnostics.push({method,...p});return {};}
  if(method==='ui.updateView')return {};
  if(method==='ui.openView')return {viewId:'opened'};
  if(method==='session.process.start'){
    let old=streamRequests.find(r=>r.operationId===p.operationId);if(old){const {processId,...original}=old;assert.deepEqual(p,original);}else{
      const processId='stream-'+(++number);old={...p,processId};streamRequests.push(old);const pull=p.command.includes("'image' 'pull'");const follow=programs.get(p.command)?.argv.includes('--follow');const log=programs.get(p.command)?.argv.includes('logs');
      const raw=log&&!follow?Buffer.from(output(p.command)):Buffer.alloc(0);const envelope=raw.length?Buffer.from(JSON.stringify({v:1,sequence:'1',source:'stdout',receivedAt:1794736800000,dataBase64:raw.toString('base64')})+'\n'):Buffer.alloc(0);streamBodies.set(processId,envelope);
      streams.set(processId,processFacts({state:log?(follow?'running':'completed'):(p.retention==='rolling'||pull&&holdPull?'running':'completed'),finished:log?!follow:p.retention!=='rolling'&&!(pull&&holdPull),producedBytes:String(envelope.length||100),earliestAvailableOffset:'0',...(log?{outputComplete:!follow,remoteExitConfirmed:!follow,exitCode:follow?null:0}:pull&&streamConfirmed?{remoteExitConfirmed:true,exitCode:0}:{})}));
    }
    if(loseStreamReceipts>0){loseStreamReceipts--;throw Object.assign(new Error('lost stream receipt'),{code:-32003,data:{dispatch:'unknown'}});}return {processId:old.processId,status:streams.get(old.processId)};
  }
  if(method==='session.process.lookup'){
    streamLookups.push(p);const old=streamRequests.find(r=>r.operationId===p.operationId);if(!old)return {known:false};const {processId,...original}=old;assert.deepEqual(p,original);return {known:true,processId,status:streams.get(processId)};
  }
  if(method==='session.process.read'){const status=streams.get(p.processId);assert(status);const body=streamBodies.get(p.processId);const chunks=body?.length&&p.offset==='0'?[JSON.parse(body.toString())]:[];return {chunks,nextOffset:body?.length?String(body.length):'0',watermark:body?.length?String(body.length):'0',earliestAvailableOffset:'0',status};}
  if(method==='session.process.status'){assert(streams.has(p.processId));return streams.get(p.processId);}
  if(method==='session.process.cancel'){streams.set(p.processId,{...streams.get(p.processId),state:'cancelled',finished:true,workerFinished:true});return streams.get(p.processId);}
  if(method==='session.process.release'){const status=streams.get(p.processId);streams.delete(p.processId);return {releaseRequested:true,resourceReleased:true,status};}
  if(method==='session.exec.start'){
    const old=[...tasks.values()].find(t=>t.operationId===p.operationId);if(old)return {taskId:old.id,state:'running'};
    const taskId=String(++number),bytes=Buffer.from(output(p.command));commands.push(p.command);tasks.set(taskId,{id:taskId,operationId:p.operationId,bytes,command:p.command});
    if(loseReceipt&&p.command.includes("'stop'")){loseReceipt=false;throw Object.assign(new Error('Lost start receipt'),{code:-32003,data:{dispatch:'unknown'}});}
    return {taskId,state:'running'};
  }
  if(method==='session.exec.poll'){
    const t=tasks.get(p.taskId);assert(t);
    if(hold||(holdMutation&&t.command.includes("'stop'")))return {state:'running',execSent:true,elapsedMs:1,outputLimitExceeded:false,exitCode:null,exitSignal:null,error:null,remoteExitConfirmed:false,stdout:{dataBase64:'',nextOffset:'0'},stderr:{dataBase64:'',nextOffset:'0'},outputComplete:false};
    const start=Number(p.stdoutOffset),end=Math.min(start+8192,t.bytes.length);
    const imageConflict=t.command.includes("'image' 'rm'");const stderr=imageConflict&&p.stderrOffset==='0'?Buffer.from('conflict: image referenced in multiple repositories'):Buffer.alloc(0);
    return {state:'completed',execSent:true,elapsedMs:1,outputLimitExceeded:false,exitSignal:null,stdout:{dataBase64:malformed ? 'Zg=a' : t.bytes.subarray(start,end).toString('base64'),nextOffset:String(end)},stderr:{dataBase64:stderr.toString('base64'),nextOffset:String(imageConflict?54:0)},outputComplete:end===t.bytes.length,remoteExitConfirmed:true,exitCode:imageConflict?1:0,error:null};
  }
  if(method==='session.exec.release'){tasks.delete(p.taskId);return {};}
  throw new Error('Unexpected core call: '+method);
}));
const request=async(method,viewId,extra={})=>{
   if(method==='ui.event'&&['openTerminal','operate','tunnel.create','tunnel.reveal','openLogs'].includes(extra.data?.action)){
     const current=(await peer.request('ui.getView',{viewId,context})).data;
     extra={...extra,data:{id,engineId:current.engine?.id,endpoint:current.endpoint,expectedContext:context,scopeEpoch:current.scopeEpoch,...extra.data}};
   }
   return peer.request(method,{viewId,context,...extra}).catch(error=>{error.message=method+' '+viewId+' '+(extra.data?.action||extra.viewType||'')+': '+error.message;throw error;});
 };
const state=id=>request('ui.getView',id).then(r=>r.data);
const idle=id=>until(()=>state(id),s=>!s.busy&&(s.kind!=='browser'||s.state!=='ready'||!['loading','unloaded'].includes(s.indexes?.container?.phase)));
try{
  await assert.rejects(peer.initialize({dataDirectory:directory,coreApiVersion:"1.0"}),/API/);
  await peer.initialize({dataDirectory:directory});
  await request('ui.createView','browser',{viewType:'browser'});let browser=await idle('browser');assert.equal(browser.state,'ready');
  const completed=await until(()=>state('browser'),s=>s.indexes.container.complete&&!s.tree.nodes.find(n=>n.key==='category:container')?.loading);
  assert(completed.tree.nodes.some(n=>n.composeProject==='web'&&n.composeLocation?.path),'completed index updates its cached tree without a navigation event');
  const initialPage=(await request('ui.event','browser',{data:{action:'browser.page',status:'abnormal',query:'中文'}})).data;
  assert(initialPage.nodes.some(n=>n.resource?.id===id));
  const project=initialPage.nodes.find(n=>n.composeProject==='web');assert(project);assert.equal(project.composeLocation.path,windows?'C:\\compose':'/srv/compose');
  const follow={action:'compose.follow',project:'web',observationRevision:project.observationRevision,publicationId:'00000000-0000-4000-8000-000000000001',engineId:browser.engine.id,endpoint:browser.endpoint,expectedContext:context,scopeEpoch:browser.scopeEpoch};
  await request('ui.event','browser',{data:follow});await until(()=>state('browser'),s=>s.directoryPublication?.state==='succeeded');assert.equal(directoryWrites.length,1,'lost reply queries the original directory publication');assert.equal(directoryWrites[0].value.filesystemView,'session-default');
  await request('ui.event','browser',{data:follow});assert.equal(directoryWrites.length,1);
  await assert.rejects(request('ui.event','browser',{data:{...follow,engineId:'other'}}),/身份/);
  await request('ui.event','browser',{data:{...follow,publicationId:'00000000-0000-4000-8000-000000000002'}});await until(()=>state('browser'),s=>s.directoryPublication?.state==='succeeded');assert.equal(directoryWrites.length,2,'explicit same directory publishes a fresh ID');

  await assert.rejects(request('ui.createView','removed',{viewType:'container',resourceKey:JSON.stringify({id})}),/未知 Docker 视图/);
  await request('ui.createView','detail',{viewType:'browser'});await idle('detail');
  const terminal={action:'openTerminal',requestId:'terminal-once',shell};
  await request('ui.event','detail',{data:terminal});await request('ui.event','detail',{data:terminal});
  const terminalResult=await until(async()=>(await request('ui.getView','detail')).data,v=>v.terminal?.status==='completed');
  assert.equal(terminalResult.terminal.viewId,'terminal-terminal-once');assert.equal(submissions.length,1,'lost submit receipt must query the original intent');
  assert.equal(submissions[0].name,'terminal.open.v1');assert.equal(submissions[0].sourceViewId,'detail');
  const launch=submissions[0].payload;assert.equal(launch.directoryIntegration,'off');assert.deepEqual(programs.get(launch.launch.command).removeEnv,['DOCKER_HOST','DOCKER_CONTEXT','DOCKER_TLS_VERIFY','DOCKER_CERT_PATH','DOCKER_API_VERSION']);assert.equal(programs.get(launch.launch.command).checks[0].expectedStdout,'engine');assert(launch.launch.command.includes(id));assert(programs.get(launch.launch.command).argv.includes(shell));
  await request('ui.event','detail',{data:{action:'browser.load',resource:{kind:'container',id}}});
  const observed=await until(async()=>(await request('ui.event','detail',{data:{action:'browser.object',resource:{kind:'container',id}}})).data,o=>o.relation?.phase==='ready');
  const detailNavigation={schemaVersion:3,inline:{kind:'resource',nodeKey:'container:'+id,resource:{kind:'container',id},tab:'mounts',offsets:{mounts:3},previousOffsets:{mounts:[0,1]}}};
  const savedNavigation=(await request('ui.event','detail',{data:{action:'browser.navigation',state:detailNavigation}})).data.navigation;
  assert.equal(savedNavigation.inline.tab,'mounts');assert.deepEqual(savedNavigation.inline.previousOffsets.mounts,[0,1]);
  const originalMounts=inspect.Mounts;
  inspect.Mounts=Array.from({length:12},(_,index)=>({Type:'bind',Source:'/srv/data-'+index,Destination:'/data-'+index,RW:true}));
  await request('ui.event','detail',{data:{action:'browser.load',resource:{kind:'container',id}}});
  const pagedObservation=await until(async()=>(await request('ui.event','detail',{data:{action:'browser.object',resource:{kind:'container',id}}})).data,o=>o.relation?.phase==='ready');
  const mountsPage=(await request('ui.event','detail',{data:{action:'browser.collection',resource:{kind:'container',id},collection:'mounts',observationRevision:pagedObservation.observationRevision,offset:0}})).data;
  assert.equal(mountsPage.rows.length,5);assert.equal(mountsPage.nextOffset,5);
  inspect.Mounts=originalMounts;
  const ports=(await request('ui.event','detail',{data:{action:'browser.collection',resource:{kind:'container',id},collection:'ports',observationRevision:pagedObservation.observationRevision,offset:0}})).data;
  assert.equal(ports.rows.length,2);assert.deepEqual(ports.rows.map(r=>r.target.host),['127.0.0.1','::1']);
  const tunnel={action:'tunnel.create',requestId:'tunnel-once',binding:ports.rows[0].binding,selectedRule:null,expectedContext:context,engineId:'engine',endpoint:terminalResult.endpoint};
  loseCommandReceipt=true;const tunnelBefore=submissions.length;
  await request('ui.event','detail',{data:tunnel});await request('ui.event','detail',{data:tunnel});
  const tunnelResult=await until(()=>state('detail'),s=>s.tunnel?.status==='completed');
  assert.equal(tunnelResult.tunnel.result.ruleId,'rule');assert.equal(submissions.length,tunnelBefore+1);assert.equal(submissions.at(-1).name,'network.tcpForward.ensure.v1');assert.deepEqual(submissions.at(-1).payload.target,{host:'127.0.0.1',port:8080});
  await assert.rejects(request('ui.event','detail',{data:{...tunnel,binding:ports.rows[1].binding}}),/相同请求参数不同/);
  inspect.NetworkSettings.Ports['80/tcp']=[];
  await request('ui.event','detail',{data:{...tunnel,requestId:'tunnel-stale'}});
  const stale=await until(()=>state('detail'),s=>s.tunnel?.status==='failed');assert.match(stale.tunnel.error.message,/端口绑定已变化/);assert.equal(submissions.length,tunnelBefore+1,'Changed binding must not submit a tunnel command');
  inspect.NetworkSettings.Ports['80/tcp']=[{HostIp:'0.0.0.0',HostPort:'8080'},{HostIp:'::',HostPort:'8080'}];
  for(tunnelOutcome of ['selectionRequired','savedNotStarted']){
    const requestId='tunnel-'+tunnelOutcome;
    await request('ui.event','detail',{data:{...tunnel,requestId}});
    await until(()=>state('detail'),s=>s.tunnel?.status==='completed');
    const page=(await request('ui.event','detail',{data:{action:'browser.tasks',offset:0}})).data;
    assert.equal(page.rows.find(t=>t.requestId===requestId)?.state,'skipped','a completed receipt must not imply a tunnel was started');
  }
  tunnelOutcome='accepted';
  const malformedBefore=commands.length;
  malformed=true;await request('ui.event','detail',{data:{action:'refresh'}});let failed=await idle('detail');
  assert(JSON.stringify(failed).includes('核心输出编码无效'),JSON.stringify(failed));assert.equal(tasks.size,0,'bad encoding must release the task');
  assert.equal(commands.length,malformedBefore+1,'bad output must not replay a command');
  malformed=false;await request('ui.event','detail',{data:{action:'refresh'}});await idle('detail');
  const before=commands.length;for(let i=0;i<3;i++)await request('ui.getView','detail');assert.equal(commands.length,before,'mount/getView must not create commands');
  const mutation={action:'operate',requestId:'stop-once',intentSequence:'1',operation:'stop'};
  await request('ui.event','detail',{data:mutation});await request('ui.event','detail',{data:mutation});
  let detail=await idle('detail');assert.equal(detail.operation.containerId,id);assert.equal(detail.operation.state,'succeeded');
  assert.equal(commands.filter(c=>c.includes("'stop'")).length,1);
  await assert.rejects(request('ui.event','detail',{data:{...mutation,operation:'restart'}}),/相同请求参数不同/);
  await request('ui.event','detail',{data:{action:'browser.stats',resource:{kind:'container',id},engineId,endpoint:(await state('detail')).endpoint,expectedContext:context,scopeEpoch:(await state('detail')).scopeEpoch}});await idle('detail');
  await request('ui.event','detail',{data:{action:'operate',operation:'start',requestId:'start-once',intentSequence:'2'}});await idle('detail');
  await request('ui.event','detail',{data:{action:'browser.stats',resource:{kind:'container',id},engineId,endpoint:(await state('detail')).endpoint,expectedContext:context,scopeEpoch:(await state('detail')).scopeEpoch}});detail=await idle('detail');assert.equal((await request('ui.event','detail',{data:{action:'browser.object',resource:{kind:'container',id}}})).data.stats.CPUPerc,'150.5%');
  await request('ui.event','detail',{data:{action:'operate',operation:'remove',requestId:'reject-running-remove',intentSequence:'3'}});detail=await idle('detail');
  assert.equal(detail.operation.state,'failed','preflight rejection must not claim a sent mutation');assert.equal(commands.filter(c=>c.includes("'rm'")).length,0);
  await request('ui.createView','logs',{viewType:'logs',resourceKey:JSON.stringify({id,engineId:'engine'})});await idle('logs');
  const logRead=query=>request('ui.getView','logs',{data:{read:query}}).then(r=>r.data);
  const logRequest={action:'logs.open',requestId:'logs-once',intentSequence:'1',range:{kind:'latest'}};
  await request('ui.event','logs',{data:logRequest});await request('ui.event','logs',{data:logRequest});
  const logs=await until(()=>logRead({op:'status'}),s=>s.ready&&!s.loading&&!s.active&&s.windowTotal>0);
  assert.equal(streamRequests.length,2,'lost open receipt and repeated UI intent do not rerun commands');
  assert.equal(loseStreamReceipts,0);assert.equal(new Set(streamLookups.map(r=>r.operationId)).size,2);
  assert(streamRequests.some(r=>r.command.includes("'--tail' '1000'")));assert(streamRequests.some(r=>r.command.includes("'--follow'")));
  assert(!streamRequests.find(r=>r.command.includes("'--tail'")).command.includes("'--since'"),'latest output has no age cutoff');
  assert.equal(logs.selection.to,'2026-11-15T10:00:00.123456789Z');
  await assert.rejects(request('ui.event','logs',{data:{...logRequest,range:{kind:'recent',seconds:3600}}}),/同一日志请求参数不同/);
  const count=streamRequests.length;await request('ui.getView','logs');assert.equal(streamRequests.length,count);
  await request('ui.event','logs',{data:{action:'logs.stop',all:true}});await until(()=>logRead({op:'status'}),s=>!s.receiving);
  inspect.State.Status='exited';inspect.State.Running=false;
  await request('ui.event','logs',{data:{...logRequest,requestId:'stopped-tail',intentSequence:'2'}});
  await until(()=>logRead({op:'status'}),s=>s.nextIntent==='3'&&!s.loading&&!s.active&&s.windowTotal>0);
  assert(!streamRequests.at(-1).command.includes("'--since'"));
  await assert.rejects(request('ui.event','logs',{data:{...logRequest,requestId:'stale-log-intent'}}),/旧日志请求/);
  inspect.State.Status='running';inspect.State.Running=true;
  await assert.rejects(request('ui.event','detail',{data:{action:'section',section:'environment'}}),/未知 Docker 操作/);
  await assert.rejects(request('ui.event','browser',{data:{action:'setEndpoint',endpoint:'tcp://host:2375'}}),/Unix|npipe/);
  await request('ui.event','detail',{data:{action:'refresh'}});await idle('detail');
  holdMutation=true;await request('ui.event','detail',{data:{action:'operate',operation:'stop',requestId:'interrupted-stop',intentSequence:'4'}});
  await until(async()=>[...tasks.values()].some(t=>t.command.includes("'stop'")),Boolean);
  await request('ui.event','detail',{data:{action:'cancel'}});detail=await idle('detail');assert.equal(detail.operation.state,'unknown');holdMutation=false;
  assert(diagnostics.some(d=>d.method==='messages.publish'&&d.notify===false));assert(!JSON.stringify(diagnostics).includes('TOKEN'));
  hold=true;await request('ui.event','detail',{data:{action:'refresh'}});
  await until(async()=>tasks.size,n=>n>0);
  assert.equal((await request('ui.beforeClose','detail')).metadata.busy,true);
  await assert.rejects(request('ui.event','detail',{data:{action:'refresh'}}),/关闭/);
  await request('ui.disposeView','detail');await until(async()=>tasks.size,n=>n===0);hold=false;
  await request('ui.disposeView','logs');await until(async()=>streams.size,n=>n===0);
  browser=await idle('browser');const oldEndpoint=browser.endpoint;
  hold=true;await request('ui.event','browser',{data:{action:'connection.save',config:{mode:'manual',endpoint:windows?'npipe:////./pipe/candidate':'unix:///run/candidate.sock'}}});
  await until(async()=>[...tasks.values()].some(t=>t.command==='environment'),Boolean);
  await request('ui.createView','late-detail',{viewType:'logs',resourceKey:JSON.stringify({id,engineId:'engine',endpoint:oldEndpoint})});
  hold=false;browser=await idle('browser');assert.equal(browser.connection.test.state,'failed');assert.equal(browser.endpoint,oldEndpoint,'a new detail during discovery must prevent binding/config replacement');assert.equal(browser.connection.config.mode,'auto');
  await request('ui.disposeView','late-detail');await until(async()=>tasks.size,n=>n===0);engineId='replacement-engine';
  await request('ui.event','browser',{data:{action:'setEndpoint',endpoint:windows?'npipe:////./pipe/test-engine':'unix:///run/docker-test.sock'}});browser=await idle('browser');assert.equal(browser.engine.id,engineId);
  await request('ui.event','browser',{data:{action:'setEndpoint',endpoint:windows?'npipe:////./pipe/dockerDesktopLinuxEngine':'unix:///var/run/docker.sock'}});browser=await idle('browser');assert.equal(browser.state,'ready');
  engineId='third-engine';await request('ui.event','browser',{data:{action:'refresh'}});browser=await idle('browser');assert.equal(browser.state,'stale');assert.notEqual(browser.engine.id,engineId);
  await request('ui.event','browser',{data:{action:'connection.identify'}});browser=await idle('browser');assert.equal(browser.engine.id,engineId);
  const target={id,engineId,endpoint:browser.endpoint,expectedContext:context};
  const browserStart={action:'operate',...target,operation:'start',requestId:'sidebar-start',intentSequence:'1'};
  await request('ui.event','browser',{data:browserStart});await request('ui.event','browser',{data:browserStart});browser=await idle('browser');
  assert.equal(browser.count,1,'sidebar operation must preserve the container list');
  assert.equal(browser.operation.containerId,id);
  assert.equal((await request('ui.event','browser',{data:{action:'browser.page',status:'all',query:'',group:false,expandedKeys:['engine','category:container']}})).data.nodes.find(n=>n.resource?.id===id).status,'running');
  await assert.rejects(request('ui.event','browser',{data:{...browserStart,id:'b'.repeat(64)}}),/相同请求参数不同/);
  await assert.rejects(request('ui.event','browser',{data:{...browserStart,requestId:'old-engine',intentSequence:'2',engineId:'engine'}}),/身份已改变/);
  await assert.rejects(request('ui.event','browser',{data:{...browserStart,requestId:'old-context',intentSequence:'2',expectedContext:{...context,generation:2}}}),/身份已改变/);
  const sidebarTerminal={action:'openTerminal',...target,requestId:'sidebar-terminal',shell};
  const submittedBefore=submissions.length;
  await request('ui.event','browser',{data:sidebarTerminal});await request('ui.event','browser',{data:sidebarTerminal});
  await assert.rejects(request('ui.event','browser',{data:{...sidebarTerminal,shell:'/bin/bash'}}),/相同终端请求参数不同/);
  await until(()=>state('browser'),s=>s.terminal?.status==='completed');
  await request('ui.event','browser',{data:sidebarTerminal});assert.equal(submissions.length,submittedBefore+1,'completed terminal intent must not be recreated');
  assert.equal(submissions.at(-1).sourceViewId,'browser');
  assert(submissions.at(-1).payload.launch.command.includes(id));
  const browserStop={action:'operate',...target,operation:'stop',requestId:'sidebar-stop',intentSequence:'2'};
  const stopsBefore=commands.filter(c=>c.includes("'stop'")).length;
  await request('ui.event','browser',{data:browserStop});await request('ui.event','browser',{data:browserStop});browser=await idle('browser');
  assert.equal(commands.filter(c=>c.includes("'stop'")).length,stopsBefore+1);
  assert.equal(browser.operation.state,'succeeded');
  assert.equal((await request('ui.event','browser',{data:{action:'browser.page',status:'all',query:'',group:false,expandedKeys:['engine','category:container']}})).data.nodes.find(n=>n.resource?.id===id).status,'exited');
  await request('ui.event','browser',{data:{...browserStart,requestId:'sidebar-start-again',intentSequence:'3'}});await idle('browser');
  holdMutation=true;const unknown={...browserStop,requestId:'sidebar-unknown',intentSequence:'4'};
  await request('ui.event','browser',{data:unknown});await until(async()=>[...tasks.values()].some(t=>t.command.includes("'stop'")),Boolean);
  await request('ui.event','browser',{data:{action:'cancel'}});browser=await idle('browser');holdMutation=false;
  assert.equal(browser.operation.state,'unknown');assert.equal(browser.operation.containerId,id);
  const unknownCount=commands.length;await request('ui.event','browser',{data:unknown});assert.equal(commands.length,unknownCount,'unknown sidebar intent must never resend');
  await request('ui.event','browser',{data:{action:'refresh'}});browser=await idle('browser');assert.equal(browser.engine.dockerRootDir,rootDir);
  const load=async(kind,resource)=>{const s=await state('browser');await request('ui.event','browser',{data:{action:'browser.load',scopeEpoch:s.scopeEpoch,branchKind:kind,resource}});if(!resource)await until(()=>state('browser'),v=>!['unloaded','loading'].includes(v.indexes[kind].phase));else await until(async()=>(await request('ui.event','browser',{data:{action:'browser.object',resource}})).data,v=>v.relation?.phase!=='loading'&&v.relation);};
  const object=async resource=>(await request('ui.event','browser',{data:{action:'browser.object',resource}})).data;
  for(const kind of ['image','network','volume'])await load(kind);
  browser=await state('browser');assert.equal(browser.indexes.image.count,2,'image IDs are deduplicated');assert.equal(browser.indexes.volume.count,2);
  const readBefore=commands.length;await request('ui.event','browser',{data:{action:'browser.page',query:'',expandedKeys:['engine','category:image'],scopeEpoch:browser.scopeEpoch}});assert.equal(commands.length,readBefore,'paging never starts remote reads');
  inspect.State.Status='exited';inspect.State.Running=false;inspect.Mounts=[{Type:'volume',Name:'used-data',Source:'/srv/docker-data/volumes/used-data/_data',Destination:'/data'},{Type:'bind',Source:'/srv/config',Destination:'/config'}];
  const usedImage={kind:'image',id:inspect.Image},usedVolume={kind:'volume',id:'used-data'},freeVolume={kind:'volume',id:'data'};
  await load(null,usedImage);await load(null,usedVolume);assert.equal((await object(usedImage)).relation.count,1,'stopped containers still reference their image');assert.equal((await object(usedVolume)).relation.count,1,'stopped containers still reference volumes');
  const resourceInput=async resource=>{const s=await state('browser'),o=await object(resource);return {action:'resource.operate',operation:'remove',resource,requestId:'resource-'+resource.kind+'-'+s.nextIntent,intentSequence:s.nextIntent,observationRevision:o.revision,engineId:s.engine.id,endpoint:s.endpoint,expectedContext:context,scopeEpoch:s.scopeEpoch};};
  await assert.rejects(request('ui.event','browser',{data:await resourceInput(usedVolume)}),/仍有容器引用/);
  await load(null,freeVolume);const oldVolume=await resourceInput(freeVolume);volumeCreatedAt='2026-11-16T01:00:00Z';await request('ui.event','browser',{data:oldVolume});browser=await idle('browser');assert.equal(browser.operation.state,'failed');assert(!commands.some(c=>c.includes("'volume' 'rm'")),'same-name volume rebuild invalidates confirmation');
  await request('ui.event','browser',{data:{action:'refresh'}});await idle('browser');await load('volume');await load(null,freeVolume);

  const deletion=await resourceInput(freeVolume);await request('ui.event','browser',{data:deletion});await request('ui.event','browser',{data:deletion});browser=await idle('browser');assert.equal(browser.operation.state,'succeeded');assert.equal(commands.filter(c=>c.includes("'volume' 'rm'")).length,1);assert.equal(browser.indexes.volume.count,1);
  await load('image');await load(null,{kind:'image',id:unusedImage});const imageDeletion=await resourceInput({kind:'image',id:unusedImage});await request('ui.event','browser',{data:imageDeletion});browser=await idle('browser');assert.equal(browser.operation.state,'failed');assert(browser.operation.message.includes('multiple repositories'));assert(commands.some(c=>c.includes("'image' 'rm' '--no-prune'")));assert(!commands.some(c=>c.includes("'--force'")||c.includes("'prune'")));
  await request('ui.event','browser',{data:{action:'refresh'}});await idle('browser');await load('network');await load(null,{kind:'network',id:systemNetwork});await request('ui.event','browser',{data:await resourceInput({kind:'network',id:systemNetwork})});browser=await idle('browser');assert.equal(browser.operation.state,'failed');assert(!commands.some(c=>c.includes("'network' 'rm'")),'system networks cannot be deleted');
  await request('ui.event','browser',{data:{action:'refresh'}});browser=await idle('browser');
  const pullInput=async(view,reference)=>{const s=await state(view);return {action:'pull.start',reference,engineId:s.engine.id,endpoint:s.endpoint,expectedContext:context,scopeEpoch:s.scopeEpoch,requestId:'pull-'+view+'-'+s.nextIntent,intentSequence:s.nextIntent};};
  loseStreamReceipts=1;const pull=await pullInput('browser','public/demo');await request('ui.event','browser',{data:pull});await request('ui.event','browser',{data:pull});browser=await until(()=>state('browser'),s=>s.pull?.state==='unknown');assert.equal(streamRequests.filter(r=>r.command.includes("'image' 'pull'")).length,1,'lost pull receipt resolves original ticket');assert(streamLookups.some(r=>r.operationId.startsWith('pull-')));await request('ui.event','browser',{data:pull});assert.equal(streamRequests.filter(r=>r.command.includes("'image' 'pull'")).length,1);
  streamConfirmed=true;const successfulPull=await pullInput('browser','public/demo:stable');await request('ui.event','browser',{data:successfulPull});browser=await until(()=>state('browser'),s=>s.pull?.state==='succeeded');assert(browser.pull.processId);await idle('browser');
  await request('ui.createView','browser2',{viewType:'browser'});await idle('browser2');await request('ui.createView','browser3',{viewType:'browser'});await idle('browser3');holdPull=true;streamConfirmed=false;
  await request('ui.event','browser',{data:await pullInput('browser','public/a')});await until(()=>state('browser'),s=>s.pull?.state==='running');await request('ui.event','browser2',{data:await pullInput('browser2','public/b')});await until(()=>state('browser2'),s=>s.pull?.state==='running');await assert.rejects(request('ui.event','browser3',{data:await pullInput('browser3','public/c')}),/两个镜像拉取/);
  await assert.rejects(request('ui.event','browser',{data:{action:'setEndpoint',endpoint:'unix:///run/other.sock'}}),/镜像拉取/);
  await request('ui.event','browser',{data:{action:'browser.visibility',visible:false}});assert.equal((await state('browser')).pull.state,'running','hiding the sidebar keeps its pull');await request('ui.event','browser',{data:{action:'pull.cancel'}});await until(()=>state('browser'),s=>s.pull?.state==='unknown');
  await request('ui.disposeView','browser2');await request('ui.disposeView','browser3');
  const taskPage=(await request('ui.event','browser',{data:{action:'browser.tasks',offset:0}})).data;assert(taskPage.rows.length>0);assert(Buffer.byteLength(JSON.stringify(await state('browser')))<24*1024);
  await request('ui.disposeView','browser');
  await until(async()=>streams.size,n=>n===0);
  await assert.rejects(state('detail'),/关闭/);
  await peer.close();assert.equal(peer.child.exitCode,0);
  console.log('Docker process with mocked '+environment.kind+'/'+engineOs+': resources, references, deletion, pull tickets/concurrency/disposal, UI v2, logs and terminals passed.');
}finally{
  if(peer.child.exitCode===null){peer.child.kill();await new Promise(r=>peer.child.once('exit',r));}
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-process-'));fs.rmSync(directory,{recursive:true,force:true});
}
if(process.argv.length===2)for(const mode of ['--windows','--windows-engine']){
  const result=spawnSync(process.execPath,[process.argv[1],mode],{stdio:'inherit',windowsHide:true});assert.equal(result.status,0);
}
