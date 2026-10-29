// T-028: real Windows host, unchanged API 6.1 packages and isolated state.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {launch,rpc,until,stop,nativeWindow} from './harness.mjs';
import {fixturePackage} from './settings-package-mutate.mjs';
const output=resolve('.qa/t028/native-'+Date.now()),data=join(output,'data');await mkdir(output,{recursive:true});
const baseline=resolve('.local/dev/SoDeck-v2-T027-r9/resources');
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy','monitor'].map(n=>join(baseline,n+'.sodeck-plugin')).join(delimiter);
const checks=[],hash=bytes=>createHash('sha256').update(bytes).digest('hex'),binaryHash=hash(await readFile('target/debug/sodeck-desktop.exe'));
let app,settings,failure;
const note=s=>{checks.push(s);console.log(s);};
async function start(options={}){app=await launch({data,shellPort:13111,pluginPort:13112,...options});settings=undefined;return app;}
async function close(){await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;}
async function list(){return (await rpc(app.page,'plugins.management.list')).items;}
async function instances(){return (await rpc(app.page,'diagnostics.get')).pluginInstances;}
async function save(changes){const p=await rpc(app.page,'plugins.enablement.get');const request={generation:p.generation,expectedRevision:p.revision,changes,operationId:crypto.randomUUID()};await rpc(app.page,'plugins.enablement.validate',{generation:p.generation,expectedRevision:p.revision,changes});const saved=await rpc(app.page,'plugins.enablement.commit',request);assert.deepEqual(await rpc(app.page,'plugins.enablement.commit',request),saved);return request;}
async function prepare(path){const operationId=crypto.randomUUID();await rpc(app.page,'plugins.package.prepare',{operationId,path});return until(async()=>{const p=await rpc(app.page,'plugins.package.operation.get',{operationId});return !['selecting','verifying'].includes(p.state)&&p;},30000);}
async function openSettings(){await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:plugins'});settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});await settings.getByRole('heading',{name:'插件管理',exact:true}).waitFor();return settings;}
async function closeSettings(){await settings.getByRole('button',{name:'取消',exact:true}).click();await until(()=>settings.isClosed());settings=undefined;}
try{
  await start();let rows=await list();assert.equal(rows.length,4);assert.ok(rows.every(r=>r.runtimeStatus==='running'&&r.selected.signature.status==='developmentVerified'));
  const initial=await instances();assert.equal(initial.length,4);note('Unchanged API 6.1 terminal/files/proxy/monitor archives run in the API 6.2 Windows host');
  await openSettings();await settings.getByLabel('启用 终端',{exact:true}).uncheck();await settings.getByRole('button',{name:'应用与数据',exact:true}).click();await settings.getByRole('button',{name:/^插件管理/}).click();assert.equal(await settings.getByLabel('启用 终端',{exact:true}).isChecked(),false);
  await settings.getByRole('button',{name:'取消',exact:true}).click();await settings.getByRole('button',{name:'继续编辑',exact:true}).click();assert.equal(await settings.getByLabel('启用 终端',{exact:true}).isChecked(),false);
  await settings.getByRole('button',{name:'取消',exact:true}).click();await settings.getByRole('button',{name:'放弃更改',exact:true}).click();await until(()=>settings.isClosed());assert.equal((await list()).find(r=>r.pluginId==='sodeck.terminal').enabled,true);
  await openSettings();await settings.getByLabel('启用 终端',{exact:true}).uncheck();await settings.getByRole('button',{name:'应用',exact:true}).click();await until(async()=>!(await list()).find(r=>r.pluginId==='sodeck.terminal').enabled);
  await until(async()=>(await settings.locator('.settings-footer').innerText()).includes('更改已应用'));assert.deepEqual(await instances(),initial);
  await settings.screenshot({path:join(output,'saved-pending-restart.png')});await closeSettings();note('Native settings draft/navigation/close-discard and Apply preserve current worker instances');
  await close();await start();rows=await list();assert.equal(rows.find(r=>r.pluginId==='sodeck.terminal').runtimeStatus,'disabled');assert.equal((await instances()).length,3);
  const catalog=await rpc(app.page,'settings.catalog.list');const terminalSettings=catalog.items.find(p=>p.pageId==='plugin:sodeck.terminal:general');assert.equal(terminalSettings.available,false);assert.match(terminalSettings.reason,/停用/);
  await assert.rejects(rpc(app.page,'settings.page.get',{pageId:terminalSettings.pageId}),e=>e.code==='PLUGIN_UNAVAILABLE');assert.equal((await instances()).length,3);
  note('Restart disables only terminal; its static settings remain visible without starting its provider');
  const unsigned=join(output,'terminal-unsigned.sodeck-plugin');
  await writeFile(unsigned,await fixturePackage(join(baseline,'terminal.sodeck-plugin'),entries=>{entries.delete('signature.json');const manifest=JSON.parse(entries.get('manifest.json'));manifest.version='0.13.2';entries.set('manifest.json',Buffer.from(JSON.stringify(manifest)));const inventory=JSON.parse(entries.get('integrity.json'));inventory['manifest.json']=hash(entries.get('manifest.json'));entries.set('integrity.json',Buffer.from(JSON.stringify(inventory)));},{resign:false}));
  const result=await prepare(unsigned);assert.equal(result.state,'prepared');assert.equal(result.signature.status,'unsigned');assert.equal((await list()).find(r=>r.pluginId==='sodeck.terminal').selected.signature.status,'developmentVerified');
  for(let i=0;i<2;i++){await close();await start();const terminal=(await list()).find(r=>r.pluginId==='sodeck.terminal');assert.equal(terminal.pending.version,'0.13.2');assert.equal(terminal.selected.version,'0.13.1');assert.equal(terminal.runtimeStatus,'disabled');assert.equal((await readdir(join(data,'plugins/data/sodeck.terminal'))).length,1);}
  note('Unsigned candidate remains pending across two disabled startups: no worker or candidate data generation');
  const broken=join(output,'bad.sodeck-plugin');await writeFile(broken,await fixturePackage(unsigned,e=>e.set('worker/main.js',Buffer.from('throw Error("INVALID_EXECUTION")')),{resign:false}));
  const bad=await prepare(broken);assert.equal(bad.state,'failed');assert.equal(bad.code,'PACKAGE_INTEGRITY_FAILED');assert.equal((await list()).find(r=>r.pluginId==='sodeck.terminal').pending.version,'0.13.2');
  await save([{pluginId:'sodeck.terminal',enabled:true}]);assert.equal((await instances()).length,3);await close();await start();assert.equal((await list()).find(r=>r.pluginId==='sodeck.terminal').selected.signature.status,'unsigned');assert.equal((await instances()).length,4);
  note('Bad content cannot replace pending; manual signed-to-unsigned update activates only after enabled restart');
  await openSettings();
  for(const theme of ['soft-light','soft-dark','dopamine']){const {snapshot:s}=await rpc(app.page,'settings.page.get',{pageId:'core:appearance'});await rpc(app.page,'settings.commit',{pageId:s.pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,changes:[{key:'themeId',value:theme}],operationId:crypto.randomUUID()});await settings.waitForFunction(t=>document.documentElement.dataset.theme===t,theme);await settings.screenshot({path:join(output,'plugins-'+theme+'.png')});}
  await settings.evaluate(()=>{document.documentElement.style.fontSize='26px';});assert.equal(await settings.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await settings.screenshot({path:join(output,'plugins-scale-200.png')});await closeSettings();
  const before=await rpc(app.page,'plugins.enablement.get');const all=(await list()).map(r=>({pluginId:r.pluginId,enabled:false}));const request=await save(all);
  await assert.rejects(rpc(app.page,'plugins.enablement.commit',{...request,changes:[{pluginId:'sodeck.terminal',enabled:true}]}),e=>e.code==='CONFLICT');
  await assert.rejects(rpc(app.page,'plugins.enablement.validate',{generation:before.generation,expectedRevision:before.revision,changes:all}),e=>e.code==='CONFLICT');
  await close();await start();assert.deepEqual(await instances(),[]);assert.ok((await list()).every(r=>!r.enabled&&r.runtimeStatus==='disabled'));
  const server=await rpc(app.page,'servers.routes.create',{name:'T028 all disabled',host:'127.0.0.1',port:1,username:'fixture',accessPath:{steps:[]}});await rpc(app.page,'workspaces.open',{serverId:server.id});
  const workbench=await rpc(app.page,'workbench.snapshot');assert.equal(workbench.views.length,0);assert.ok(workbench.workspaces.every(w=>!w.initializationError));assert.deepEqual((await rpc(app.page,'resources.list')).ptys,[]);
  await openSettings();await settings.screenshot({path:join(output,'all-disabled.png')});await closeSettings();note('All plugins disabled is a normal empty workspace; server/settings core remains usable with zero plugin processes');
  await close();const preferencePath=join(data,'plugins/enablement.json');await writeFile(preferencePath,'{broken');await start();assert.deepEqual(await instances(),[]);const corrupt=await rpc(app.page,'plugins.enablement.get');assert.equal(corrupt.storageStatus,'corrupt');
  const repair=await rpc(app.page,'plugins.enablement.repair',{operationId:crypto.randomUUID(),diagnosticToken:corrupt.diagnosticToken});assert.equal(await readFile(repair.backupPath,'utf8'),'{broken');assert.ok((await list()).every(r=>!r.enabled));
  await close();await writeFile(preferencePath,JSON.stringify({formatVersion:99}));await start();assert.equal((await rpc(app.page,'plugins.enablement.get')).storageStatus,'newer');assert.deepEqual(await instances(),[]);await close();
  note('Damaged configuration fails closed, explicit repair preserves original bytes and disables all; newer format remains read-only');
  assert.equal(hash(await readFile('target/debug/sodeck-desktop.exe')),binaryHash);note('Core executable unchanged throughout independent package updates and restarts');
}catch(e){failure={message:e.message,stack:e.stack};if(settings&&!settings.isClosed())await settings.screenshot({path:join(output,'failure.png')}).catch(()=>{});throw e;}
finally{if(app){if(settings&&!settings.isClosed()){await settings.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});await settings.getByRole('button',{name:'放弃更改',exact:true}).click({timeout:1000}).catch(()=>{});}await stop(app);}const result={passed:!failure,output,checks,failure,binaryHash};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t028/native-latest.json',JSON.stringify(result,null,2));}
