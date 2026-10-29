// Full installed SoDeck, signed local release fixtures, real isolated Linux.
// GitHub discovery/download and public CI remain a separate G5 validation.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,readdir,copyFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {spawnSync,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createPrivateKey,sign,createHash} from 'node:crypto';
import net from 'node:net';
import {chromium} from 'playwright';
import {launchInstalled} from './installed-updates-harness.mjs';
import {rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const out=resolve('.qa/t029/release-local'),config=JSON.parse(await readFile(join(out,'config.json'),'utf8'));
const coreOnly=process.argv.includes('--core-only');
const prior=coreOnly?JSON.parse(await readFile('.qa/t029/installer-latest.json','utf8')):null;
const pluginEvidence=prior?.priorEvidence?JSON.parse(await readFile(join(prior.priorEvidence,'result.json'),'utf8')):prior;
const built={};for(const label of ['A','B'])built[label]=JSON.parse(await readFile(join(out,label,'built.json'),'utf8'));
const planB=JSON.parse(await readFile(join(out,'B/plan.json'),'utf8'));
const install=join(out,'install'),data=join(process.env.APPDATA,config.identifier),evidence=join(out,'validation-'+Date.now());await mkdir(evidence,{recursive:true});
const checks=[],owners=['sodeck.terminal','sodeck.files','sodeck.monitor','sodeck.network-proxy'],browsers=new Map();let app,settings,server,failure,rule,forward;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const note=message=>{checks.push(message);console.log(message);};
const product=(label,id)=>built[label].products.find(p=>p.componentId===id);
function record(label,id){const p=product(label,id),asset={repository:'BgaSol/soDeck',releaseTag:`sodeck-v2/preview/${label==='A'?'000001':'000002'}`,assetId:(label==='A'?100:200)+built[label].products.indexOf(p),name:p.name,size:p.size,archiveSha256:p.archiveSha256};
  if(id==='core'){const {appVersion,buildId,coreCompatibilityVersion,pluginApiVersion,capabilities,dataFormats,bundledPlugins,inputFingerprint}=p;return {appVersion,buildId,coreCompatibilityVersion,pluginApiVersion,capabilities,dataFormats,bundledPlugins,inputFingerprint,platforms:{'windows-x86_64':{asset,signature:p.signature,signedVersion:p.signedVersion}}};}
  return {version:p.manifest.version,requires:p.manifest.requires,dataSchemaVersion:p.manifest.dataSchemaVersion,inputFingerprint:p.inputFingerprint,packageDigest:p.packageDigest,signingKeyId:config.keyId,asset};
}
async function seedCatalog(){
  const catalog={schemaVersion:1,product:'sodeck-v2',repository:'BgaSol/soDeck',channel:'preview',sequence:2,releaseTag:'sodeck-v2/preview/000002',sourceCommit:planB.sourceCommit,core:{current:record('B','core'),history:[record('A','core')]},plugins:Object.fromEntries(owners.map(id=>[id,{current:record('B',id),history:[record('A',id)]}]))};
  const body=Buffer.from(JSON.stringify(catalog,null,2)+'\n'),key=createPrivateKey(await readFile(join(out,'catalog-private.pem'))),signature={schemaVersion:1,keyId:config.keyId,signature:sign(null,Buffer.concat([Buffer.from('sodeck-v2-catalog-v1\0'),body]),key).toString('base64')};
  const dir=join(data,'updates/catalogs/preview/000002');await mkdir(dir,{recursive:true});await writeFile(join(dir,'catalog-v1.json'),body);await writeFile(join(dir,'catalog-v1.sig'),JSON.stringify(signature));
  const file=join(data,'updates/state.json'),state=JSON.parse(await readFile(file,'utf8'));state.catalog={sequence:2,digest:hash(body),channel:'preview'};state.lastSuccess=Date.now();state.lastCheck=Date.now();state.checkError=null;state.retryAfter=0;await writeFile(file,JSON.stringify(state));
  for(const p of built.B.products)await copyFile(join(out,'B/assets',p.name),join(data,'updates/cache',p.archiveSha256));
  await writeFile(join(evidence,'fixture-catalog.json'),body);
}
async function start(){app=await launchInstalled({config,install});assert.equal((await rpc(app.page,'updates.status.get')).distribution,'installed');}
async function shutdown(){if(settings&&!settings.isClosed()){await settings.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});await settings.getByRole('button',{name:'放弃更改',exact:true}).click({timeout:500}).catch(()=>{});}settings=undefined;if(app){await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;}for(const b of browsers.values())await b.close().catch(()=>{});browsers.clear();}
async function view(id){return until(async()=>{let b=browsers.get(id);if(!b?.isConnected()){b=await chromium.connectOverCDP('http://127.0.0.1:'+(13502+owners.indexOf(id)));browsers.set(id,b);}return b.contexts().flatMap(c=>c.pages()).find(p=>!p.isClosed()&&p.url().includes('sodeck-plugin.localhost'));},30000);}
const invoke=async(p,command,args={})=>pluginRpc(p,'plugin.invoke',{command,args:{...args,viewId:await p.evaluate(()=>window.__SODECK_CONTEXT__.viewId)}});
const command=(id,name,args={},viewId)=>rpc(app.page,'commands.execute',{pluginId:id,commandId:name,workspaceId:server.id,operationId:crypto.randomUUID(),args,...(viewId?{viewId}:{})});
async function open(){await rpc(app.page,'workspaces.open',{serverId:server.id});await until(async()=>{const d=app.page.getByRole('dialog',{name:'确认身份与登录',exact:true});if(await d.count())await d.getByRole('button',{name:'信任并连接',exact:true}).click();return (await rpc(app.page,'resources.list')).ptys.some(p=>p.state==='active');},45000);}
async function echo(marker){const p=await view('sodeck.terminal');await p.waitForSelector('.xterm-rows');await until(async()=>/[$#]\s*$/.test(await p.locator('.xterm-rows').innerText()),30000);await p.locator('.xterm-helper-textarea').focus();await p.locator('.xterm-helper-textarea').pressSequentially(`printf '${marker}\\n'`);await p.locator('.xterm-helper-textarea').press('Enter');await until(async()=>(await p.locator('.xterm-rows').innerText()).split('\n').some(l=>l.trim()===marker));}
const curl=async(port,scheme)=>(await promisify(execFile)('curl.exe',['-sf','--max-time','15','--noproxy',scheme?'':'*',...(scheme?['--proxy',`${scheme}://127.0.0.1:${port}`]:[]),`http://127.0.0.1:${scheme?18089:port}/`],{windowsHide:true})).stdout;
const available=port=>new Promise(resolve=>{const s=net.connect(port,'127.0.0.1');s.once('connect',()=>{s.destroy();resolve(true);});s.once('error',()=>resolve(false));});
async function runRule(p,id){await command('sodeck.network-proxy','network.start',{id},await p.evaluate(()=>window.__SODECK_CONTEXT__.viewId));return(await invoke(p,'network.snapshot')).items.find(i=>i.rule.id===id).run.address.port;}
async function business(marker){await echo(marker);await command('sodeck.files','files.connect');const files=await view('sodeck.files');await until(async()=>{const s=await invoke(files,'files.snapshot');return s.state==='ready'&&s.rows.length>0;},30000);const mon=await view('sodeck.monitor');await until(async()=>{const s=await invoke(mon,'monitor.snapshot');return s.cpu.value?.percent!==null&&s.cpu.value&&s.memory.value&&s.system.os==='Linux';},45000);await command('sodeck.network-proxy','network.open');const proxy=await view('sodeck.network-proxy');await until(()=>invoke(proxy,'network.snapshot'),15000);if(!rule){rule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'T029 proxy',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}});forward=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'T029 forward',type:'port-forward',bindMode:'auto',port:0,targetHost:'127.0.0.1',targetPort:18089,autoReconnect:false}});}const ports=[await runRule(proxy,rule.id),await runRule(proxy,forward.id)];for(const scheme of ['http','socks5h'])assert.equal(await curl(ports[0],scheme),'T029_HTTP_OK');assert.equal(await curl(ports[1]),'T029_HTTP_OK');return ports;}
async function operation(method,ids){const plan=await rpc(app.page,'updates.plan.get',{componentIds:ids});assert.deepEqual(plan.blockedReasons,[]);const id=crypto.randomUUID();await rpc(app.page,method,{operationId:id,planRevision:plan.planRevision,componentIds:ids});return until(async()=>{const r=await rpc(app.page,'updates.operation.get',{operationId:id});if(!r.running){assert.ok(!r.error,JSON.stringify(r));return r;}},45000);}
async function updateSettings(){await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:updates'});settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});await settings.getByRole('heading',{name:'更新',exact:true}).waitFor();return settings;}
try {
  // Refresh the isolated installation with the final A build, preserving its QA data.
  const a=join(out,'A/assets',product('A','core').name);assert.equal(spawnSync(a,['/S','/D='+install],{windowsHide:true,timeout:120000}).status,0);
  await start();const aStatus=await rpc(app.page,'updates.status.get');assert.equal(aStatus.buildId,product('A','core').buildId);
  const policy=aStatus.policy;await rpc(app.page,'updates.policy.commit',{generation:policy.generation,expectedRevision:policy.revision,autoCheck:false,operationId:crypto.randomUUID()});
  if(coreOnly){
    assert.ok(owners.every(id=>pluginEvidence.checks.some(c=>c.startsWith(id+':'))),'Prior four-plugin evidence required');
    server=(await rpc(app.page,'servers.list')).servers.find(s=>s.name==='T029 isolated Debian'&&s.host==='127.0.0.1'&&s.port===22384);assert.ok(server,'Existing credential owner required');
    assert.equal((await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot.values.find(v=>v.key==='fontSize').value,18);
    await shutdown();await seedCatalog();note('Core-only retry preserves previous four-plugin evidence and existing DPAPI/settings without rewriting them');
  }else{
  server=await rpc(app.page,'servers.routes.create',{name:'T029 isolated Debian',host:'127.0.0.1',port:22384,username:'root',authType:'key',accessPath:{steps:[]}});
  const c=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:server.id},expectedOwnerRevision:server.revision,expectedDependencies:[]});await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:c.contextToken,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});
  await open();const initialPorts=await business('T029_A_NATIVE');
  const pageId='plugin:sodeck.terminal:general',{snapshot:s}=await rpc(app.page,'settings.page.get',{pageId});await rpc(app.page,'settings.commit',{pageId,providerEpoch:s.providerEpoch,schemaVersion:s.schemaVersion,expectedRevision:s.settingsRevision,changes:[{key:'fontSize',value:18}],operationId:crypto.randomUUID()});
  await shutdown();for(const port of initialPorts)assert.equal(await available(port),false);await seedCatalog();
  note('Installed A uses all four real Linux services; full exit releases listeners and preserves an encrypted credential plus plugin settings');
  const coreHash=hash(await readFile(join(install,'sodeck-desktop.exe')));
  for(const id of owners){await start();await open();const ports=await business('T029_BEFORE_'+id.split('.').at(-1).replaceAll('-','_'));const before=await rpc(app.page,'plugins.management.list');
    assert.equal((await operation('updates.download',[id])).stage,'downloaded');assert.ok((await rpc(app.page,'plugins.management.list')).items.every(p=>!p.pending));await echo('T029_DOWNLOAD_KEEPS_SESSION');
    assert.equal((await operation('updates.install',[id])).stage,'ready');assert.equal((await rpc(app.page,'plugins.management.list')).items.find(p=>p.pluginId===id).selected.digest,before.items.find(p=>p.pluginId===id).selected.digest);await echo('T029_PREPARE_KEEPS_SESSION');
    await shutdown();for(const port of ports)assert.equal(await available(port),false);await start();await open();await business('T029_UPDATED');
    const after=await rpc(app.page,'plugins.management.list');for(const row of after.items){assert.equal(row.selected.digest,row.pluginId===id?product('B',id).packageDigest:before.items.find(p=>p.pluginId===row.pluginId).selected.digest);}
    const ui=await view(id);assert.equal(await ui.locator('meta[name="t029-release"]').getAttribute('content'),product('B',id).manifest.version);assert.equal((await invoke(ui,'t029.release.version')).version,product('B',id).manifest.version);
    await rpc(app.page,'views.activate',{viewId:await ui.evaluate(()=>window.__SODECK_CONTEXT__.viewId)});await delay(200);await ui.screenshot({path:join(evidence,id+'-B.png')});assert.equal(hash(await readFile(join(install,'sodeck-desktop.exe'))),coreHash);await shutdown();note(`${id}: signed A→B preparation keeps current sessions; restart runs new UI/worker and real business; core and other packages stay unchanged`);
  }
  }
  await start();await open();const ports=await business('T029_BEFORE_CORE');assert.equal((await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot.values.find(v=>v.key==='fontSize').value,18);
  assert.equal((await operation('updates.download',['core'])).stage,'downloaded');await updateSettings();
  for(const themeId of ['soft-light','soft-dark','dopamine']){const t=await rpc(app.page,'ui.theme.catalog.get');await rpc(app.page,'ui.theme.choose',{themeId,expectedRevision:t.revision});await delay(300);await settings.screenshot({path:join(evidence,'updates-'+themeId+'.png')});}
  await settings.getByLabel('自动检查并提示更新',{exact:true}).check();const corePlan=await rpc(app.page,'updates.plan.get',{componentIds:['core']}),cancelledId=crypto.randomUUID();await rpc(app.page,'updates.install',{operationId:cancelledId,planRevision:corePlan.planRevision,componentIds:['core']});
  await settings.getByRole('button',{name:'继续编辑',exact:true}).click();await until(async()=>(await rpc(app.page,'updates.operation.get',{operationId:cancelledId})).stage==='cancelled');await echo('T029_CANCEL_KEEPS_SESSION');
  note('Core install review can be cancelled with dirty settings; PTY and all active services remain available');
  await settings.getByLabel('自动检查并提示更新',{exact:true}).uncheck();const installId=crypto.randomUUID();await rpc(app.page,'updates.install',{operationId:installId,planRevision:corePlan.planRevision,componentIds:['core']});
  await until(()=>app.process.exitCode!==null,60000);for(const port of ports)assert.equal(await available(port),false);
  const previous=app;app=undefined;settings=undefined;for(const b of browsers.values())await b.close().catch(()=>{});browsers.clear();await previous.browser.close().catch(()=>{});
  let reopenedBrowser;const reopened=await until(async()=>{reopenedBrowser=await chromium.connectOverCDP('http://127.0.0.1:13501');const p=reopenedBrowser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));if(!p)return;const s=await rpc(p,'updates.status.get');return s.appVersion===product('B','core').appVersion&&{page:p,status:s};},120000);
  assert.equal(reopened.status.buildId,product('B','core').buildId);assert.equal(reopened.status.distribution,'installed');assert.equal((await rpc(reopened.page,'resources.list')).ptys.length,0);
  await reopened.page.waitForSelector('#root > *');
  const journal=JSON.parse(await readFile(join(data,'updates/state.json'),'utf8'));assert.equal(journal.handoff.stage,'installed');assert.equal((await rpc(reopened.page,'updates.operation.get',{operationId:installId})).stage,'installed');
  await rpc(reopened.page,'app.exit').catch(()=>{});await until(async()=>!await available(13501),30000);await reopenedBrowser.close().catch(()=>{});
  await start();await open();await business('T029_AFTER_CORE');assert.equal((await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot.values.find(v=>v.key==='fontSize').value,18);
  await shutdown();note('Native NSIS A→B reopens the exact signed build, reacquires the data lock, preserves DPAPI and plugin settings, and starts without restoring remote sessions');
  for(const enabled of [[],['sodeck.terminal'],owners]){
    await start();const prefs=await rpc(app.page,'plugins.enablement.get');await rpc(app.page,'plugins.enablement.commit',{generation:prefs.generation,expectedRevision:prefs.revision,operationId:crypto.randomUUID(),changes:owners.map(pluginId=>({pluginId,enabled:enabled.includes(pluginId)}))});await shutdown();
    await start();const rows=(await rpc(app.page,'plugins.management.list')).items;assert.deepEqual(rows.filter(p=>p.runtimeStatus==='running').map(p=>p.pluginId).sort(),[...enabled].sort());assert.ok(rows.every(p=>p.bootEnabled===enabled.includes(p.pluginId)));await updateSettings();await settings.screenshot({path:join(evidence,'enabled-'+enabled.length+'.png')});await shutdown();
  }
  note('Installed B supports all-disabled, terminal-only and restored-all after full restart; core settings and update page remain available');
} catch(e){failure={message:e.message,stack:e.stack};if(app)await app.page.screenshot({path:join(evidence,'failure.png')}).catch(()=>{});throw e;}
finally{await shutdown();const result={passed:!failure,kind:'full-native-installed-application',mode:coreOnly?'core-only':'full',priorEvidence:pluginEvidence?.evidence,github:'signed local fixture; no live GitHub release',linuxPort:22384,install,data,evidence,checks,failure};await writeFile(join(evidence,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t029/installer-latest.json',JSON.stringify(result,null,2));}
