// T-028 ordinary delivery plus an isolated old-core v7/DPAPI/plugin-data copy.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,cp,readdir} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {createHash} from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {launch,rpc,until,stop} from './harness.mjs';
const directory=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T028-r1');
const output=resolve('.qa/t028/delivery-'+Date.now());await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON;assert.ok(python,'Set SODECK_TEST_PYTHON to an available Python 3 runtime');
const build=JSON.parse(await readFile(join(directory,'build.json'),'utf8')),checks=[],evidence={};let app,failure;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const note=s=>{checks.push(s);console.log(s);};
async function verify(folder){const b=JSON.parse(await readFile(join(folder,'build.json'),'utf8'));for(const [name,digest] of Object.entries(b.hashes))assert.equal(hash(await readFile(join(folder,name))),digest,name);return b;}
async function clone(folder,name){const b=await verify(folder),target=join(output,name);for(const path of [...Object.keys(b.hashes),'build.json']){await mkdir(dirname(join(target,path)),{recursive:true});await cp(join(folder,path),join(target,path));}return target;}
async function start(folder,port){app=await launch({ordinary:true,bundledResources:true,executable:join(folder,'sodeck-desktop.exe'),data:join(folder,'data'),shellPort:port,cwd:output});}
async function close(){if(app){await stop(app);assert.equal(app.shutdown.forced,false);assert.deepEqual(app.errors,[]);app=undefined;}}
async function snapshot(source,target){return JSON.parse((await promisify(execFile)(python,['tests/desktop/core-snapshot.py',source,target],{windowsHide:true,env:{...process.env,PYTHONIOENCODING:'utf-8'}})).stdout);}
async function treeHashes(folder){const values={};async function walk(base,relative=''){for(const e of await readdir(base,{withFileTypes:true})){const path=join(relative,e.name);if(e.isDirectory())await walk(join(base,e.name),path);else values[path]=hash(await readFile(join(base,e.name)));}}await walk(folder);return values;}
// SQLite read-only backup may update SHM reader slots; DB/WAL/secret bytes must stay unchanged.
async function coreHashes(folder){const values=await treeHashes(folder);delete values['core.sqlite-shm'];return values;}
try{
  await verify(directory);assert.equal(build.version,'2.0.0-dev.20');assert.equal(build.pluginApiVersion,'6.2.0');assert.equal(build.coreSchema,7);
  await start(directory,13301);
  const runtime=await rpc(app.page,'app.runtimeInfo.get');assert.equal(runtime.mode,'portable');assert.equal(runtime.version,build.version);assert.equal(resolve(runtime.dataPath),join(directory,'data'));
  const rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.length,4);
  for(const expected of build.plugins){const actual=rows.find(p=>p.pluginId===expected.pluginId);assert.equal(actual.selected.version,expected.version);assert.equal(actual.selected.signature.status,'developmentVerified');assert.equal(actual.runtimeStatus,'running');}
  const caps=(await rpc(app.page,'capabilities.list')).capabilities;for(const c of ['plugins.management.v1','plugins.optional-signature.v1'])assert.ok(caps.includes(c),c);
  assert.equal((await rpc(app.page,'servers.list')).servers.length,0);assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(app.page,'resources.list')).leases.length,0);
  await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:plugins'});
  const settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});
  await settings.getByRole('heading',{name:'插件管理',exact:true}).waitFor();await settings.getByLabel('启用 终端',{exact:true}).waitFor();assert.equal(await settings.getByRole('checkbox').count(),4);
  await settings.screenshot({path:join(output,'plugin-management.png')});await settings.getByRole('button',{name:'取消',exact:true}).click();await until(()=>settings.isClosed());await close();
  evidence.runtime=runtime;evidence.plugins=rows.map(r=>({pluginId:r.pluginId,version:r.selected.version,signature:r.selected.signature.status}));note('Final executable starts normally from unrelated cwd: portable path, API6.2 capabilities, four original signed archives, native management UI, empty data and normal shutdown');

  // The actual user-facing .cmd must clear an inherited development-mode flag.
  const executable=join(directory,'sodeck-desktop.exe'),quote=v=>"'"+v.replaceAll("'","''")+"'";
  const processes=async()=>{const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`@(Get-Process -Name sodeck-desktop -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq ${quote(executable)}} | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress`],{windowsHide:true});const value=stdout.trim()?JSON.parse(stdout):[];return Array.isArray(value)?value:[value];};
  assert.deepEqual(await processes(),[]);const env={...process.env};for(const key of Object.keys(env))if(key.startsWith('SODECK_')||key.startsWith('WEBVIEW2_'))delete env[key];env.SODECK_DEV_MODE='1';env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=13302';
  const child=spawn(process.env.ComSpec??'cmd.exe',['/d','/s','/c',`""${join(directory,'Start-SoDeck.cmd')}""`],{cwd:output,env,windowsHide:true,windowsVerbatimArguments:true,stdio:'ignore'});let browser;
  try{await until(()=>fetch('http://127.0.0.1:13302/json/version').then(r=>r.ok),30000);browser=await chromium.connectOverCDP('http://127.0.0.1:13302');const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));await page.waitForSelector('#root > *');assert.equal((await rpc(page,'app.runtimeInfo.get')).mode,'portable');assert.equal((await rpc(page,'app.info')).startupError,null);await rpc(page,'app.exit').catch(e=>{if(e.code!=='CLOSE_REVIEW_REQUIRED'&&!/closed|Target|destroyed/.test(String(e)))throw e;});await until(async()=>(await processes()).length===0);note('Start-SoDeck.cmd launches the delivered executable with portable mode and exits normally');}
  finally{await browser?.close().catch(()=>{});for(const pid of await processes())try{process.kill(pid);}catch{}if(child.exitCode===null)child.kill();}

  // Create test-only profiles with the actual previous API6.1 executable.
  const old=await clone(resolve('.local/dev/SoDeck-v2-T027-r9'),'old-api61');await start(old,13303);
  const server=await rpc(app.page,'servers.routes.create',{name:'T028 isolated compatibility fixture',host:'127.0.0.1',port:1,username:'fixture',authType:'password',accessPath:{steps:[]}});
  const prepared=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:server.id},expectedOwnerRevision:server.revision,expectedDependencies:[]});
  await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:prepared.contextToken,password:'T028-only-offline-fixture',remember:true});assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).hasSecret,true);
  const pageId='plugin:sodeck.terminal:general',{snapshot:s}=await rpc(app.page,'settings.page.get',{pageId});await rpc(app.page,'settings.commit',{pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,changes:[{key:'fontSize',value:18}],operationId:crypto.randomUUID()});await close();
  const next=await clone(directory,'compatibility'),source=join(old,'data/core'),target=join(next,'data/core');
  const sourceBefore=await coreHashes(source),before=await snapshot(source,target);assert.ok(before.encryptedFiles>0);await cp(join(old,'data/plugins'),join(next,'data/plugins'),{recursive:true});const pluginBefore=await treeHashes(join(next,'data/plugins/data')),secretBefore=await treeHashes(join(target,'secrets'));
  await start(next,13304);assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).hasSecret,true);const afterSettings=await rpc(app.page,'settings.page.get',{pageId});assert.equal(afterSettings.snapshot.values.find(v=>v.key==='fontSize').value,18);assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(app.page,'resources.list')).leases.length,0);await close();
  const after=await snapshot(target,join(output,'post-startup-core'));assert.deepEqual(after.rowDigests,before.rowDigests);assert.equal(after.schema,7);assert.deepEqual(await treeHashes(join(target,'secrets')),secretBefore);assert.deepEqual(await treeHashes(join(next,'data/plugins/data')),pluginBefore);assert.deepEqual(await coreHashes(source),sourceBefore);
  evidence.compatibility={before,after,encryptedBlobsUnchanged:true,pluginDataUnchanged:true,sourceUnchanged:true};note('Actual API6.1 core v7 data copied read-only: all table row digests, encrypted credential bytes and plugin settings unchanged under API6.2; original copy untouched, no connection started');
  await verify(directory);
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await close();const result={passed:!failure,output,directory,checks,evidence,failure,desktopSha256:build.hashes['sodeck-desktop.exe']};await writeFile(join(output,'results.json'),JSON.stringify(result,null,2));await writeFile('.qa/t028/delivery-latest.json',JSON.stringify(result,null,2));}
