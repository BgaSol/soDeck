import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {cp,mkdir,mkdtemp,readFile,readdir,writeFile} from 'node:fs/promises';
import {join,resolve,relative} from 'node:path';
import {launch,rpc,until,stop} from './harness.mjs';
const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-T019'),output=resolve('.qa/t019/package-smoke');
await mkdir(output,{recursive:true});const run=await mkdtemp(join(output,'run-')),destination=join(run,'relocated package'),data=join(run,'data');
const manifest=JSON.parse(await readFile(join(source,'build.json'),'utf8'));
assert.equal(JSON.parse(await readFile(join(source,'portable.json'),'utf8')).schemaVersion,1);
const index=JSON.parse(await readFile(join(source,'resources/bundled-plugins.json'),'utf8'));
assert.equal(index.schemaVersion,1);assert.equal(index.publicKeys,'development-public-key.json');
assert.deepEqual(index.plugins.map(p=>p.pluginId).sort(),manifest.pluginPackages.map(p=>p.pluginId).sort());
for(const plugin of index.plugins)assert.equal(plugin.path,plugin.pluginId.replace('sodeck.','')+'.sodeck-plugin');
async function list(path){const result=[];for(const entry of await readdir(path,{withFileTypes:true})){const file=join(path,entry.name);if(entry.isDirectory())result.push(...await list(file));else result.push(relative(source,file).replaceAll('\\','/'));}return result;}
assert.deepEqual((await list(source)).sort(),[...Object.keys(manifest.hashes),'build.json'].sort());
for(const [file,hash] of Object.entries(manifest.hashes))assert.equal(createHash('sha256').update(await readFile(join(source,file))).digest('hex'),hash);
await cp(source,destination,{recursive:true});
let app;
try{
  app=await launch({bundledResources:true,data,shellPort:9393,pluginPort:9394,executable:join(destination,'sodeck-desktop.exe'),cwd:destination});
  const info=await rpc(app.page,'app.info');assert.equal(info.version,'2.0.0-dev.15');
  const pluginList=await rpc(app.page,'plugins.list');
  for(const pkg of manifest.pluginPackages){const plugin=pluginList.find(p=>p.pluginId===pkg.pluginId);assert.ok(plugin,`Missing ${pkg.pluginId}`);assert.equal(plugin.activeVersion,pkg.version);}
  await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID()});
  const monitor=await until(async()=>{for(const page of app.browser.contexts().flatMap(c=>c.pages()))if(await page.evaluate(()=>Boolean(window.__SODECK_MONITOR_CONTEXT__)).catch(()=>false))return page;});
  await monitor.getByRole('heading',{name:'通信监控 · 全部服务器',exact:true}).waitFor();
  for(const name of ['概览','历史','诊断','设置'])assert.equal(await monitor.getByRole('tab',{name,exact:true}).count(),1);
  assert.equal(await monitor.getByRole('tab',{name:'测速',exact:true}).count(),0);
  await monitor.getByRole('tab',{name:'诊断',exact:true}).click();assert.equal(await monitor.getByRole('button',{name:'已知悉',exact:true}).count(),0);
  assert.equal(await monitor.getByRole('alert').count(),0);
  await stop(app);assert.equal(app.shutdown.forced,false);assert.equal(app.process.exitCode,0);app=undefined;
  await writeFile(join(output,'results.json'),JSON.stringify({passed:true,version:manifest.version,packageRevision:manifest.packageRevision,files:Object.keys(manifest.hashes).length,plugins:manifest.pluginPackages,relocated:true,bundledResources:true,debugTransport:true,ordinaryStartup:false,normalExit:true},null,2));
  console.log('Relocated package: bundled index, three plugins without path overrides, monitor sections and normal exit passed; ordinary startup is verified separately');
}finally{if(app)await stop(app);}
