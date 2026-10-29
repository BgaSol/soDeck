import assert from 'node:assert/strict';
import {readFile,mkdir,mkdtemp,writeFile,readdir} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {execFileSync} from 'node:child_process';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';

const output=resolve('.qa/t016/native-boundaries');await mkdir(output,{recursive:true});
const data=await mkdtemp(join(output,'data-')),checks=[];
const fixture=JSON.parse(await readFile('.qa/t016/config.json','utf8'));
const node=fixture.nodes.find(n=>n.platform==='linux');
const ports={'sodeck.terminal':9612,'sodeck.files':9613,'sodeck.network-proxy':9614};
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(ports);
const bodies={terminal:'TERMINAL_BODY_T016_7ad2',path:'/FILE_BODY_T016_21e0_missing',rule:'URL_BODY_T016_c830'};
const markers=[...Object.values(bodies),Buffer.from(bodies.path).toString('base64')];
const browsers=new Map();let app,exportJob;
const note=text=>{checks.push(text);console.log(text);};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function pageFor(plugin,workspace){return until(async()=>{
 let browser=browsers.get(plugin);if(!browser?.isConnected()){browser=await chromium.connectOverCDP(`http://127.0.0.1:${ports[plugin]}`);browsers.set(plugin,browser);}
 for(const page of browser.contexts().flatMap(c=>c.pages()))if(!page.isClosed()&&await page.evaluate(()=>!window.__SODECK_CONTEXT__?.dialog&&window.__SODECK_CONTEXT__?.workspaceId).catch(()=>null)===workspace)return page;
},30000);}
async function events(filter){let cursor,items=[];const bounded={fromUtc:Date.now()-600000,toUtc:Date.now()+1000,...filter};do{const page=await rpc(app.page,'events.v2.query',{filter:bounded,limit:200,...(cursor?{cursor}:{})});items.push(...page.items);cursor=page.nextCursor;}while(cursor);return items;}
async function scan(path){let count=0;for(const entry of await readdir(path,{withFileTypes:true})){const name=join(path,entry.name);if(entry.isDirectory())count+=await scan(name);else{const bytes=await readFile(name);for(const marker of markers)assert.ok(!bytes.includes(Buffer.from(marker)),`Prohibited body in ${name}`);count++;}}return count;}
try{
 app=await launch({data,shellPort:9611,pluginPort:9612});
 const server=await rpc(app.page,'servers.create',{name:'T016 isolated boundaries',host:node.host,port:node.port,username:node.username,authType:'key'});
 await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:node.key});
 for(let i=0;i<3;i++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});break;}catch(error){assert.equal(error.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...error.details});}
 await rpc(app.page,'workspaces.open',{serverId:server.id});
 await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===1,45000);
 const authServer=await rpc(app.page,'servers.create',{name:'T016 auth cancellation',host:node.host,port:node.port,username:node.username,authType:'key'});
 await rpc(app.page,'workspaces.open',{serverId:authServer.id});await app.page.getByRole('heading',{name:'连接认证',exact:true}).waitFor();
 const authPage=await pageFor('sodeck.terminal',authServer.id);
 const waiting=await until(async()=>(await invoke(authPage,'sessions.list',{workspaceId:authServer.id})).find(s=>s.status==='waitingAuth'));
 await app.page.getByRole('button',{name:'取消',exact:true}).click();
 await until(async()=>(await invoke(authPage,'sessions.list',{workspaceId:authServer.id})).some(s=>s.sessionId===waiting.sessionId&&s.status==='canceled'));
 const prior=await until(async()=>(await events({type:'plugin.sodeck.terminal.session.open.completed'})).find(e=>e.attributes.sessionId===waiting.sessionId));
 assert.equal(prior.outcome,'unknown');assert.ok(prior.errorCode.startsWith('AUTH_'));
 assert.equal((await rpc(app.page,'resources.list')).ptys.filter(p=>p.serverId===authServer.id).length,0);
 await rpc(app.page,'auth.ssh.set',{serverId:authServer.id,privateKeyPath:node.key});
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.reconnect',viewId:waiting.viewId,operationId:crypto.randomUUID()});
 await until(async()=>(await rpc(app.page,'resources.list')).ptys.some(p=>p.serverId===authServer.id),45000);
 const retried=await until(async()=>(await events({type:'plugin.sodeck.terminal.session.open.completed'})).find(e=>e.attributes.sessionId===waiting.sessionId&&e.outcome==='succeeded'));
 assert.notEqual(retried.attributes.attemptId,prior.attributes.attemptId);assert.equal(retried.attributes.retryOf,prior.attributes.attemptId);assert.notEqual(retried.traceId,prior.traceId);
 assert.equal((await rpc(app.page,'resources.list')).ptys.filter(p=>p.serverId===server.id).length,1);
 await rpc(app.page,'workspaces.close',{workspaceId:authServer.id});await rpc(app.page,'workspaces.open',{serverId:server.id});
 note('A second server waits for authentication, cancels without PTY, then retries the same logical session with a new attempt; the first server remains connected');
 let files=await pageFor('sodeck.files',server.id),terminal=await pageFor('sodeck.terminal',server.id);
 await until(async()=>['ready','partialError'].includes((await invoke(files,'files.snapshot')).state),45000);
 const type='plugin.sodeck.files.directory.operation.completed';
 const initial=(await events({type})).filter(e=>e.attributes.operation==='locate').length;
 for(let i=0;i<2;i++){
  const receipt=await invoke(files,'files.locate',{text:bodies.path});assert.equal(receipt.accepted,true);
  await until(async()=>(await events({type})).filter(e=>e.attributes.operation==='locate').length>=initial+i+1,30000);
 }
 const failures=(await events({type})).filter(e=>e.attributes.operation==='locate').slice(0,2);
 assert.equal(failures.length,2);assert.ok(failures.every(e=>e.outcome==='failed'&&e.traceId));assert.notEqual(failures[0].traceId,failures[1].traceId);assert.notEqual(failures[0].spanId,failures[1].spanId);
 for(const failure of failures){const trace=await rpc(app.page,'traces.get',{traceId:failure.traceId});assert.ok(trace.items.some(e=>e.kind==='call'&&e.outcome==='failed'));}
 await invoke(files,'files.locate',{text:'/'});await until(async()=>(await events({type})).some(e=>e.attributes.operation==='locate'&&e.outcome==='succeeded'),30000);
 note('Real missing directory: accepted then failed twice with distinct traces/spans; a later valid location succeeds');
 await terminal.locator('.xterm-helper-textarea').fill(`printf '${bodies.terminal}\\n'`);await terminal.locator('.xterm-helper-textarea').press('Enter');
 await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes(bodies.terminal),15000);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});
 let proxy=await pageFor('sodeck.network-proxy',server.id);await until(()=>invoke(proxy,'network.snapshot'));
 const rule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:bodies.rule,type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}});
 const proxyView=await proxy.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:proxyView,operationId:crypto.randomUUID(),args:{id:rule.id}});
 const ptBefore=(await rpc(app.page,'resources.list')).ptys;
 const openedBefore=(await events({type:'plugin.sodeck.terminal.session.open.completed'})).length;
 await until(async()=>(await events({type:'plugin.sodeck.network-proxy.run.start.completed'})).some(e=>e.outcome==='succeeded'));
 const stoppedBefore=(await events({type:'plugin.sodeck.network-proxy.run.stop.completed'})).length;
 await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});
 const windowId=(await rpc(app.page,'workbench.snapshot')).workspaces.find(w=>w.workspaceId===server.id||w.serverId===server.id).ownerWindowId;
 assert.notEqual(windowId,'main');
 const child=await until(async()=>{for(const page of app.browser.contexts().flatMap(c=>c.pages()))if(!page.isClosed()&&await page.evaluate(()=>window.__SODECK_CONTEXT__?.windowId).catch(()=>null)===windowId)return page;});
 terminal=await pageFor('sodeck.terminal',server.id);files=await pageFor('sodeck.files',server.id);proxy=await pageFor('sodeck.network-proxy',server.id);
 await invoke(files,'files.refresh');await until(async()=>(await invoke(files,'files.snapshot')).state==='ready');
 const ptAfter=(await rpc(app.page,'resources.list')).ptys;
 const ptyIdentity=({bufferedBytes,queuedInputBytes,...identity})=>identity;
 assert.deepEqual(ptAfter.map(ptyIdentity),ptBefore.map(ptyIdentity));
 assert.ok(ptAfter.every(p=>p.bufferedBytes>=0&&p.bufferedBytes<=4*1024*1024&&p.queuedInputBytes>=0));
 assert.equal((await events({type:'plugin.sodeck.terminal.session.open.completed'})).length,openedBefore);
 assert.equal((await events({type:'plugin.sodeck.network-proxy.run.stop.completed'})).length,stoppedBefore);
 note('Moving the live workspace preserves terminal PTY and does not invent terminal reconnect or proxy stop');
 await delay(1500);
 const evidence=await rpc(child,'evidence.create',{filter:{excludeDiagnosticCalls:true},allowPartial:true});
 await until(async()=>['succeeded','partial'].includes((await rpc(app.page,'evidence.get',{jobId:evidence.jobId})).job.state),30000);
 let offset=0,text='';for(;;){const result=await rpc(app.page,'evidence.get',{jobId:evidence.jobId,offset});text+=result.chunks.join('');if(result.nextOffset===null)break;offset=result.nextOffset;}
 for(const marker of markers)assert.ok(!text.includes(marker));await writeFile(join(output,'evidence.jsonl'),text);
 const scannedLive=await scan(join(data,'observability'));
 note('Real terminal input/output, file path/base64 and rule name are absent from live observation DB/WAL and exported evidence');
 exportJob=await rpc(child,'evidence.export',{jobId:evidence.jobId,format:'jsonl'});
 await writeFile(join(output,'state.json'),JSON.stringify({data,pid:app.process.pid,windowId,exportJob,readyForSourceClose:true},null,2));
 if(process.env.SODECK_TEST_NATIVE_INSPECT==='1')await until(async()=>JSON.parse(await readFile(join(output,'command.json'),'utf8')).action==='close-source',180000);
 else await delay(1000);
 const closeStarted=Date.now();await rpc(app.page,'windows.close',{windowId});
 await until(()=>child.isClosed());
 const canceled=await until(async()=>{const result=(await rpc(app.page,'evidence.get',{jobId:exportJob.jobId})).job;return result.state==='canceled'&&result;},15000);
 const closeMs=Date.now()-closeStarted;assert.ok(closeMs<=10000,`Source close took ${closeMs}ms`);
 assert.equal(app.process.exitCode,null);assert.ok((await rpc(app.page,'app.info')).version);
 await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===0);
 note('Closing only the export source child window cancels its modal/job while the main App remains responsive');
 if(process.env.SODECK_TEST_WORKER_CRASH==='1'){
  await rpc(app.page,'workspaces.open',{serverId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.some(p=>p.serverId===server.id),45000);
  const activeTerminal=await pageFor('sodeck.terminal',server.id);
  const active=await pluginRpc(activeTerminal,'traces.begin',{clientOperationId:'worker-crash-inflight',actionType:'plugin.sodeck.terminal.session.open',trigger:'plugin',fields:{}});
  const worker=(await rpc(app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.terminal');assert.ok(Number.isInteger(worker.processId)&&worker.processId>0);
  execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${worker.processId} -ErrorAction Stop`],{windowsHide:true});
  await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===0,10000);
  const interrupted=await until(async()=>{const trace=await rpc(app.page,'traces.get',{traceId:active.traceId});return trace.items.some(e=>e.outcome==='interrupted')&&trace;},10000);
  assert.ok(!interrupted.items.some(e=>e.outcome==='succeeded'));
  await until(async()=>(await events({type:'core.plugin.failed',pluginId:'sodeck.terminal'})).length>0,10000);
  assert.ok((await rpc(app.page,'app.info')).version);
  note('Killing the exact terminal worker interrupts its active diagnostic stage and releases its real PTY while the main App survives');
 }
 await stop(app);assert.equal(app.shutdown.forced,false);
 const scannedAfter=await scan(join(data,'observability'));
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,data,checks,failures:failures.map(e=>({eventId:e.eventId,traceId:e.traceId,spanId:e.spanId,outcome:e.outcome})),export:canceled,closeMs,scannedLive,scannedAfter,shutdown:app.shutdown},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({data,checks,error:String(error),code:error.code,stack:error.stack},null,2));throw error;}
finally{await stop(app);for(const browser of browsers.values())await browser.close().catch(()=>{});}
