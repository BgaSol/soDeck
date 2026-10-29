// Ordinary startup of the isolated T-027 refinement package. Read-only app RPCs;
// no workspaces, connections, or remote commands are started from user profiles.
import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
import {rpc,until} from './harness.mjs';

const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T027-r8');
const evidencePrefix=process.env.SODECK_DELIVERY_EVIDENCE_PREFIX??'refinement-delivery';
assert.match(evidencePrefix,/^[a-z0-9-]+$/);
const output=resolve('.qa/t027/'+evidencePrefix+'-'+Date.now());await mkdir(output,{recursive:true});
const build=JSON.parse(await readFile(join(source,'build.json'),'utf8'));
assert.equal(build.pluginApiVersion,'6.1.0');assert.equal(build.coreSchema,7);
const executable=join(source,'sodeck-desktop.exe'),checks=[],evidence=[];
const quote=v=>"'"+v.replaceAll("'","''")+"'";
async function processes(){
  const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-Command',`@(Get-Process -Name sodeck-desktop -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq ${quote(executable)}} | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress`],{windowsHide:true});
  const ids=stdout.trim()?JSON.parse(stdout):[];return Array.isArray(ids)?ids:[ids];
}
assert.deepEqual(await processes(),[],'Delivery smoke requires its own package to be stopped');
let priorServers,priorProxies;
for(const mode of ['direct','launcher']){
  for(const [path,expected] of Object.entries(build.hashes))assert.equal(createHash('sha256').update(await readFile(join(source,path))).digest('hex'),expected,path);
  const port=mode==='direct'?12981:12982,env={...process.env};
  for(const key of Object.keys(env))if(key.startsWith('SODECK_')||key.startsWith('WEBVIEW2_'))delete env[key];
  env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=`--remote-debugging-port=${port}`;
  if(mode==='launcher')env.SODECK_DEV_MODE='1'; // Launcher must neutralize this inherited flag.
  const child=mode==='direct'?spawn(executable,[],{cwd:output,env,windowsHide:true,stdio:'ignore'}):spawn(process.env.ComSpec??'cmd.exe',['/d','/s','/c',`""${join(source,'Start-SoDeck.cmd')}""`],{cwd:output,env,windowsHide:true,windowsVerbatimArguments:true,stdio:'ignore'});
  let browser;
  try{
    await until(()=>fetch(`http://127.0.0.1:${port}/json/version`).then(r=>r.ok),30000);
    browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
    await page.waitForSelector('#root > *');
    const info=await rpc(page,'app.info');assert.equal(info.version,build.version);assert.equal(info.startupError,null);
    const capabilities=(await rpc(page,'capabilities.list')).capabilities;
    for(const capability of ['connections.diagnostics.v1','auth.ssh.confirm.v1','ui.connectionDiagnostics.v1'])assert.ok(capabilities.includes(capability),capability);
    const plugins=await rpc(page,'plugins.list');
    for(const expected of build.plugins){const p=plugins.find(p=>p.pluginId===expected.pluginId);assert.equal(p?.activeVersion,expected.version);assert.equal(p?.error,null);}
    if(process.env.SODECK_EXPECT_TERMINAL_INTEGRATION!==undefined){
      const {snapshot}=await rpc(page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'});
      assert.equal(snapshot.values.find(v=>v.key==='shellIntegration').value,process.env.SODECK_EXPECT_TERMINAL_INTEGRATION==='true');
    }
    const servers=(await rpc(page,'servers.routes.list')).servers,proxies=(await rpc(page,'proxies.socks5.list')).proxies;
    if(priorServers){assert.deepEqual(servers,priorServers);assert.deepEqual(proxies,priorProxies);}else{priorServers=servers;priorProxies=proxies;}
    const resources=await rpc(page,'resources.list');assert.equal(resources.leases.length,0);
    assert.equal(await page.getByRole('dialog',{name:'确认身份与登录',exact:true}).count(),0);
    await page.screenshot({path:join(output,`${mode}-startup.png`)});
    await rpc(page,'app.exit').catch(error=>{if(error.code!=='CLOSE_REVIEW_REQUIRED'&&!/closed|Target|destroyed/.test(String(error)))throw error;});
    await until(async()=>(await processes()).length===0,15000);
    checks.push(`${mode}: ordinary startup, API6.1 capabilities, four bundled plugins, retained profiles, no automatic connection, normal exit`);
    evidence.push({mode,startupError:info.startupError,servers:servers.length,proxies:proxies.length,plugins:plugins.map(p=>({pluginId:p.pluginId,version:p.activeVersion})),normalExit:true});
    console.log(checks.at(-1));
  }finally{
    await browser?.close().catch(()=>{});
    // Only this explicitly isolated delivery executable is eligible for cleanup.
    for(const pid of await processes())try{process.kill(pid);}catch{}
    if(mode==='launcher'&&child.exitCode===null)child.kill();
  }
}
const result={passed:true,source,output,checks,evidence};
await writeFile(join(output,'results.json'),JSON.stringify(result,null,2));
await writeFile('.qa/t027/'+evidencePrefix+'-latest.json',JSON.stringify(result,null,2));
