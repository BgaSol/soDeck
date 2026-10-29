import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,mkdtemp,copyFile,readFile,writeFile,access} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {chromium} from 'playwright';
import {rpc,until} from './harness.mjs';

const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T019'),output=resolve('.qa/t019/package-ordinary');
await mkdir(output,{recursive:true});
const run=await mkdtemp(join(output,'run-')),manifest=JSON.parse(await readFile(join(source,'build.json'),'utf8')),results=[];
const exists=path=>access(path).then(()=>true,error=>{if(error.code==='ENOENT')return false;throw error;});
const psQuote=value=>"'"+value.replaceAll("'","''")+"'";
async function processes(executable){
  const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`@(Get-Process -Name sodeck-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq ${psQuote(executable)} } | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress`],{windowsHide:true});
  const ids=stdout.trim()?JSON.parse(stdout):[];return Array.isArray(ids)?ids:[ids];
}
for(const mode of ['direct','launcher']){
  const destination=join(run,`${mode} portable package`),executable=join(destination,'sodeck-desktop.exe'),data=join(destination,'data'),port=mode==='direct'?9491:9492;
  for(const file of [...Object.keys(manifest.hashes),'build.json']){await mkdir(dirname(join(destination,file)),{recursive:true});await copyFile(join(source,file),join(destination,file));}
  const env={...process.env};for(const key of Object.keys(env))if(key.startsWith('SODECK_')||key.startsWith('WEBVIEW2_'))delete env[key];
  // Ordinary startup must ignore stale developer paths. The launcher must also
  // neutralize inherited SODECK_DEV_MODE. Only the browser inspection port is external.
  Object.assign(env,{SODECK_DATA_DIR:join(run,'must-not-exist'),SODECK_PLUGIN_PACKAGE:'missing-stale-package',SODECK_PLUGIN_KEYS:'missing-stale-keys',SODECK_SHELL_DEBUG_PORT:'9989',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`});
  if(mode==='launcher')env.SODECK_DEV_MODE='1';
  const child=mode==='direct'?spawn(executable,[],{cwd:run,env,windowsHide:true,stdio:'ignore'}):spawn(process.env.ComSpec??'cmd.exe',['/d','/s','/c',`""${join(destination,'Start-SoDeck.cmd')}""`],{cwd:run,env,windowsHide:true,windowsVerbatimArguments:true,stdio:'ignore'});
  let browser,page,ids=[];
  try{
    await until(async()=>fetch(`http://127.0.0.1:${port}/json/version`).then(r=>r.ok),30000);
    browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
    await page.waitForSelector('#root > *');ids=await processes(executable);assert.ok(ids.length>=1);
    const info=await rpc(page,'app.info');assert.equal(info.version,manifest.version);assert.equal(info.startupError,null);
    assert.equal(await page.getByRole('alert').count(),0);
    const plugins=await rpc(page,'plugins.list');
    for(const expected of manifest.pluginPackages)assert.equal(plugins.find(p=>p.pluginId===expected.pluginId)?.activeVersion,expected.version);
    const log=await readFile(join(data,'logs/startup.log'),'utf8');assert.match(log,/main window ready/);assert.doesNotMatch(log,/startup (warning|failed)/);
    assert.equal(await exists(join(run,'must-not-exist')),false);
    assert.equal(await exists(join(data,'core')),true);assert.equal(await exists(join(data,'plugins')),true);
    assert.equal(await fetch('http://127.0.0.1:9989/json/version').then(()=>true,()=>false),false);
    await page.screenshot({path:join(output,`${mode}-startup.png`)});
    await rpc(page,'app.exit').catch(error=>{if(!/closed|Target|destroyed/.test(String(error)))throw error;});
    await until(async()=>(await processes(executable)).length===0,10000);
    results.push({mode,passed:true,ordinaryApplicationMode:true,browserInspectionPort:port,portableData:true,plugins:plugins.map(p=>({pluginId:p.pluginId,version:p.activeVersion})),normalExit:true});
    console.log(`${mode}: ordinary startup, bundled plugins, portable data and normal exit passed`);
  }finally{
    await browser?.close().catch(()=>{});
    // Only this isolated test executable is eligible for failure cleanup.
    for(const id of await processes(executable))try{process.kill(id);}catch{}
    if(mode==='launcher'&&child.exitCode===null)child.kill();
  }
}
await writeFile(join(output,'results.json'),JSON.stringify({passed:true,version:manifest.version,packageRevision:manifest.packageRevision,run,results},null,2));
