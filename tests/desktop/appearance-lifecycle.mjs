import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,until,stop} from './harness.mjs';

const output=resolve('.qa/t011/lifecycle');await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t010/linux/test-config.json','utf8'));
assert.equal(config.host,'127.0.0.1');assert.equal(config.port,22281);
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9902,'sodeck.terminal':9903});
delete process.env.SODECK_THEME_FILE;
const samples=[];let app;const browsers=[];
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9901,pluginPort:9902});
 const servers=[];
 for(let i=0;i<2;i++){
  const server=await rpc(app.page,'servers.create',{name:`主题生命周期 ${i+1}`,host:config.host,port:config.port,username:config.username,authType:'key'});
  await rpc(app.page,'auth.set',{serverId:server.id,privateKeyPath:config.privateKeyPath,remember:true});
  try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(error){assert.equal(error.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...error.details});}servers.push(server);
 }
 for(let cycle=0;cycle<3;cycle++){
  await rpc(app.page,'workspaces.open',{serverId:servers[0].id});await rpc(app.page,'workspaces.open',{serverId:servers[1].id,target:'newWindow'});
  const child=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(p!==app.page&&!p.isClosed()&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId))return p;});
  const plugins=[];
  for(const port of [9902,9903]){const browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));browsers.push(browser);await until(()=>browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('sodeck')).length===2);plugins.push(...browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('sodeck')));}
  const surfaces=[app.page,child,...plugins];assert.equal(surfaces.length,6);
  await until(async()=>{for(const page of [app.page,child]){const wb=await rpc(page,'workbench.snapshot');if(wb.pendingOpens.length||!wb.views.every(v=>v.status===(v.pluginId==='sodeck.files'?'ready':'running')))return false;}return true;},30000);
  await until(async()=>(await rpc(app.page,'diagnostics.get')).communication.contracts['app.theme.v1']===6);
  for(const mode of ['light','dark']){const pref=await rpc(child,'ui.theme.preference.get');const next=await rpc(child,'ui.theme.select',{mode,expectedRevision:pref.revision});await until(async()=>(await Promise.all(surfaces.map(p=>p.evaluate(()=>document.documentElement.dataset.themeRevision)))).every(r=>r===next.revision));}
  await rpc(child,'windows.merge',{operationId:crypto.randomUUID()});for(const server of servers)await rpc(app.page,'workspaces.close',{workspaceId:server.id});
  const start=performance.now();await until(async()=>(await rpc(app.page,'diagnostics.get')).communication.contracts['app.theme.v1']===1,1000);
  const cleanupMs=performance.now()-start,stats=(await rpc(app.page,'diagnostics.get')).communication;
  assert.ok(cleanupMs<=1000);assert.ok(stats.queueBytes<=8*1024*1024);assert.equal(stats.subscriptions,1);
  await until(async()=>{const r=await rpc(app.page,'resources.list');return ['ptys','connections','leases'].every(k=>r[k].length===0);});
  samples.push({cycle,surfaces:surfaces.length,cleanupMs,stats});console.log(`cycle ${cycle+1}: 6 surfaces return to one subscription in ${cleanupMs.toFixed(1)} ms`);
  for(const browser of browsers.splice(0))await browser.close().catch(()=>{});
 }
 await stop(app);assert.equal(app.shutdown.forced,false);assert.equal(app.process.exitCode,0);app=null;
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,samples,checks:['three six-surface create/switch/close cycles','one subscription per surface; cleanup below one second','PTY, lease and connection counts return to zero; normal process exit']},null,2));
}finally{await stop(app);for(const browser of browsers)await browser.close().catch(()=>{});}
