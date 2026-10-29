import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {monitorHarness,snapshot} from './monitor-harness.mjs';
import {rpc,until,delay} from './harness.mjs';
const folder=resolve(process.argv[2]),h=await monitorHarness('package-verification',19081),checks=[];let failure;
const executable=join(folder,'sodeck-desktop.exe'),empty=join(h.output,'empty-cwd');await mkdir(empty,{recursive:true});
const build=JSON.parse(await readFile(join(folder,'build.json'),'utf8'));
try{
 let app=await h.start({ordinary:true,executable,data:join(folder,'data'),cwd:empty,startupTimeout:60000});
 const info=await rpc(app.page,'app.runtimeInfo.get');assert.equal(info.mode,'portable');assert.equal(resolve(info.dataPath),join(folder,'data'));assert.equal(info.version,build.version);
 const plugins=await rpc(app.page,'plugins.list');assert.deepEqual(plugins.map(p=>p.pluginId).sort(),build.pluginPackages.map(p=>p.pluginId).sort());assert.ok(plugins.every(p=>p.activeVersion&&!p.error));
 let server=await h.create('T024 ordinary directory package');await rpc(app.page,'workspaces.open',{serverId:server.id});
 const health=await until(async()=>{const s=await rpc(app.page,'diagnostics.snapshots.query',{pluginId:'sodeck.monitor',type:'monitor.runtime.v1'});return s.items.find(i=>i.state==='current'&&['collecting','partial'].includes(i.fields.state));},45000);
 assert.equal((await rpc(app.page,'resources.list')).ptys.filter(p=>p.state==='active').length,1);
 await writeFile(join(h.output,'ordinary-startup.json'),JSON.stringify({info,plugins,monitorHealth:health},null,2));
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);await h.stop();assert.equal(app.shutdown.forced,false);
 checks.push('ordinary portable startup from unrelated cwd loads four bundled signed plugins; real Linux sampling and terminal start; graceful cleanup');
 app=await h.start({executable,bundledResources:true,cwd:empty});server=await h.create('Linux 资源监控 · 目录包');await rpc(app.page,'workspaces.open',{serverId:server.id});
 const page=await h.view('sodeck.monitor',server.id),terminal=await h.view('sodeck.terminal',server.id);
 await until(async()=>(await snapshot(page)).cpu.quality==='fresh',45000);await until(async()=>/\d+\.\d+%/.test(await page.locator('.monitor-heading').first().innerText()),5000);
 await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'T024-PACKAGE-PTY-OK\\n'\r");await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('T024-PACKAGE-PTY-OK'));
 for(const mode of ['dopamine','light','dark']){
  const pref=await rpc(app.page,'ui.theme.preference.get');const next=await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:pref.revision});
  await until(async()=>await page.evaluate(()=>document.documentElement.dataset.themeRevision)===next.revision);await delay(200);
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:join(h.output,'monitor-'+mode+'.png')});
 }
 const chart=page.getByRole('img',{name:/CPU 使用率趋势/});await chart.focus();await chart.press('Home');assert.match(await page.locator('.sd-chart-readout').first().innerText(),/CPU 使用率/);
 await page.evaluate(()=>document.documentElement.style.fontSize='26px');assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:join(h.output,'monitor-200-text.png')});
 await page.emulateMedia({forcedColors:'active',reducedMotion:'reduce'});await page.screenshot({path:join(h.output,'monitor-forced-colors.png')});
 checks.push('same bundled runtime renders real metrics in three native WebView themes, keyboard chart, 200% CSS text and emulated forced-colors/reduced-motion; terminal stays interactive');
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await h.stop();await writeFile(join(h.output,'results.json'),JSON.stringify({folder,checks,buildHashes:build.hashes,failure},null,2));}
