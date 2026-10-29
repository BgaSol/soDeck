import assert from 'node:assert/strict';
import {readFile,writeFile,cp,readdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync,execFileSync} from 'node:child_process';
import {build} from 'esbuild';
import {fixturePackage} from './settings-package-mutate.mjs';
import {monitorHarness,invoke,snapshot} from './monitor-harness.mjs';
import {rpc,until} from './harness.mjs';
const h=await monitorHarness('updates',19041),checks=[],source=join(h.output,'candidate-source'),data=join(h.output,'data-'+Date.now());let failure,server;
const core=resolve(process.env.SODECK_MONITOR_RUNTIME??'.qa/t024/runtime-monitor-r7/sodeck-desktop.exe');
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const paths=[core,...['terminal','files','network-proxy','monitor'].map(n=>resolve('.qa/t024/packages',n+'.sodeck-plugin'))];const before=await Promise.all(paths.map(hash));
await cp('plugins/monitor/dist',source,{recursive:true});
await build({entryPoints:['plugins/monitor/src/worker.ts'],outfile:join(source,'worker/main.js'),bundle:true,format:'iife',platform:'neutral',define:{__MONITOR_VERSION__:'"0.1.1"'},plugins:[{name:'candidate-parser',setup(b){b.onLoad({filter:/metrics[\\/]parse\.ts$/},async args=>({loader:'ts',resolveDir:resolve('plugins/monitor/src/metrics'),contents:(await readFile(args.path,'utf8')).replace("host:displayText((values.host??'').trim())","host:'更新验证 · '+displayText((values.host??'').trim())")}));}}]});
for(const name of await readdir(join(source,'ui/assets')))if(name.endsWith('.js')){const path=join(source,'ui/assets',name);await writeFile(path,(await readFile(path,'utf8')).replaceAll('资源监控','资源监控（更新验证）').replaceAll('"0.1.0"','"0.1.1"'));}
const candidate=join(h.output,'monitor-0.1.1.sodeck-plugin'),packed=spawnSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','monitor','--source',source,'--version','0.1.1','--out',candidate],{encoding:'utf8',windowsHide:true});assert.equal(packed.status,0,packed.stderr);
const badSignature=join(h.output,'bad-signature.sodeck-plugin');await writeFile(badSignature,await fixturePackage(candidate,e=>{const s=JSON.parse(e.get('signature.json'));s.signature=(s.signature[0]==='A'?'B':'A')+s.signature.slice(1);e.set('signature.json',Buffer.from(JSON.stringify(s)));},{resign:false}));
const incompatible=join(h.output,'incompatible.sodeck-plugin');await writeFile(incompatible,await fixturePackage(candidate,e=>{const m=JSON.parse(e.get('manifest.json'));m.requires.pluginApi='^99.0.0';e.set('manifest.json',Buffer.from(JSON.stringify(m)));}));
const broken=join(h.output,'prepare-failure.sodeck-plugin');await writeFile(broken,await fixturePackage(candidate,e=>{const m=JSON.parse(e.get('manifest.json'));m.version='0.1.2';e.set('manifest.json',Buffer.from(JSON.stringify(m)));e.set('worker/main.js',Buffer.concat([e.get('worker/main.js'),Buffer.from('\nsodeckPlugin.prepare=()=>{throw Error("T024_PREPARE_FAILURE")};')]));}));
const start=()=>h.start({data,allowStartupWarning:true});const status=async()=>(await rpc(h.app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.monitor');
const open=async()=>{await rpc(h.app.page,'workspaces.open',{serverId:server.id});const page=await h.view('sodeck.monitor',server.id);await until(async()=>(await snapshot(page)).cpu.quality==='fresh',40000);return page;};
try{
 await start();server=await h.create('T024 update');let page=await open();const pty=(await rpc(h.app.page,'resources.list')).ptys[0].id;
 await rpc(h.app.page,'plugins.prepareUpdate',{path:candidate});assert.equal((await status()).activeVersion,'0.1.0');assert.equal((await status()).pendingVersion,'0.1.1');assert.equal((await invoke(page,'monitor.diagnostics')).version,'0.1.0');assert.equal((await rpc(h.app.page,'resources.list')).ptys[0].id,pty);
 await assert.rejects(rpc(h.app.page,'plugins.prepareUpdate',{path:badSignature}),e=>e.code==='PACKAGE_INVALID');await assert.rejects(rpc(h.app.page,'plugins.prepareUpdate',{path:incompatible}));assert.equal((await status()).pendingVersion,'0.1.1');
 checks.push('prepare leaves active 0.1.0 and terminal unchanged; signature/incompatible rejection preserves pending 0.1.1');
 await h.stop();await start();assert.equal((await status()).activeVersion,'0.1.1');page=await open();assert.match((await snapshot(page)).system.host,/^更新验证/);await page.getByText('资源监控（更新验证）',{exact:true}).waitFor();await page.screenshot({path:join(h.output,'updated-monitor.png')});
 checks.push('restart activates independently changed UI and inventory parser in 0.1.1');
 await rpc(h.app.page,'plugins.prepareUpdate',{path:broken});await h.stop();await start();assert.equal((await status()).activeVersion,'0.1.1');assert.match((await status()).error,/T024_PREPARE_FAILURE/);page=await open();assert.match((await snapshot(page)).system.host,/^更新验证/);checks.push('failed candidate prepare falls back to previous healthy monitor');
 const terminal=await h.view('sodeck.terminal',server.id),original=(await rpc(h.app.page,'resources.list')).ptys[0].id,victim=(await rpc(h.app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.monitor');assert.ok(victim.processId);
 execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-CimInstance Win32_Process -Filter 'ProcessId=${Number(victim.processId)}'; if($p.CommandLine -notmatch '--sodeck-internal-plugin-host' -or $p.ExecutablePath -notmatch 't024'){throw 'Unexpected process'}; Stop-Process -Id $p.ProcessId`],{windowsHide:true});
 await until(async()=>(await rpc(h.app.page,'workbench.snapshot')).views.some(v=>v.pluginId==='sodeck.monitor'&&v.status==='viewFailed'),15000);assert.equal((await rpc(h.app.page,'resources.list')).ptys[0].id,original);
 await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'MONITOR-CRASH-PTY-OK\\n'\r");await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('MONITOR-CRASH-PTY-OK'));
 const failed=(await rpc(h.app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.monitor');await rpc(h.app.page,'views.recover',{viewId:failed.viewId});page=await h.view('sodeck.monitor',server.id);await until(async()=>(await snapshot(page)).cpu.quality==='fresh',40000);checks.push('exact monitor worker crash preserves terminal; recover starts a fresh sampler');
 assert.deepEqual(await Promise.all(paths.map(hash)),before);
}catch(error){failure={message:error.message,stack:error.stack,workbench:await rpc(h.app.page,'workbench.snapshot').catch(()=>null)};if(server){const p=await h.view('sodeck.monitor',server.id).catch(()=>null);if(p)failure.body=await p.locator('body').innerText();}throw error;}
finally{await writeFile(join(h.output,'results.json'),JSON.stringify({checks,unchangedFiles:paths.map((path,i)=>({path,sha256:before[i]})),failure},null,2));await h.stop();}
