import assert from 'node:assert/strict';
import {readFile,writeFile,cp,mkdir} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {spawnSync,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import os from 'node:os';
import {build} from 'esbuild';
import {monitorHarness,invoke,snapshot} from './monitor-harness.mjs';
import {rpc,pluginRpc,until,delay} from './harness.mjs';
const h=await monitorHarness(process.env.SODECK_MONITOR_LOAD_NAME??'load',18901),source=join(h.output,'fixture-source'),checks=[],samples=[],servers=[];
await mkdir(source,{recursive:true});await cp('plugins/monitor/dist',source,{recursive:true});
await build({entryPoints:['plugins/monitor/src/worker.ts'],outfile:join(source,'worker/main.js'),bundle:true,format:'iife',platform:'neutral',target:'es2022',define:{__MONITOR_VERSION__:'"0.1.0-load"'},plugins:[{name:'test-only-transport',setup(b){b.onLoad({filter:/monitor[\\/]src[\\/]worker\.ts$/},async args=>({loader:'ts',resolveDir:resolve('plugins/monitor/src'),contents:`import {fixtureTransport} from '../../../tests/desktop/monitor-load-transport';\n`+(await readFile(args.path,'utf8')).replace('workerClient(runtime.sodeck)','workerClient(fixtureTransport(runtime.sodeck))')}));}}]});
await writeFile(join(source,'worker/main.js'),(await readFile(join(source,'worker/main.js'),'utf8'))+`\n{const invoke=sodeckPlugin.invoke;sodeckPlugin.invoke=async function(command,p){if(command==='qa.interrupt'){globalThis.__monitorLoadFault(p.workspaceId);return {accepted:true};}return invoke(command,p);};}\n`);
const archive=join(h.output,'monitor-load.sodeck-plugin');const packed=spawnSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','monitor','--source',source,'--version','0.1.0-load','--out',archive],{encoding:'utf8',windowsHide:true});assert.equal(packed.status,0,packed.stderr+packed.stdout);
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(p=>resolve('.qa/t024/packages',p+'.sodeck-plugin')).concat(archive).join(delimiter);
let failure,start=0;
try{
  const app=await h.start(),pages=[];
  for(let i=0;i<8;i++){
    const server=await h.create(`CONTROLLED monitor load ${i+1}`);servers.push(server);await rpc(app.page,'workspaces.open',{serverId:server.id});
    let page=await h.view('sodeck.monitor',server.id);await until(async()=>(await snapshot(page)).connection==='connected',30000);
    await until(async()=>!(await rpc(app.page,'workbench.snapshot')).pendingOpens.length,30000);
    if(i<7)await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});
    page=await h.view('sodeck.monitor',server.id);await until(async()=>{const s=await snapshot(page);return s.visible&&s.connection==='connected'&&!s.cleanupPending&&s.cpu.quality==='fresh'&&s.mountCount===128&&s.diskCount===64;},45000);pages.push(page);
    if(i<7){const views=(await rpc(app.page,'workbench.snapshot')).views;for(const view of views.filter(v=>v.workspaceId===server.id&&['sodeck.files','sodeck.terminal'].includes(v.pluginId)))await rpc(app.page,'views.close',{viewId:view.viewId});}
  }
  checks.push('eight visible native monitor windows, each with 64 interfaces, 64 devices and 128 controlled mounts over actual SSH exec');
  const terminal=await h.view('sodeck.terminal',servers[7].id),files=await h.view('sodeck.files',servers[7].id);
  await invoke(files,'files.locate',{text:'/var/tmp/sodeck-monitor-load'});
  const fileState=await until(async()=>{const s=await invoke(files,'files.snapshot');return s.path==='/var/tmp/sodeck-monitor-load'&&!s.loading?s:false;},45000);
  await invoke(files,'files.toggle',{id:fileState.selected});
  await until(async()=>(await invoke(files,'files.snapshot')).rows.some(r=>r.name==='payload.bin'),30000);
  const pty=(await rpc(app.page,'resources.list')).ptys.find(p=>p.serverId===servers[7].id);let inputSeq=0;
  const opened=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:servers[7].id,operationId:crypto.randomUUID()});let network=await h.view('sodeck.network-proxy',servers[7].id);
  await until(()=>invoke(network,'network.snapshot'),20000);
  await rpc(app.page,'views.move',{viewId:opened.viewId,placement:'center',operationId:crypto.randomUUID()});network=await h.view('sodeck.network-proxy',servers[7].id);await until(()=>invoke(network,'network.snapshot'),20000);
  await rpc(app.page,'views.activate',{viewId:await terminal.evaluate(()=>window.__SODECK_CONTEXT__.viewId)});
  await rpc(app.page,'views.activate',{viewId:await pages[7].evaluate(()=>window.__SODECK_CONTEXT__.viewId)});
  const rule=await invoke(network,'network.save',{expectedRevision:0,rule:{name:'Monitor load transfer',type:'port-forward',bindMode:'auto',port:0,autoReconnect:false,targetHost:'127.0.0.1',targetPort:18088}});
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:opened.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});
  const netState=await invoke(network,'network.snapshot',{selected:rule.id});const port=netState.selected.run.address.port;
  start=Date.now();const duration=Number(process.env.SODECK_MONITOR_LOAD_MS??1800000);let faulted=false,reconnectAt=0,recovered=false,iteration=0;
  while(Date.now()-start<duration){
    const began=performance.now();
    await pluginRpc(terminal,'pty.write',{ptyId:pty.id,generation:pty.generation,inputSeq:++inputSeq,bytes:Buffer.from(`printf 'MON-LOAD-${iteration}\\n'\r`).toString('base64')});
    try{await terminal.waitForFunction(text=>document.querySelector('.xterm-rows')?.textContent?.includes(text),`MON-LOAD-${iteration}`,{timeout:10000,polling:100});}
    catch(error){await writeFile(join(h.output,'echo-failure.json'),JSON.stringify({iteration,text:await terminal.locator('body').innerText(),workbench:await rpc(app.page,'workbench.snapshot'),resources:await rpc(app.page,'resources.list')},null,2));throw error;}
    const echoMs=performance.now()-began;
    let refreshUnknown=false;
    try{await invoke(files,'files.refresh',{id:fileState.selected});}catch(error){if(error.code!=='RESULT_UNKNOWN')throw error;refreshUnknown=true;}
    try{await until(async()=>{const s=await invoke(files,'files.snapshot');return s.rows.some(r=>r.name==='payload.bin')&&!s.rows.find(r=>r.id===fileState.selected)?.loading;},20000);}
    catch(error){await writeFile(join(h.output,'files-failure.json'),JSON.stringify(await invoke(files,'files.snapshot'),null,2));throw error;}
    const transferStart=performance.now(),response=await fetch(`http://127.0.0.1:${port}/payload.bin`);assert.equal(response.status,200);const data=await response.arrayBuffer();assert.equal(data.byteLength,1048576);
    const state=await invoke(pages[7],'monitor.diagnostics');assert.ok(state.scheduler.targets<=8&&state.scheduler.fast<=3&&state.scheduler.slow<=1);assert.ok(state.scheduler.transientBytes<=4194304);assert.ok(state.rpc.pending<=16&&state.rpc.ordinary<=12);assert.ok(state.cacheBytes<=33554432);
    const metrics=await Promise.all(pages.map(snapshot));for(const m of metrics){assert.ok(m.diagnostics.cacheBytes<=4194304);assert.ok(m.trends.cpu.length<=301);}
    if(!faulted&&Date.now()-start>30000){await invoke(pages[1],'qa.interrupt',{workspaceId:servers[1].id});faulted=true;reconnectAt=Date.now()+10000;}
    if(faulted&&!recovered&&Date.now()>=reconnectAt){await rpc(app.page,'commands.execute',{pluginId:'sodeck.monitor',commandId:'monitor.connect',workspaceId:servers[1].id,viewId:await pages[1].evaluate(()=>window.__SODECK_CONTEXT__.viewId),operationId:crypto.randomUUID()});await until(async()=>(await snapshot(pages[1])).cpu.quality==='fresh',30000);recovered=true;}
    let rss=null,storage=null;if(iteration%6===0){storage=await rpc(app.page,'observability.v2.health.get');const instance=(await rpc(app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.monitor');if(instance?.processId){const result=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`(Get-Process -Id ${Number(instance.processId)}).WorkingSet64`],{windowsHide:true});rss=Number(result.stdout.trim());}}
    samples.push({elapsedMs:Date.now()-start,echoMs,refreshUnknown,transferMs:performance.now()-transferStart,workerRss:rss,storage,...state,targets:metrics.map(m=>({cpuAt:m.cpu.updatedAt,quality:m.cpu.quality,connection:m.connection,visible:m.visible,status:m.status,error:m.error,points:m.trends.cpu.length,cache:m.diagnostics.cacheBytes,cleanup:m.cleanupPending}))});
    if(iteration%6===0)await writeFile(join(h.output,'progress.json'),JSON.stringify({start,duration,elapsedMs:Date.now()-start,iterations:iteration,last:samples.at(-1)},null,2));
    iteration++;await delay(5000);
  }
  assert.ok(faulted&&recovered,'the controlled interruption must be followed by explicit reconnection');
  for(let i=0;i<8;i++){
    for(let from=0;from<duration;from+=300000){const window=samples.filter(s=>s.elapsedMs>=from&&s.elapsedMs<from+300000);if(window.length>2)assert.ok(new Set(window.map(s=>s.targets[i].cpuAt).filter(v=>v!==null)).size>1,`target ${i+1} must keep progressing in window ${from}`);}
  }
  checks.push('full-duration bounded collection and all eight targets progress in every five-minute window alongside PTY, file refresh, 1 MiB tunnel transfers, slow-capacity timeout and ten-second controlled interruption');
  for(const server of servers){
    await until(async()=>{
      try{return await rpc(app.page,'workspaces.activate',{workspaceId:server.id});}
      catch(error){if(error.code==='CLOSE_REVIEW_REQUIRED')return false;throw error;}
    },30000);
    try{await rpc(app.page,'workspaces.close',{workspaceId:server.id});}
    catch(error){if(error.code!=='CLOSE_REVIEW_REQUIRED')throw error;}
    // Closing a child window coordinates even an empty connection-home page.
    // Its renderer acknowledges asynchronously; never bypass a real draft.
    await until(async()=>!(await rpc(app.page,'workbench.snapshot')).workspaces.some(w=>w.serverId===server.id),30000);
  }
  await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);checks.push('all test leases returned to zero');
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await writeFile(join(h.output,'results.json'),JSON.stringify({environment:{cpu:os.cpus()[0].model,logicalCpus:os.cpus().length,memory:os.totalmem(),platform:os.platform(),fixture:'controlled metrics through real SSH, native QuickJS worker'},checks,start,elapsedMs:start?Date.now()-start:0,samples,failure},null,2));await h.stop();}
