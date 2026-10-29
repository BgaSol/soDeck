// Verify the ordinary delivery after relocation, keeping its original data empty.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,cp,readdir} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {createHash} from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {launch,rpc,until,stop} from './harness.mjs';
const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T029-r2'),output=resolve('.qa/t029/delivery-'+Date.now()),directory=join(output,'relocated package');await mkdir(output,{recursive:true});
const build=JSON.parse(await readFile(join(source,'build.json'),'utf8')),checks=[];let app,settings,failure;
const hash=b=>createHash('sha256').update(b).digest('hex'),note=s=>{checks.push(s);console.log(s);};
async function listFiles(folder,prefix=''){const all=[];for(const e of await readdir(folder,{withFileTypes:true})){const path=prefix+e.name;if(e.isDirectory())all.push(...await listFiles(join(folder,e.name),path+'/'));else all.push(path);}return all;}
async function start(){app=await launch({ordinary:true,bundledResources:true,executable:join(directory,'sodeck-desktop.exe'),data:join(directory,'data'),shellPort:13601,cwd:output});}
async function close(){if(settings&&!settings.isClosed())await settings.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});settings=undefined;if(app){await stop(app);assert.equal(app.shutdown.forced,false);assert.deepEqual(app.errors,[]);app=undefined;}}
try{
  assert.equal(build.version,'2.0.0-dev.21');assert.equal(build.pluginApiVersion,'6.3.0');assert.equal(build.coreSchema,7);
  assert.deepEqual((await listFiles(source)).sort(),[...Object.keys(build.hashes),'build.json'].sort(),'No undeclared data or credentials');
  for(const path of [...Object.keys(build.hashes),'build.json']){await mkdir(dirname(join(directory,path)),{recursive:true});await cp(join(source,path),join(directory,path));if(build.hashes[path])assert.equal(hash(await readFile(join(directory,path))),build.hashes[path],path);}
  await start();const runtime=await rpc(app.page,'app.runtimeInfo.get');assert.equal(runtime.mode,'portable');assert.equal(runtime.version,build.version);assert.equal(resolve(runtime.dataPath),join(directory,'data'));
  const status=await rpc(app.page,'updates.status.get');assert.equal(status.channel,'development');assert.equal(status.buildId,'development');assert.equal(status.configured,false);
  const rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.length,4);for(const expected of build.plugins){const actual=rows.find(p=>p.pluginId===expected.pluginId);assert.equal(actual.selected.version,expected.version);assert.equal(actual.selected.signature.status,'developmentVerified');assert.equal(actual.runtimeStatus,'running');}
  assert.equal((await rpc(app.page,'servers.list')).servers.length,0);assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(app.page,'resources.list')).leases.length,0);
  await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:updates'});settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});
  await settings.getByRole('heading',{name:'更新',exact:true}).waitFor();await settings.screenshot({path:join(output,'updates.png')});await close();
  await start();assert.equal((await rpc(app.page,'servers.list')).servers.length,0);assert.equal((await rpc(app.page,'plugins.management.list')).items.filter(p=>p.runtimeStatus==='running').length,4);await close();
  note('Relocated ordinary build verifies every file, uses portable empty data and development identity, loads four original signed plugins, opens updates and restarts normally');
  const executable=join(directory,'sodeck-desktop.exe'),quote=v=>"'"+v.replaceAll("'","''")+"'";
  const processes=async()=>{const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`@(Get-Process -Name sodeck-desktop -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq ${quote(executable)}} | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress`],{windowsHide:true});const value=stdout.trim()?JSON.parse(stdout):[];return Array.isArray(value)?value:[value];};
  const env={...process.env};for(const key of Object.keys(env))if(key.startsWith('SODECK_')||key.startsWith('WEBVIEW2_'))delete env[key];env.SODECK_DEV_MODE='1';env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=13602';
  const child=spawn(process.env.ComSpec??'cmd.exe',['/d','/s','/c',`""${join(directory,'Start-SoDeck.cmd')}""`],{cwd:output,env,windowsHide:true,windowsVerbatimArguments:true,stdio:'ignore'});let browser;
  try{await until(()=>fetch('http://127.0.0.1:13602/json/version').then(r=>r.ok),30000);browser=await chromium.connectOverCDP('http://127.0.0.1:13602');const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));await page.waitForSelector('#root > *');assert.equal((await rpc(page,'app.runtimeInfo.get')).mode,'portable');assert.equal((await rpc(page,'updates.status.get')).buildId,'development');await rpc(page,'app.exit').catch(()=>{});await until(async()=>(await processes()).length===0);note('Start-SoDeck.cmd clears inherited development mode, opens the delivered application and exits normally');}
  finally{await browser?.close().catch(()=>{});for(const pid of await processes())try{process.kill(pid);}catch{}if(child.exitCode===null)child.kill();}
  assert.deepEqual((await listFiles(source)).sort(),[...Object.keys(build.hashes),'build.json'].sort());
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await close();const result={passed:!failure,source,output,directory,checks,failure,desktopSha256:build.hashes['sodeck-desktop.exe']};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t029/delivery-latest.json',JSON.stringify(result,null,2));}
