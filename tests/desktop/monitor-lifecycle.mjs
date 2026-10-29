import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {monitorHarness,invoke,snapshot} from './monitor-harness.mjs';
import {rpc,until,delay,nativeWindow} from './harness.mjs';
const h=await monitorHarness('lifecycle',19021),checks=[],cycles=[];let failure,stage='startup';
try{
 const app=await h.start(),server=await h.create('T024 lifecycle');await rpc(app.page,'workspaces.open',{serverId:server.id});let page=await h.view('sodeck.monitor',server.id);
 await until(async()=>(await snapshot(page)).trends.cpu.length>=5,30000);
 const terminal=(await rpc(app.page,'resources.list')).ptys[0].id;const monitorOwner=(await rpc(app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.monitor').instanceId;assert.ok(monitorOwner);
 let viewId=await page.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await invoke(page,'monitor.setOptions',{options:{rangeSeconds:300}});
 for(const placement of ['detached','center','right']){stage=placement;
  const old=await snapshot(page),before=old.trends.cpu[0].time+old.timelineMs;
  await rpc(app.page,'views.move',{viewId,placement,operationId:crypto.randomUUID()});page=await h.view('sodeck.monitor',server.id);await rpc(app.page,'views.activate',{viewId});
  await until(async()=>(await snapshot(page)).visible,10000);const s=await snapshot(page);assert.equal(s.options.rangeSeconds,300);assert.equal(s.trends.cpu[0].time+s.timelineMs,before);
 }
 checks.push('dock→native detached→center→dock keeps options/history and server identity');
 await rpc(app.page,'app.hide');await until(async()=>{const s=await snapshot(page);return !s.visible&&!s.cleanupPending;},10000);const hidden=await snapshot(page);await delay(2300);assert.equal((await snapshot(page)).cpu.updatedAt,hidden.cpu.updatedAt);
 const windows=await nativeWindow(app,'list'),main=windows.find(w=>w.title==='SoDeck v2')??windows[0];await nativeWindow(app,'show',{WindowHandle:main.handle});
 await until(async()=>(await snapshot(page)).connection==='connected',20000);checks.push('tray hide stops I/O and restore resumes existing connection');
 for(let i=0;i<100;i++){
  await rpc(app.page,'views.close',{viewId});
  await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.leases.some(l=>l.instanceId===monitorOwner);},10000);
  const opened=await rpc(app.page,'commands.execute',{pluginId:'sodeck.monitor',commandId:'monitor.open',workspaceId:server.id,operationId:crypto.randomUUID()});viewId=opened.viewId;
  page=await h.view('sodeck.monitor',server.id);await until(async()=>{const s=await snapshot(page);return s.connection==='connected'&&s.cpu.updatedAt!==null;},25000);
  const resources=await rpc(app.page,'resources.list');assert.ok(resources.ptys.some(p=>p.id===terminal));const d=await invoke(page,'monitor.diagnostics');assert.equal(d.controllers,1);assert.ok(d.rpc.pending<=16);cycles.push({cycle:i+1,leases:resources.leases.length,...d});
  if(i%10===0)await writeFile(join(h.output,'progress.json'),JSON.stringify({completed:i+1,last:cycles.at(-1)},null,2));
 }
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);checks.push('100 native close/reopen cycles retain terminal PTY and bounded single monitor controller; final leases zero');
}catch(error){failure={stage,message:error.message,stack:error.stack,workbench:await rpc(h.app.page,'workbench.snapshot').catch(()=>null)};throw error;}
finally{await writeFile(join(h.output,'results.json'),JSON.stringify({checks,cycles,failure},null,2));await h.stop();}
