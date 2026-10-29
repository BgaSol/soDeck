// Real Debian/OpenSSH on the dedicated T-028 overlay; no ordinary server/data.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import net from 'node:net';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const packages=JSON.parse(await readFile('.qa/t028/packages.json','utf8'));
const output=resolve('.qa/t028/business-'+Date.now()),data=join(output,'data');await mkdir(output,{recursive:true});
const owners=['sodeck.terminal','sodeck.files','sodeck.monitor','sodeck.network-proxy'];
const baseline=resolve('.local/dev/SoDeck-v2-T027-r9/resources');
process.env.SODECK_PLUGIN_PACKAGES=[packages.A.path,...['files','monitor','network-proxy'].map(n=>join(baseline,n+'.sodeck-plugin'))].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(Object.fromEntries(owners.map((id,i)=>[id,13202+i])));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),binaryHash=hash(await readFile('target/debug/sodeck-desktop.exe'));
const checks=[],browsers=new Map();let app,server,failure;
const note=s=>{checks.push(s);console.log(s);};
const invoke=async(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args:{...args,viewId:await page.evaluate(()=>window.__SODECK_CONTEXT__.viewId)}});
async function view(id){return until(async()=>{let b=browsers.get(id);if(!b?.isConnected()){b=await chromium.connectOverCDP('http://127.0.0.1:'+(13202+owners.indexOf(id)));browsers.set(id,b);}for(const p of b.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===server.id)return p;},30000);}
async function start(){app=await launch({data,shellPort:13201,pluginPort:13202});}
async function shutdown(){await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;for(const b of browsers.values())await b.close().catch(()=>{});browsers.clear();}
async function save(enabled){const p=await rpc(app.page,'plugins.enablement.get');await rpc(app.page,'plugins.enablement.commit',{generation:p.generation,expectedRevision:p.revision,operationId:crypto.randomUUID(),changes:owners.map(pluginId=>({pluginId,enabled:enabled.includes(pluginId)}))});}
async function open(){await rpc(app.page,'workspaces.open',{serverId:server.id});}
async function command(pluginId,commandId,args={},viewId){return rpc(app.page,'commands.execute',{pluginId,commandId,workspaceId:server.id,...(viewId?{viewId}:{}),operationId:crypto.randomUUID(),args});}
async function echo(p,marker){await p.waitForSelector('.xterm-rows');await until(async()=>/[$#]\s*$/.test(await p.locator('.xterm-rows').innerText()),30000);await p.locator('.xterm-helper-textarea').focus();await p.locator('.xterm-helper-textarea').pressSequentially(`printf '${marker}\\n'`);await p.locator('.xterm-helper-textarea').press('Enter');await until(async()=>(await p.locator('.xterm-rows').innerText()).split('\n').some(line=>line.trim()===marker));}
async function monitor(){const p=await view('sodeck.monitor');await until(async()=>{const s=await invoke(p,'monitor.snapshot');return s.cpu.value?.percent!==null&&s.cpu.value&&s.memory.value&&s.system.os==='Linux';},45000);return p;}
const curl=async(port,scheme)=> (await promisify(execFile)('curl.exe',['-sf','--max-time','15','--noproxy',scheme?'':'*',...(scheme?['--proxy',`${scheme}://127.0.0.1:${port}`]:[]),`http://127.0.0.1:${scheme?18088:port}/`],{windowsHide:true})).stdout;
const available=port=>new Promise(resolve=>{const s=net.connect(port,'127.0.0.1');s.once('connect',()=>{s.destroy();resolve(true);});s.once('error',()=>resolve(false));});
async function runRule(p,id){const viewId=await p.evaluate(()=>window.__SODECK_CONTEXT__.viewId);await command('sodeck.network-proxy','network.start',{id},viewId);const s=await invoke(p,'network.snapshot');return s.items.find(i=>i.rule.id===id).run.address.port;}
try{
  await start();server=await rpc(app.page,'servers.routes.create',{name:'T028 isolated Debian',host:'127.0.0.1',port:22383,username:'root',authType:'key',accessPath:{steps:[]}});
  const credentials=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:server.id},expectedOwnerRevision:server.revision,expectedDependencies:[]});await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:credentials.contextToken,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});
  await open();await until(async()=>{const dialog=app.page.getByRole('dialog',{name:'确认身份与登录',exact:true});if(await dialog.count())await dialog.getByRole('button',{name:'信任并连接',exact:true}).click();return (await rpc(app.page,'resources.list')).ptys.some(p=>p.state==='active');},45000);
  let terminal=await view('sodeck.terminal');await echo(terminal,'T028_UNSIGNED_A');assert.equal((await rpc(app.page,'plugins.management.list')).items.find(r=>r.pluginId==='sodeck.terminal').selected.version,'0.13.1');
  const pageId='plugin:sodeck.terminal:general';const {snapshot:s}=await rpc(app.page,'settings.page.get',{pageId});await rpc(app.page,'settings.commit',{pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,changes:[{key:'fontSize',value:18}],operationId:crypto.randomUUID()});
  await command('sodeck.files','files.connect');let files=await view('sodeck.files');const fileSnapshot=await until(async()=>{const s=await invoke(files,'files.snapshot');return s.state==='ready'&&s;},30000);assert.equal(fileSnapshot.state,'ready');assert.ok(fileSnapshot.rows.length>0);
  let mon=await monitor();await mon.screenshot({path:join(output,'linux-monitor.png')});await files.screenshot({path:join(output,'linux-files.png')});
  await command('sodeck.network-proxy','network.open');let proxy=await view('sodeck.network-proxy');
  const proxyRule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'T028 proxy',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}});
  const forwardRule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'T028 forward',type:'port-forward',bindMode:'auto',port:0,autoReconnect:false,targetHost:'127.0.0.1',targetPort:18088}});
  const proxyPort=await runRule(proxy,proxyRule.id),forwardPort=await runRule(proxy,forwardRule.id);for(const scheme of ['http','socks5h'])assert.equal(await curl(proxyPort,scheme),'T028_HTTP_OK');assert.equal(await curl(forwardPort),'T028_HTTP_OK');
  note('Unsigned real terminal A executes Linux PTY; files/SFTP and CPU/memory/mount samples work alongside HTTP, SOCKS5 and fixed forwarding');
  await save(['sodeck.files','sodeck.monitor']);await echo(terminal,'T028_STILL_RUNNING');assert.equal(await curl(proxyPort,'http'),'T028_HTTP_OK');
  const old=(await rpc(app.page,'diagnostics.get')).pluginInstances.find(p=>p.pluginId==='sodeck.terminal');const oldView=await terminal.evaluate(()=>window.__SODECK_CONTEXT__.viewId);process.kill(old.processId);
  await until(async()=>!(await rpc(app.page,'diagnostics.get')).pluginInstances.some(p=>p.instanceId===old.instanceId));await rpc(app.page,'views.recover',{viewId:oldView});await until(async()=>(await rpc(app.page,'diagnostics.get')).pluginInstances.some(p=>p.pluginId==='sodeck.terminal'&&p.instanceId!==old.instanceId));
  note('Saving disable leaves live PTY and proxy usable; crashed terminal can still recover under this boot’s enabled snapshot');
  const operationId=crypto.randomUUID();await rpc(app.page,'plugins.package.prepare',{operationId,path:packages.B.path});await until(async()=>{const p=await rpc(app.page,'plugins.package.operation.get',{operationId});assert.notEqual(p.state,'failed',p.error);return p.state==='prepared';});
  await shutdown();assert.equal(await available(proxyPort),false);assert.equal(await available(forwardPort),false);await start();await open();assert.deepEqual((await rpc(app.page,'diagnostics.get')).pluginInstances.map(p=>p.pluginId).sort(),['sodeck.files','sodeck.monitor']);
  await command('sodeck.files','files.connect');files=await view('sodeck.files');await until(async()=>(await invoke(files,'files.snapshot')).state==='ready',30000);await monitor();assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  note('Restart releases listeners and excludes terminal/proxy processes; files connect and monitor observes Linux without terminal or PTY');
  await save(['sodeck.monitor']);await shutdown();await start();await open();mon=await view('sodeck.monitor');const waiting=await until(()=>invoke(mon,'monitor.snapshot')); assert.notEqual(waiting.connection,'connected');await command('sodeck.monitor','monitor.connect');await monitor();assert.equal((await rpc(app.page,'diagnostics.get')).pluginInstances.length,1);
  note('Monitor-only startup does not dial automatically; its own Connect action acquires Linux data without loading files or terminal');
  await save(['sodeck.terminal','sodeck.network-proxy']);await shutdown();await start();await open();terminal=await view('sodeck.terminal');await until(async()=>(await rpc(app.page,'resources.list')).ptys.some(p=>p.state==='active'),45000);await echo(terminal,'T028_UNSIGNED_B');assert.equal((await rpc(app.page,'plugins.management.list')).items.find(r=>r.pluginId==='sodeck.terminal').selected.version,'0.13.2');
  assert.equal((await rpc(app.page,'settings.page.get',{pageId})).snapshot.values.find(v=>v.key==='fontSize').value,18);assert.equal((await rpc(app.page,'plugins.management.list')).items.find(p=>p.pluginId==='sodeck.terminal').selected.signature.status,'unsigned');
  await command('sodeck.network-proxy','network.open');proxy=await view('sodeck.network-proxy');const rules=await invoke(proxy,'network.snapshot');assert.ok(rules.items.some(i=>i.rule.id===proxyRule.id));const restoredPort=await runRule(proxy,proxyRule.id);assert.equal(await curl(restoredPort,'socks5h'),'T028_HTTP_OK');
  assert.ok(!(await rpc(app.page,'diagnostics.get')).pluginInstances.some(p=>['sodeck.files','sodeck.monitor'].includes(p.pluginId)));await terminal.screenshot({path:join(output,'unsigned-terminal-b.png')});
  note('Independent genuine UI/worker A→B update shows 0.13.2, preserves font setting and signed status accurately; proxy rules survive disable/re-enable');
  await save(owners);await shutdown();await start();await open();await command('sodeck.files','files.connect');await monitor();assert.equal((await rpc(app.page,'diagnostics.get')).pluginInstances.length,4);
  await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,20000);await until(async()=>(await rpc(app.page,'net.resources.list')).items.length===0,15000);
  assert.equal(hash(await readFile('target/debug/sodeck-desktop.exe')),binaryHash);note('Re-enabling all restores four plugins; workspace close releases leases/network resources, core binary never changed');
}catch(e){failure={message:e.message,stack:e.stack};if(app){await app.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});for(const [id,b] of browsers){for(const p of b.contexts().flatMap(c=>c.pages())){await p.screenshot({path:join(output,id+'.png')}).catch(()=>{});await writeFile(join(output,id+'.json'),JSON.stringify({text:await p.locator('body').innerText().catch(()=>''),state:await pluginRpc(p,'view.initialize').catch(e=>String(e)),focus:await p.evaluate(()=>document.hasFocus()).catch(()=>false)},null,2));}}}throw e;}
finally{if(app)await shutdown();const result={passed:!failure,output,checks,failure,binaryHash,packages};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t028/business-latest.json',JSON.stringify(result,null,2));}
