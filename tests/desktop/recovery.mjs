import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {copyFile,mkdir,readFile,readdir,rmdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages,delay} from './harness.mjs';

const output=join(root,'.qa/recovery-native',String(Date.now())),data=join(output,'app');
const originalExecutable=join(root,'target/debug/sodeck-desktop.exe'),originalFactory=join(root,'resources/terminal.sodeck-plugin'),originalKeys=join(root,'resources/development-public-key.json');
const artifacts=join(output,'artifacts'),executable=join(artifacts,'sodeck-desktop.exe'),factory=join(artifacts,'terminal.sodeck-plugin'),keys=join(artifacts,'development-public-key.json');
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const schemaOnly=process.argv.includes('--schema-only'),checks=[],hashes={},interruptions=[];
await mkdir(output,{recursive:true});let app,raw,normalStops=0;
const json=async path=>JSON.parse(await readFile(path,'utf8'));
const activation=()=>json(join(data,'plugins/activation.json'));
const selected=state=>state.active['sodeck.terminal'];
const settingsPath=generation=>join(data,'plugins/data/sodeck.terminal',generation,'settings.json');
const hash=async path=>createHash('sha256').update(await readFile(path)).digest('hex');
const invoke=(command,args={})=>rpc(app.page,'plugin.invoke',{command,args});
const status=async()=>(await rpc(app.page,'plugins.list')).find(plugin=>plugin.pluginId==='sodeck.terminal');
const start=()=>launch({data,shellPort:9405,pluginPort:9406,executable,packagePath:factory,keysPath:keys});
async function viewPage(viewId){return until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===viewId)return page;}catch{}}});}
async function normalStop(){const previous=app;await stop(previous);assert.equal(previous.process.exitCode,0);await writeFile(join(output,`normal-stop-${++normalStops}.log`),previous.output());app=undefined;}
async function candidate(version,transform){
  const source=join(output,`source-${version}`);await mkdir(source,{recursive:true});
  const unpack=spawnSync(python,['-c','import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])',factory,source],{encoding:'utf8',windowsHide:true});assert.equal(unpack.status,0,unpack.stderr);
  const manifestPath=join(source,'manifest.json'),manifest=await json(manifestPath);for(const view of manifest.contributes.views)view.stateVersion=2;await writeFile(manifestPath,JSON.stringify(manifest,null,2));await transform?.(source,manifest);
  const path=join(output,`terminal-${version}.sodeck-plugin`);
  const packed=spawnSync(process.execPath,[join(root,'scripts/pack-plugin.mjs'),'--source',source,'--version',version,'--out',path,'--key',join(root,'.local/signing/development-private.pem'),'--key-id',(await json(keys)).keyId],{cwd:root,encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(packed.status,0,packed.stderr);await writeFile(join(output,`pack-${version}.log`),packed.stdout);hashes[version]=await hash(path);return path;
}
function rawStartup(){const child=spawn(executable,[],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,SODECK_DATA_DIR:data,SODECK_SHELL_DEBUG_PORT:'9405',SODECK_WEBVIEW_ARGS:'--remote-debugging-port=9406',SODECK_PLUGIN_PACKAGE:factory,SODECK_PLUGIN_KEYS:keys}});let logs='';child.stdout.on('data',d=>logs+=d);child.stderr.on('data',d=>logs+=d);return{process:child,output:()=>logs};}
async function killActual(runtime,label,facts){
  const exited=new Promise(resolve=>runtime.process.once('exit',(code,signal)=>resolve({code,signal}))),pid=runtime.process.pid;assert.equal(runtime.process.kill(),true);const exit=await exited;assert.ok(exit.code!==0||exit.signal);interruptions.push({label,pid,...exit,facts});await runtime.pluginBrowser?.close().catch(()=>{});await runtime.browser?.close().catch(()=>{});await writeFile(join(output,`${label}.log`),runtime.output());await writeFile(join(output,'interruptions.json'),JSON.stringify(interruptions,null,2));await delay(200);
}
try{
  await mkdir(artifacts,{recursive:true});
  for(const [name,source,target] of [['core',originalExecutable,executable],['factory',originalFactory,factory],['keys',originalKeys,keys]]){hashes[`${name}Source`]=await hash(source);await copyFile(source,target);hashes[`${name}Before`]=await hash(target);assert.equal(hashes[`${name}Before`],hashes[`${name}Source`]);}
  app=await start();const server=await rpc(app.page,'servers.create',{name:'恢复隔离档案',host:'127.0.0.1',port:1,username:'isolated',authType:'password'});await rpc(app.page,'workbench.activateWorkspace',{serverId:server.id});await invoke('settings.set',{fontSize:17});
  const original=(await rpc(app.page,'workbench.snapshot')).views.find(view=>view.viewType==='terminal.sessions');assert.ok(original);
  const schemaPackage=await candidate('0.1.6');await rpc(app.page,'plugins.prepareUpdate',{path:schemaPackage});await normalStop();app=await start();assert.equal((await status()).activeVersion,'0.1.6');
  const incompatible=(await rpc(app.page,'workbench.snapshot')).views.find(view=>view.viewId===original.viewId);await writeFile(join(output,'schema-before-recovery.json'),JSON.stringify(incompatible,null,2));await app.page.screenshot({path:join(output,'schema-before-recovery.png')});
  assert.equal(incompatible.status,'viewFailed','UPD-05: persisted schema-1 view must not silently load schema 2');
  const placeholder=app.page.locator(`[data-view-id="${original.viewId}"]`);await placeholder.getByText(/版本.*不兼容|不兼容.*版本/).waitFor();assert.equal((await rpc(app.page,'servers.list')).servers.length,1);assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);await rpc(app.page,'calls.list');
  const blocker=join(data,'workbench.pending'),layout=join(data,'workbench.json');
  const beforeBlocked={memory:await rpc(app.page,'workbench.snapshot'),disk:await readFile(layout,'utf8')};let blockedError;
  await mkdir(blocker);
  try{
    await assert.rejects(rpc(app.page,'workbench.recover',{viewId:original.viewId}),error=>{blockedError={code:error.code,message:error.message};return error.code==='STORAGE_ERROR';});
    const afterBlocked={memory:await rpc(app.page,'workbench.snapshot'),disk:await readFile(layout,'utf8')};
    await writeFile(join(output,'schema-storage-failure.json'),JSON.stringify({blocker,error:blockedError,before:beforeBlocked,after:afterBlocked},null,2));
    assert.deepEqual(afterBlocked.memory,beforeBlocked.memory,'Rejected schema reset must preserve the complete in-memory layout');
    assert.equal(afterBlocked.disk,beforeBlocked.disk,'Rejected schema reset must preserve the persisted layout byte-for-byte');
    await placeholder.getByText(/版本.*不兼容|不兼容.*版本/).waitFor();assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);
  }finally{await rmdir(blocker);}
  checks.push('WB-12 recovery storage failure: real workbench.pending directory conflict returns STORAGE_ERROR without changing schema, status, revision, memory or saved layout; removing the empty blocker permits retry');
  await placeholder.getByRole('button',{name:'重新加载视图',exact:true}).click();await until(async()=>{const view=(await rpc(app.page,'workbench.snapshot')).views.find(view=>view.viewId===original.viewId);return view.status==='ready'&&view.stateVersion===2&&!view.failureReason;});const restoredPage=await viewPage(original.viewId);await restoredPage.getByText('还没有终端',{exact:true}).waitFor();assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);await app.page.screenshot({path:join(output,'schema-after-recovery.png')});await restoredPage.screenshot({path:join(output,'schema-restored-plugin.png')});
  checks.push('UPD-05: schema-1 view becomes a diagnosable placeholder under schema 2; explicit reload rebuilds the same view with schema 2 and no SSH');
  if(!schemaOnly){
    const faultPackage=await candidate('0.1.7',async(source,manifest)=>{const path=join(source,manifest.entry.worker);await writeFile(path,`${await readFile(path,'utf8')}\nconst originalPrepare=globalThis.sodeckPlugin.prepare;globalThis.sodeckPlugin.prepare=async context=>{await originalPrepare?.(context);await sodeck.call('storage.set',{key:'fontSize',value:23});await sodeck.call('storage.set',{key:'interruptionReady',value:true});await new Promise(resolve=>setTimeout(resolve,3000));throw Error('ACTUAL_PRECOMMIT_INTERRUPTION_FIXTURE');};\n`);});
    await invoke('settings.set',{fontSize:17});await rpc(app.page,'plugins.prepareUpdate',{path:faultPackage});const previous=await activation(),old=selected(previous);await normalStop();raw=rawStartup();
    const copied=await until(async()=>{const recovery=await json(join(data,'plugins/recovery.json'));for(const name of await readdir(join(data,'plugins/data/sodeck.terminal'))){if(name===old.dataGeneration)continue;try{const settings=await json(settingsPath(name));if(settings.interruptionReady===true&&settings.fontSize===23)return{recovery,dataGeneration:name,settings};}catch{}}},10000);
    const beforeKill=await activation();assert.equal(beforeKill.generation,previous.generation);assert.equal(selected(beforeKill).dataGeneration,old.dataGeneration);assert.equal((await json(settingsPath(old.dataGeneration))).fontSize,17);await killActual(raw,'precommit-termination',{activation:beforeKill,candidate:copied});raw=undefined;
    app=await start();const fallback=await status();assert.equal(fallback.activeVersion,'0.1.6');assert.equal(fallback.pendingVersion,null);assert.match(fallback.error,/ACTUAL_PRECOMMIT_INTERRUPTION_FIXTURE/);assert.equal(selected(await activation()).dataGeneration,old.dataGeneration);assert.equal((await invoke('settings.get')).fontSize,17);assert.equal((await rpc(app.page,'servers.list')).servers.length,1);
    checks.push('UPD-04 precommit: actual Tauri termination after journal and copied-data writes; restart preserves old version/data and clears the failed pending candidate');
    await invoke('settings.set',{fontSize:21});const committed=await activation();assert.equal((await json(settingsPath(selected(committed).dataGeneration))).fontSize,21);await killActual(app,'postcommit-termination',{activation:committed,settings:await json(settingsPath(selected(committed).dataGeneration))});app=undefined;
    app=await start();assert.equal((await status()).activeVersion,'0.1.6');assert.equal((await invoke('settings.get')).fontSize,21);assert.equal(selected(await activation()).dataGeneration,selected(committed).dataGeneration);assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);checks.push('UPD-04 postcommit running interruption: committed data generation and subsequent font setting 21 survive actual process termination and restart');
  }
  assert.deepEqual(app.errors,[]);await normalStop();hashes.coreAfter=await hash(executable);hashes.factoryAfter=await hash(factory);hashes.keysAfter=await hash(keys);assert.equal(hashes.coreAfter,hashes.coreBefore);assert.equal(hashes.factoryAfter,hashes.factoryBefore);assert.equal(hashes.keysAfter,hashes.keysBefore);
  await writeFile(join(output,'result.json'),JSON.stringify({passed:true,checks,hashes,interruptions,normalStops,schemaOnly},null,2));console.log(JSON.stringify({passed:true,output,checks,hashes,normalStops}));
}catch(error){const diagnostic={};if(app)for(const method of ['workbench.snapshot','plugins.list','resources.list']){try{diagnostic[method]=await rpc(app.page,method);}catch(failure){diagnostic[method]={error:failure.message};}}await writeFile(join(output,'result.json'),JSON.stringify({passed:false,checks,hashes,interruptions,normalStops,schemaOnly,error:{message:error.message,stack:error.stack},diagnostic},null,2));console.error(error);console.error(`Evidence: ${output}`);process.exitCode=1;}
finally{await stop(app);if(raw&&raw.process.exitCode===null){raw.process.kill();await writeFile(join(output,'aborted-startup.log'),raw.output());}}
