import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import net from 'node:net';
import {createInterface} from 'node:readline';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';
const output=resolve('.qa/t014/benchmark-'+Date.now());await mkdir(output,{recursive:true});
const node=JSON.parse(await readFile('.qa/t014/linux/config.json','utf8')).nodes[0],metrics={rounds:[]};
process.env.SODECK_PLUGIN_PACKAGES=resolve('resources/network-proxy.sodeck-plugin');process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9952});
let app,browser,reference,refErrors='';
const ps=async cmd=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-Command',cmd],{windowsHide:true})).stdout);
const pattern=Buffer.from(Array.from({length:256},(_,i)=>i)),hash=createHash('sha256');for(let i=0;i<262144;i++)hash.update(pattern);const expected=hash.digest('hex');
async function transfer(port,proxy=false){const t=performance.now();const {stdout}=await promisify(execFile)('curl.exe',['-sSf','--max-time','90','--noproxy',proxy?'':'*',...(proxy?['--proxy',`http://127.0.0.1:${port}`]:[]),`http://127.0.0.1:${proxy?18082:port}/large`],{windowsHide:true,encoding:'buffer',maxBuffer:70*1024*1024});const ms=performance.now()-t;assert.equal(stdout.length,64*1024*1024);assert.equal(createHash('sha256').update(stdout).digest('hex'),expected);return ms;}
try{
 app=await launch({data:join(output,'data'),shellPort:9951,pluginPort:9952,packagePath:resolve('resources/network-proxy.sodeck-plugin'),allowStartupWarning:true});
 const s=await rpc(app.page,'servers.routes.v2.create',{name:'单跳基准 A',host:node.host,port:node.port,username:node.username,authType:'key',connectionRoute:{kind:'direct'}});await rpc(app.page,'auth.set',{serverId:s.id,privateKeyPath:node.key,remember:false});
 try{const l=await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:l.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
 await rpc(app.page,'workspaces.open',{serverId:s.id});const v=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:s.id,operationId:crypto.randomUUID()});
 browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9952'));const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));await page.getByRole('button',{name:'新建',exact:true}).waitFor();const invoke=(command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
 const ports={};for(const type of ['port-forward','network-proxy']){const rule=await invoke('network.save',{expectedRevision:0,rule:{name:type,type,bindMode:'auto',port:0,autoReconnect:false,...(type==='port-forward'?{targetHost:'127.0.0.1',targetPort:18082}:{})}});await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:v.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});ports[type]=(await invoke('network.snapshot',{selected:rule.id})).selected.run.address.port;}
 reference=spawn(resolve('target/debug/examples/network_reference.exe'),[String(node.port),node.username,node.key,'18082'],{windowsHide:true,stdio:['ignore','pipe','pipe']});reference.stderr.on('data',b=>refErrors+=b);
 const baseline=await new Promise((r,j)=>{createInterface({input:reference.stdout}).once('line',line=>r(JSON.parse(line)));reference.once('exit',()=>j(Error(refErrors)));});metrics.baseline=baseline;const referencePort=baseline.port;
 await until(()=>new Promise(r=>{const s=net.connect(referencePort,'127.0.0.1');s.once('connect',()=>{s.destroy();r(true);});s.once('error',()=>r(false));}));
 const worker=(await rpc(app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.network-proxy');
 const sample=()=>ps(`Get-Process -Id ${app.process.pid},${worker.processId},${reference.pid} | Select-Object Id,ProcessName,CPU,WorkingSet64,PrivateMemorySize64,@{Name='Threads';Expression={$_.Threads.Count}} | ConvertTo-Json -Compress`);
 metrics.hardware=await ps('Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors | ConvertTo-Json -Compress');metrics.before=await sample();
 for(let i=0;i<3;i++){const round={referenceMs:await transfer(referencePort),spliceMs:await transfer(ports['port-forward']),httpMs:await transfer(ports['network-proxy'],true)};metrics.rounds.push(round);console.log(JSON.stringify(round));}
 metrics.after=await sample();const median=k=>metrics.rounds.map(r=>r[k]).sort((a,b)=>a-b)[1];metrics.spliceRatio=median('referenceMs')/median('spliceMs');metrics.httpRatio=median('spliceMs')/median('httpMs');assert.ok(metrics.spliceRatio>=.7,JSON.stringify(metrics));assert.ok(metrics.httpRatio>=.25,JSON.stringify(metrics));metrics.passed=true;
}finally{reference?.kill();await stop(app);await browser?.close().catch(()=>{});await writeFile(join(output,'reference-stderr.log'),refErrors);await writeFile(join(output,'results.json'),JSON.stringify(metrics,null,2));await writeFile('.qa/t014/benchmark-latest.json',JSON.stringify({output,metrics},null,2));}
