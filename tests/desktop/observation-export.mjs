// Native dialog driver: setup/results through public RPC; dialog input via Computer Use.
import assert from 'node:assert/strict';
import {mkdir,mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {launch,rpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t016/export-native');await mkdir(output,{recursive:true});
const data=await mkdtemp(join(output,'data-'));let app,handled=0;
const exports=[];
const report=async extra=>writeFile(join(output,'state.json'),JSON.stringify({data,pid:app?.process.pid,exports,...extra},null,2));
try{
 app=await launch({data,shellPort:9581,pluginPort:9582});
 const evidence=await rpc(app.page,'evidence.create',{filter:{kind:'lifecycle'},allowPartial:true});
 await until(async()=>['succeeded','partial'].includes((await rpc(app.page,'evidence.get',{jobId:evidence.jobId})).job.state));
 await report({ready:true,packageId:evidence.jobId});const began=Date.now();
 while(Date.now()-began<1800000){
  const command=await readFile(join(output,'command.json'),'utf8').then(JSON.parse).catch(()=>null);
  if(!command||command.id<=handled){await delay(200);continue;}handled=command.id;
  if(command.action==='stop')break;
  if(command.action==='export'){
   const job=await rpc(app.page,'evidence.export',{jobId:evidence.jobId,format:'jsonl'});exports.push({id:job.jobId,label:command.label});
  }else if(command.action==='cancel'){
   await rpc(app.page,'evidence.cancel',{jobId:exports.at(-1).id});
  }
  for(const entry of exports)entry.result=(await rpc(app.page,'evidence.get',{jobId:entry.id})).job;
  assert.equal((await rpc(app.page,'evidence.get',{jobId:evidence.jobId})).job.kind,'evidence');
  await report({ready:true,packageId:evidence.jobId,handled});
 }
}finally{await stop(app);await report({finished:true,handled,shutdown:app?.shutdown});}
