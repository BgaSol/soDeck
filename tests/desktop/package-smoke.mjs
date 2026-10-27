import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {cp,mkdir,readFile,readdir,writeFile} from 'node:fs/promises';
import {delimiter,join,relative,resolve} from 'node:path';
import {chromium} from 'playwright';
import {root,launch,rpc,pluginRpc,stop,until} from './harness.mjs';

const source=resolve(process.env.SODECK_TEST_PACKAGE_DIR??join(root,'.local/dev/SoDeck-v2-T009'));
const output=join(root,'.qa/package-smoke',String(Date.now()));
const packageRoot=join(output,'relocated package');
await mkdir(output,{recursive:true});let app,networkBrowser,networkRule;
const previous={packages:process.env.SODECK_PLUGIN_PACKAGES,ports:process.env.SODECK_PLUGIN_DEBUG_PORTS};
try{
  const build=JSON.parse(await readFile(join(source,'build.json'),'utf8'));
  const multihop=build.deliveryTask==='T013-Multihop';
  const network=build.deliveryTask==='T014-NetworkTunnel';
  const t012=build.deliveryTask==='T012',t013=build.deliveryTask==='T013',t011=build.deliveryTask==='T011',t010=build.deliveryTask==='T010',dual=network||multihop||t012||t013||t011||t010||build.deliveryTask==='T008';
  if(network||multihop||t011||t012){delete process.env.SODECK_THEME_FILE;delete process.env.SODECK_TEST_BRAND_FILE;}
  const expectedFiles=[...Object.keys(build.hashes),'build.json'].sort();
  // Do not silently exclude data, signing keys or credentials from the audit.
  assert.deepEqual((await listFiles(source)).sort(),expectedFiles,'delivery must contain only declared files');
  await cp(source,packageRoot,{recursive:true,errorOnExist:true,force:false});
  for(const [file,expected] of Object.entries(build.hashes))assert.equal(createHash('sha256').update(await readFile(join(packageRoot,file))).digest('hex'),expected,file);
  const terminal=join(packageRoot,'resources/terminal.sodeck-plugin');
  if(dual){
    assert.equal(build.pluginApi,4);assert.equal(build.pluginApiVersion,network?'4.7.0':multihop?'4.6.0':t012?'4.5.0':t013?'4.4.0':t011?'4.3.0':t010?'4.2.0':'4.1.0');
    if(!network&&!multihop&&!t010&&!t011&&!t012&&!t013)assert.equal(build.hashes['resources/terminal.sodeck-plugin'],'cee21412074f6f39c82e092dcb3a6d38919df3bd9b3424fd58a703939e8fe1b2');
    process.env.SODECK_PLUGIN_PACKAGES=[terminal,join(packageRoot,'resources/files.sodeck-plugin'),...(network?[join(packageRoot,'resources/network-proxy.sodeck-plugin')]:[])].join(delimiter);
    process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':9504,'sodeck.files':9505,...(network?{'sodeck.network-proxy':9506}:{})});
  }else{delete process.env.SODECK_PLUGIN_PACKAGES;delete process.env.SODECK_PLUGIN_DEBUG_PORTS;}
  const options={data:join(output,'app'),shellPort:9503,pluginPort:9504,executable:join(packageRoot,'sodeck-desktop.exe'),packagePath:terminal,keysPath:join(packageRoot,'resources/development-public-key.json'),cwd:packageRoot};
  app=await launch(options);
  assert.equal((await rpc(app.page,'app.info')).version,build.version);
  const plugins=await rpc(app.page,'plugins.list');
  assert.equal(plugins.find(p=>p.pluginId==='sodeck.terminal').activeVersion,build.pluginVersion);
  if(dual)assert.equal(plugins.find(p=>p.pluginId==='sodeck.files').activeVersion,network||multihop?'0.4.1':t012?'0.4.0':t013?'0.3.3':t011?'0.3.0':t010?'0.2.0':'0.1.0');
  if(network)assert.equal(plugins.find(p=>p.pluginId==='sodeck.network-proxy').activeVersion,'0.1.0');
  if(network||multihop||t012){const theme=await rpc(app.page,'ui.theme.catalog.get');assert.equal(theme.selectedThemeId,'dopamine');await rpc(app.page,'ui.theme.choose',{themeId:'soft-light',expectedRevision:theme.revision});}
  if(t011){const theme=await rpc(app.page,'ui.theme.preference.get');assert.equal(theme.mode,'dark');await rpc(app.page,'ui.theme.select',{mode:'light',expectedRevision:theme.revision});}
  assert.equal((await rpc(app.page,'servers.list')).servers.length,0,'no bundled server profile');
  assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0,'clean initial launch');
  const server=await rpc(app.page,'servers.create',{name:'目录包离线验收',host:'127.0.0.1',port:1,username:'fixture',authType:'password'});
  const opened=await rpc(app.page,'workspaces.open',{serverId:server.id,operationId:crypto.randomUUID()});
  if(build.pluginApi>=4){
    assert.ok(opened.generation);assert.equal(opened.initializers.length,1);assert.equal(opened.persisted,false);
    await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal')?.status==='error');
  }
  const initial=await rpc(app.page,'workbench.snapshot');
  assert.equal(initial.schemaVersion,build.pluginApi);
  if(dual){assert.equal(initial.views.length,2);assert.equal(initial.views.find(v=>v.pluginId==='sodeck.files').placement,'left');}
  assert.deepEqual(await app.page.locator('.sessions-panel .panel-heading > span').allTextContents(),dual?['文件']:[]);
  if(network){
    await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});
    networkBrowser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9506'));
    const page=await until(()=>networkBrowser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));
    await page.getByRole('button',{name:'新建内网代理',exact:true}).waitFor();
    networkRule=await pluginRpc(page,'plugin.invoke',{command:'network.save',args:{expectedRevision:0,rule:{name:'目录包配置保留',type:'network-proxy',bindMode:'fixed',port:10808,autoReconnect:false}}});
    assert.equal((await rpc(app.page,'net.resources.list')).items.length,0);
  }
  const moved=await rpc(app.page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});assert.equal(moved.committed,true);
  await rpc(app.page,'windows.merge',{sourceWindowId:moved.targetWindowId,operationId:crypto.randomUUID()});
  assert.deepEqual((await rpc(app.page,'workbench.snapshot')).workspaceIds,[server.id]);
  const resources=await rpc(app.page,'resources.list');for(const key of ['ptys','leases','connections'])assert.equal(resources[key].length,0);
  const instances=dual?(await rpc(app.page,'diagnostics.get')).pluginInstances:[];
  await stop(app);assert.equal(app.process.exitCode,0);assert.equal(app.shutdown.forced,false);app=null;
  await networkBrowser?.close();networkBrowser=undefined;
  for(const instance of instances)await until(()=>!processAlive(instance.processId));
  if(build.pluginApi>=4){
    app=await launch(options);
    if(network||multihop||t011||t012)assert.equal((await rpc(app.page,'ui.theme.get')).mode,'light');
    const snapshot=await rpc(app.page,'workbench.snapshot');assert.equal(snapshot.views.length,0);assert.equal(snapshot.workspaces.length,0);assert.equal((await rpc(app.page,'servers.list')).servers.length,1);
    const restartedResources=await rpc(app.page,'resources.list');for(const key of ['ptys','leases','connections'])assert.equal(restartedResources[key].length,0);
    if(network){
      assert.equal((await rpc(app.page,'net.resources.list')).items.length,0);
      await rpc(app.page,'workspaces.open',{serverId:server.id});
      await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});
      networkBrowser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9506'));
      const page=await until(()=>networkBrowser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));
      await page.getByRole('button',{name:'新建',exact:true}).waitFor();
      const state=await pluginRpc(page,'plugin.invoke',{command:'network.snapshot',args:{}});
      assert.equal(state.items[0].rule.id,networkRule.id);assert.ok(!state.items[0].run);
    }
    const restartedInstances=dual?(await rpc(app.page,'diagnostics.get')).pluginInstances:[];
    await stop(app);assert.equal(app.process.exitCode,0);assert.equal(app.shutdown.forced,false);app=null;
    await networkBrowser?.close();networkBrowser=undefined;
    for(const instance of restartedInstances)await until(()=>!processAlive(instance.processId));
  }
  const result={passed:true,source,relocated:packageRoot,version:build.version,sourceFingerprint:build.sourceFingerprint,hashes:{core:build.hashes['sodeck-desktop.exe'],terminal:build.hashes['resources/terminal.sodeck-plugin'],files:build.hashes['resources/files.sodeck-plugin']},checks:['entire delivery matches declared file list; no bundled data or credentials','relocated files match all hashes; startup cwd outside source workspace',dual?'original terminal and files load independently, with API 4 and API version 4.1 distinguished':'terminal loads',build.pluginApi>=4?'initial terminal failure stays visible; initializer is unique; detach/merge and clean restart preserve profile':'offline workspace detaches and merges','normal exit code 0; captured plugin worker PIDs exit']};
  await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));
  if(t010){result.checks[2]='terminal 0.5 and files 0.2 load independently with API 4.2';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(t011){result.checks[2]='terminal 0.6 and files 0.3 load independently with API 4.3; light preference survives relocated package restart';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(t013){result.checks[2]='terminal 0.6.0 and files 0.3.3 load independently with API 4.4; relocated package clean restart succeeds';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(multihop){result.checks[2]='terminal 0.7.1 and files 0.4.1 load with API 4.6; fresh dopamine and persisted light survive relocated clean restart';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(t012){result.checks[2]='terminal 0.7.0 and files 0.4.0 load with API 4.5; fresh dopamine and persisted light survive relocated clean restart';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(network){result.hashes.network=build.hashes['resources/network-proxy.sodeck-plugin'];result.checks[2]='terminal 0.7.1, files 0.4.1 and network 0.1.0 load with API 4.7; manual network view, persisted rule without run, and light preference survive relocated clean restart';await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));}
  if(dual)await writeFile(join(root,network?'.qa/t014/package-smoke.json':multihop?'.qa/t013-multihop/package-smoke.json':t012?'.qa/t012/package-smoke.json':t013?'.qa/t013/package-smoke.json':t011?'.qa/t011/package-smoke.json':t010?'.qa/t010/package-smoke.json':'.qa/t008/package-smoke.json'),JSON.stringify(result,null,2));
  console.log(JSON.stringify({passed:true,output}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{
  await stop(app);
  await networkBrowser?.close().catch(()=>{});
  for(const [key,value] of [['SODECK_PLUGIN_PACKAGES',previous.packages],['SODECK_PLUGIN_DEBUG_PORTS',previous.ports]])if(value===undefined)delete process.env[key];else process.env[key]=value;
}
async function listFiles(directory){
  const entries=await readdir(directory,{withFileTypes:true}),files=[];
  for(const entry of entries){const path=join(directory,entry.name);if(entry.isDirectory())files.push(...await listFiles(path));else{assert.ok(entry.isFile(),'unexpected link in delivery');files.push(relative(source,path).replaceAll('\\','/'));}}
  return files;
}
function processAlive(pid){assert.ok(Number.isInteger(pid)&&pid>0);try{process.kill(pid,0);return true;}catch(error){if(error.code==='ESRCH')return false;throw error;}}
