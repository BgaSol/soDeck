// T-027: isolated API6 plugin updates and v6 backup/rebuild/rollback.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,cp,readdir} from 'node:fs/promises';
import {resolve,join,dirname,delimiter} from 'node:path';
import {createHash} from 'node:crypto';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {fixturePackage} from './settings-package-mutate.mjs';
import {launch,rpc,pluginRpc,stop,until,nativeWindow} from './harness.mjs';

const output=resolve('.qa/t027/package-'+Date.now());await mkdir(output,{recursive:true});
const current=resolve(process.env.SODECK_TEST_PACKAGE??'.local/dev/SoDeck-v2-T027-r6'),previous=resolve('.local/dev/SoDeck-v2-T025');
const names=['terminal','files','network-proxy','monitor'];
const basePackages=names.map(n=>join(current,'resources',n+'.sodeck-plugin'));
const checks=[],evidence={},browsers=new Map();let app,failure;
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const note=s=>{checks.push(s);console.log(s);};
const files=[join(current,'sodeck-desktop.exe'),join(current,'sodeck-plugin-host.exe'),...basePackages];
const before=await Promise.all(files.map(hash));
const powershell=async script=>(await promisify(execFile)('powershell.exe',['-NoProfile','-Command',script],{windowsHide:true})).stdout.trim();
const literal=s=>"'"+s.replaceAll("'","''")+"'";
async function clone(source,name){
  const dir=join(output,name),build=JSON.parse(await readFile(join(source,'build.json'),'utf8'));
  for(const path of [...Object.keys(build.hashes),'build.json']){
    const bytes=await readFile(join(source,path));
    if(build.hashes[path])assert.equal(createHash('sha256').update(bytes).digest('hex'),build.hashes[path],path);
    await mkdir(dirname(join(dir,path)),{recursive:true});await writeFile(join(dir,path),bytes);
  }
  return dir;
}
async function close(){for(const b of browsers.values())await b.close().catch(()=>{});browsers.clear();await stop(app);if(app)assert.equal(app.shutdown.forced,false);app=undefined;}
async function pluginPage(name){
  const id='sodeck.'+name;let browser=browsers.get(name);
  if(!browser){browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+(12862+names.indexOf(name))));browsers.set(name,browser);}
  return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.pluginId)===id){await p.waitForSelector('#root > *');return p;}});
}
async function openViews(server){
  await rpc(app.page,'workspaces.open',{serverId:server.id});
  const win=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck'));
  const scale=await app.page.evaluate(()=>devicePixelRatio);
  await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:1280*scale,Height:850*scale});
  for(const [pluginId,commandId] of [['sodeck.monitor','monitor.open'],['sodeck.network-proxy','network.open']])await rpc(app.page,'commands.execute',{pluginId,commandId,workspaceId:server.id,operationId:crypto.randomUUID()});
}
async function versions(){return Object.fromEntries((await rpc(app.page,'plugins.list')).map(p=>{assert.equal(p.error,null,p.pluginId);return [p.pluginId,p.activeVersion];}));}
async function acknowledgeReset(executable){
  const pid=await until(async()=>{const text=await powershell(`Get-Process sodeck-desktop -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq ${literal(executable)}} | Select-Object -ExpandProperty Id`);return text?Number(text):null;},20000);
  const runtime={process:{pid},executable};
  const dialog=await until(async()=>(await nativeWindow(runtime,'list')).find(w=>w.visible&&w.title==='SoDeck · 数据库版本更新'),30000);
  await nativeWindow(runtime,'capture',{WindowHandle:dialog.handle,OutputPath:join(output,'schema-reset-notice.png')});
  // Standard Windows informational MessageBox: the only content button is OK.
  // The owned window geometry comes from the native harness and is captured above.
  const scale=(dialog.dpi??192)/96;
  await nativeWindow(runtime,'click',{WindowHandle:dialog.handle,X:Math.round(dialog.x+dialog.width-70*scale),Y:Math.round(dialog.y+dialog.height-27*scale)});
  evidence.resetNotice={title:dialog.title,screenshot:join(output,'schema-reset-notice.png')};
}
try{
  let active;
  if(!process.argv.includes('--storage')){
  // Each update changes its own UI and worker, and preserves all production archives.
  process.env.SODECK_PLUGIN_PACKAGES=basePackages.join(delimiter);
  process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(Object.fromEntries(names.map((n,i)=>['sodeck.'+n,12862+i])));
  const data=join(output,'updates-data');
  const start=async()=>app=await launch({data,executable:join(current,'sodeck-desktop.exe'),shellPort:12861,pluginPort:12862});
  await start();active=await versions();assert.equal(Object.keys(active).length,4);
  const server=await rpc(app.page,'servers.routes.create',{name:'T027 package-only offline fixture',host:'127.0.0.1',port:1,username:'fixture',accessPath:{steps:[]}});
  await openViews(server);
  for(const name of names){
    const id='sodeck.'+name,version=active[id].replace(/\d+$/,v=>String(Number(v)+1)),candidate=join(output,name+'-candidate.sodeck-plugin');
    await writeFile(candidate,await fixturePackage(basePackages[names.indexOf(name)],entries=>{
      const manifest=JSON.parse(entries.get('manifest.json'));manifest.version=version;entries.set('manifest.json',Buffer.from(JSON.stringify(manifest)));
      const worker=manifest.entry?.worker??'worker/main.js';assert.ok(entries.has(worker));
      entries.set(worker,Buffer.concat([entries.get(worker),Buffer.from(`\n{const prior=globalThis.sodeckPlugin.invoke;globalThis.sodeckPlugin.invoke=function(command,...args){return command==='qa.mixedRouteVersion'?{version:${JSON.stringify(version)}}:prior.call(this,command,...args);};}\n`)]));
      const ui=[...entries.keys()].filter(n=>n.startsWith('ui/assets/')&&n.endsWith('.js'));assert.ok(ui.length);
      for(const path of ui)entries.set(path,Buffer.concat([entries.get(path),Buffer.from(`\ndocument.documentElement.dataset.mixedRouteUpdate=${JSON.stringify(version)};\n`)]));
    }));
    await rpc(app.page,'plugins.prepareUpdate',{path:candidate});assert.deepEqual(await versions(),active);
    const pending=(await rpc(app.page,'plugins.list')).find(p=>p.pluginId===id);assert.equal(pending.pendingVersion,version);
    const oldPage=await pluginPage(name);assert.equal(await oldPage.evaluate(()=>document.documentElement.dataset.mixedRouteUpdate),undefined);
    await close();await start();active={...active,[id]:version};assert.deepEqual(await versions(),active);await openViews(server);
    const page=await pluginPage(name);await page.waitForFunction(v=>document.documentElement.dataset.mixedRouteUpdate===v,version);
    assert.deepEqual(await pluginRpc(page,'plugin.invoke',{command:'qa.mixedRouteVersion',args:{}}),{version});
    await rpc(app.page,'views.activate',{viewId:await page.evaluate(()=>window.__SODECK_CONTEXT__.viewId)});
    await page.screenshot({path:join(output,name+'-updated.png')});assert.deepEqual(await Promise.all(files.map(hash)),before);
    note(`${name}: single-package update pending before restart, its UI and worker active after restart; core and other active versions unchanged`);
  }
  await close();
  }
  if(!process.argv.includes('--updates-only')){
  // Exact API5 archives fail on new core during startup.
  evidence.api5On6=[];
  for(const name of names){
    process.env.SODECK_PLUGIN_PACKAGES=name==='monitor'?resolve('.qa/t024/packages/monitor.sodeck-plugin'):join(previous,'resources',name+'.sodeck-plugin');
    app=await launch({data:join(output,'api5-rejected-'+name),executable:join(current,'sodeck-desktop.exe'),shellPort:12871+names.indexOf(name),pluginPort:12875+names.indexOf(name),allowStartupWarning:true});
    const info=await rpc(app.page,'app.info');assert.match(info.startupError,/API|api|版本/);
    assert.equal((await rpc(app.page,'plugins.list')).length,0);evidence.api5On6.push({name,error:info.startupError});await close();
  }
  note('All four API5 plugin archives rejected by API6 core at startup');

  // Use the old executable and old schema in a dedicated clone, never the source data.
  delete process.env.SODECK_PLUGIN_PACKAGES;delete process.env.SODECK_PLUGIN_DEBUG_PORTS;
  const old=await clone(previous,'old');app=await launch({ordinary:true,data:join(old,'data'),executable:join(old,'sodeck-desktop.exe'),shellPort:12880});
  evidence.api6On5=[];for(const path of basePackages){await assert.rejects(rpc(app.page,'plugins.prepareUpdate',{path}),e=>{evidence.api6On5.push({code:e.code,message:e.message});return /API|api|版本/.test(e.message);});}
  const original=await rpc(app.page,'servers.routes.create',{name:'T027 complete backup fixture',host:'fixture.invalid',username:'fixture',authType:'password',accessPath:{kind:'direct'}});
  const prepared=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:original.id},expectedOwnerRevision:original.revision,expectedDependencies:[]});
  await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:prepared.contextToken,password:'T027-isolated-backup-fixture',remember:true});
  assert.equal((await rpc(app.page,'servers.get',{serverId:original.id})).hasSecret,true);await close();
  note('Old API5 core rejects all four API6 archives; old v6 server and remembered credential created in isolated clone');
  const pluginFile='plugins/t027-preserved.bin',marker=Buffer.from('independent plugin storage remains intact');
  await mkdir(join(old,'data/plugins'),{recursive:true});await writeFile(join(old,'data',pluginFile),marker);
  const oldDb=join(old,'data/core/core.sqlite'),oldHash=await hash(oldDb);
  const fresh=await clone(current,'rebuild');await cp(join(old,'data'),join(fresh,'data'),{recursive:true});
  const backupRoot=join(fresh,'data/backups/core-schema-reset'),known=new Set(await readdir(backupRoot));
  const executable=join(fresh,'sodeck-desktop.exe');
  const started=launch({ordinary:true,data:join(fresh,'data'),executable,shellPort:12881,startupTimeout:60000,allowStartupWarning:true});started.catch(()=>{});
  try{await acknowledgeReset(executable);}catch(error){app=await started.catch(()=>undefined);throw error;}
  app=await started;evidence.rebuildStartup=await rpc(app.page,'app.info');
  if(evidence.rebuildStartup.startupError)assert.match(evidence.rebuildStartup.startupError,/incompatible core or API/);
  assert.equal((await rpc(app.page,'servers.list')).servers.length,0);assert.equal((await rpc(app.page,'proxies.socks5.list')).proxies.length,0);await close();
  assert.deepEqual(await readFile(join(fresh,'data',pluginFile)),marker);assert.equal(await hash(oldDb),oldHash);
  const added=(await readdir(backupRoot)).filter(n=>!known.has(n));assert.equal(added.length,1);const backup=join(backupRoot,added[0]);
  const journal=JSON.parse(await readFile(join(backup,'backup.json'),'utf8'));const member=journal.members.find(m=>m.path==='core/core.sqlite');
  assert.equal(member.previousVersion,6);assert.equal(member.targetVersion,7);assert.ok(member.backupFiles.some(f=>f.name.startsWith('secrets/')));
  for(const m of journal.members)for(const file of m.backupFiles)assert.equal(await hash(join(backup,dirname(m.path),file.name)),file.sha256);
  note('Visible native reset notice; complete v6 core and credential backup verified; empty v7 created and independent plugin file retained');
  const rollback=await clone(previous,'rollback');await cp(join(old,'data'),join(rollback,'data'),{recursive:true});
  for(const m of journal.members)for(const file of m.backupFiles){const target=join(rollback,'data',dirname(m.path),file.name);await mkdir(dirname(target),{recursive:true});await cp(join(backup,dirname(m.path),file.name),target);}
  app=await launch({ordinary:true,data:join(rollback,'data'),executable:join(rollback,'sodeck-desktop.exe'),shellPort:12882});
  const restored=await rpc(app.page,'servers.get',{serverId:original.id});assert.equal(restored.name,original.name);assert.equal(restored.hasSecret,true);await close();
  note('Matching old executable reads exact server ID and remembered credential after restoring complete backup in a separate directory');
  evidence.backup=backup;
  }
  evidence.activeVersions=active;assert.deepEqual(await Promise.all(files.map(hash)),before);
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await close();const result={passed:!failure,output,checks,evidence,unchangedFiles:files.map((path,i)=>({path,sha256:before[i]})),failure};await writeFile(join(output,'results.json'),JSON.stringify(result,null,2));await writeFile(process.argv.includes('--updates-only')?'.qa/t027/refinement-package-latest.json':'.qa/t027/package-latest.json',JSON.stringify(result,null,2));}
