// The actual Docker plugin owns parsing, windows, searches and export; only its core/Engine is a fixture.
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {pluginPeer,packageRoot,context,until} from './docker-process.mjs';
import {withProcessFacts,processFacts} from './process-fixture.mjs';
export async function createDockerLogFixture(){
  const root=packageRoot(),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'))),directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-log-fixture-'));
  const id='a'.repeat(64),engineNow=Date.now(),origin=engineNow-3*86400000;
  let records=Array.from({length:3000},(_,i)=>({source:i%11?'stdout':'stderr',text:`${i===20?'ERROR historic-needle':i===2998?'ERROR latest-needle':'INFO record'} ${i} 中文 ${i%7===0?'wrapped text '.repeat(30):''}`,time:new Date(origin+i*1000).toISOString()}));
  const inspect={Id:id,Name:'/prod-api-中文',Config:{Tty:false},State:{Status:'running',Running:true,StartedAt:new Date(origin).toISOString(),FinishedAt:'0001-01-01T00:00:00Z'}};
  const tasks=new Map(),processes=new Map(),calls=[];let serial=0,sequence=0,hold=false,fail=false,observe=false,peer;
  const envelope=record=>({v:1,sequence:String(++sequence),source:record.source,receivedAt:Date.now(),dataBase64:Buffer.from(record.time+' '+record.text+'\n').toString('base64')});
  const add=(task,record)=>{for(const bytes of split(Buffer.from(record.time+' '+record.text+'\n'),8192)){const chunk={v:1,sequence:String(task.chunks.length+1),source:record.source,receivedAt:Date.now(),dataBase64:bytes.toString('base64')};const length=Buffer.byteLength(JSON.stringify(chunk)+'\n');task.chunks.push({start:task.bytes,end:task.bytes+length,chunk});task.bytes+=length;}};
  const output=argv=>{if(argv.includes('version'))return JSON.stringify({Client:{Os:'linux',Version:'24.0.9'},Server:{Os:'linux',Arch:'amd64',Version:'24.0.9'}});if(argv.includes('info'))return JSON.stringify({ID:'fixture-engine',Name:'fixture-engine',SystemTime:new Date(engineNow).toISOString(),DockerRootDir:'/var/lib/docker'});if(argv.includes('inspect')&&!argv.includes('context'))return JSON.stringify(inspect);if(argv.includes('context')&&argv.includes('show'))return'default';if(argv.includes('context'))return JSON.stringify({Host:'unix:///var/run/docker.sock'});throw Error('Unexpected metadata argv '+JSON.stringify(argv));};
  const facts=task=>processFacts({state:task.done?'completed':'running',finished:task.done,workerFinished:task.done,outputComplete:task.done&&!task.cancelled,exitCode:task.done?task.failed?1:task.cancelled?null:0:null,remoteExitConfirmed:task.done&&!task.cancelled,producedBytes:String(task.bytes),earliestAvailableOffset:'0',error:task.failed?{code:'execFailed',message:'fixture Docker logs failure'}:null});
  peer=pluginPeer(path.join(root,manifest.entry),withProcessFacts(async(method,p)=>{
    calls.push({method,...p});
    if(method==='settings.get')return{values:{refreshSeconds:'30'}};
    if(method==='session.target.describe')return{state:'connected',context,targetSource:'current',target:{name:'fixture',protocol:'ssh',host:'fixture',port:22,endpoint:'fixture:22',username:'ops'},closing:false,capabilities:['exec']};
    if(method==='session.program.prepare')return{command:JSON.stringify(p.program.argv),length:JSON.stringify(p.program.argv).length,mode:p.mode,programOutput:'startup-v1'};
    if(method==='session.environment.start'){const taskId=String(++serial);tasks.set(taskId,Buffer.from(JSON.stringify({environment:{kind:'posix'},values:{}})));return{taskId,state:'running'};}
    if(method==='session.exec.start'){const taskId=String(++serial);tasks.set(taskId,Buffer.from(output(JSON.parse(p.command))));return{taskId,state:'running'};}
    if(method==='session.exec.poll'){const bytes=tasks.get(p.taskId);return{state:'completed',execSent:true,elapsedMs:0,outputLimitExceeded:false,remoteExitConfirmed:true,exitCode:0,exitSignal:null,error:null,stdout:{dataBase64:bytes.subarray(Number(p.stdoutOffset)).toString('base64'),nextOffset:String(bytes.length)},stderr:{dataBase64:'',nextOffset:'0'},outputComplete:true};}
    if(method==='session.exec.release'){tasks.delete(p.taskId);return{};}
    if(method==='session.process.start'){const argv=JSON.parse(p.command),follow=argv.includes('--follow'),get=name=>argv.includes(name)?argv[argv.indexOf(name)+1]:undefined,from=get('--since'),to=get('--until'),tail=Number(get('--tail')??Infinity),processId=String(++serial);const task={request:p,chunks:[],bytes:0,done:!follow&&!hold,failed:fail,follow,cancelled:false};fail=false;for(const row of records.filter(r=>(!from||stamp(r.time)>=stamp(from))&&(!to||stamp(r.time)<stamp(to))).slice(-tail))add(task,row);processes.set(processId,task);return{processId,status:facts(task)};}
    if(method==='session.process.lookup'){const found=[...processes].find(([,t])=>t.request.operationId===p.operationId);return found?{known:true,processId:found[0],status:facts(found[1])}:{known:false};}
    if(method==='session.process.read'){if(observe){observe=false;throw Error('fixture transient observation failure');}const task=processes.get(p.processId);if(!task)throw Error('原进程资源已释放');const offset=Number(p.offset);const chunks=task.chunks.filter(c=>c.start>=offset&&c.end<=offset+48*1024);const next=chunks.at(-1)?.end??offset;return{chunks:chunks.map(c=>c.chunk),nextOffset:String(next),watermark:String(task.bytes),earliestAvailableOffset:'0',status:facts(task)};}
    if(method==='session.process.status'){const task=processes.get(p.processId);if(!task)throw Error('原进程资源已释放');return facts(task);}
    if(method==='session.process.cancel'){const task=processes.get(p.processId);task.done=true;task.cancelled=true;return facts(task);}
    if(method==='session.process.release'){const task=processes.get(p.processId);processes.delete(p.processId);return{releaseRequested:true,resourceReleased:true,status:facts(task)};}
    if(['logging.write','messages.publish','ui.updateView'].includes(method))return{};
    throw Error('Unexpected log fixture core call '+method);
  }));
  await peer.initialize({dataDirectory:directory});await peer.request('ui.createView',{viewId:'logs',viewType:'logs',context,resourceKey:JSON.stringify({id,engineId:'fixture-engine',endpoint:'unix:///var/run/docker.sock'})});
  const snapshot=await until(()=>peer.request('ui.getView',{viewId:'logs',context}).then(r=>r.data),s=>s.state==='ready');
  const event=data=>peer.request('ui.event',{viewId:'logs',context,data}).then(r=>r.data);
  const read=query=>peer.request('ui.getView',{viewId:'logs',context,data:{read:query}}).then(r=>r.data);
  return{root,peer,calls,snapshot,origin,engineNow,read,event,inspect,get records(){return records;},set records(next){records=next;},set hold(value){hold=value;},set fail(value){fail=value;},set observe(value){observe=value;},
    append(text,count=1){for(let i=0;i<count;i++){const row={source:'stdout',text:text+(count>1?' '+i:''),time:new Date(engineNow+1000+i).toISOString()};records.push(row);for(const task of processes.values())if(task.follow&&!task.done)add(task,row);}},
    async close(){await peer.request('ui.disposeView',{viewId:'logs',context});await until(async()=>processes.size,n=>n===0);await peer.close();if(path.dirname(directory)!==os.tmpdir()||!path.basename(directory).startsWith('sodeck-log-fixture-'))throw Error('Unexpected fixture path');fs.rmSync(directory,{recursive:true,force:true});}
  };
}
function* split(bytes,size){for(let at=0;at<bytes.length;at+=size)yield bytes.subarray(at,at+size);}

function stamp(value){const m=/^(.*T\d\d:\d\d:\d\d)(?:\.(\d+))?Z$/.exec(value);if(!m)throw Error('Fixture time invalid: '+value);return BigInt(Date.parse(m[1]+'Z'))*1000000n+BigInt((m[2]||'').padEnd(9,'0'));}
