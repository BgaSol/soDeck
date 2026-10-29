import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,until,stop,nativeWindow} from './harness.mjs';
import {fixturePackage} from './settings-package-mutate.mjs';
const output=resolve('.qa/t028/import-'+Date.now());await mkdir(output,{recursive:true});
const baseline=resolve('.local/dev/SoDeck-v2-T027-r9/resources');process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy','monitor'].map(n=>join(baseline,n+'.sodeck-plugin')).join(delimiter);
const checks=[];let app,settings,failure;
const note=s=>{checks.push(s);console.log(s);};
async function operation(operationId){return until(async()=>{const result=await rpc(app.page,'plugins.package.operation.get',{operationId});return !['selecting','verifying'].includes(result.state)&&result;},20000);}
try{
  app=await launch({data:join(output,'data'),shellPort:13301,pluginPort:13302});const before=await rpc(app.page,'plugins.management.list');
  await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:plugins'});settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});
  await settings.getByRole('heading',{name:'插件管理',exact:true}).waitFor();await settings.getByLabel('启用 文件',{exact:true}).uncheck();
  const selecting=crypto.randomUUID();await rpc(settings,'plugins.package.prepare',{operationId:selecting});
  const dialog=await until(async()=>(await nativeWindow(app,'list')).find(w=>w.visible&&/打开|Open/.test(w.title)));await writeFile(join(output,'picker.json'),JSON.stringify(dialog,null,2));
  await assert.rejects(rpc(app.page,'plugins.package.prepare',{operationId:crypto.randomUUID()}),e=>e.code==='BUSY');
  await rpc(settings,'ui.settings.close.request',{reason:'close'});await settings.getByRole('button',{name:'继续编辑',exact:true}).waitFor();await settings.getByRole('button',{name:'放弃更改',exact:true}).click();await until(async()=>(await settings.locator('body').innerText()).includes('设置仍在保存'));assert.equal(settings.isClosed(),false);
  await nativeWindow(app,'escape',{WindowHandle:dialog.handle});assert.equal((await operation(selecting)).state,'cancelled');await settings.getByRole('button',{name:'继续编辑',exact:true}).click();assert.equal((await rpc(app.page,'plugins.management.list')).items.length,before.items.length);
  note('Native file picker cancellation has no package effect; concurrent import and closing while picker active are blocked');
  const path=resolve('.qa/t028/third-party.sodeck-plugin'),op=crypto.randomUUID();await rpc(settings,'plugins.package.prepare',{operationId:op,path});assert.equal((await operation(op)).state,'prepared');
  await settings.getByLabel('启用 第三方验证插件',{exact:true}).waitFor();assert.equal(await settings.getByLabel('启用 文件',{exact:true}).isChecked(),false);assert.equal(await settings.getByLabel('启用 第三方验证插件',{exact:true}).isChecked(),true);
  await settings.getByRole('button',{name:'取消',exact:true}).click();await settings.getByRole('button',{name:'放弃更改',exact:true}).click();await until(()=>settings.isClosed());
  let rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.find(r=>r.pluginId==='sodeck.files').enabled,true);assert.equal(rows.find(r=>r.pluginId==='example.third-party').pending.signature.status,'unsigned');
  note('Third-party new ID defaults enabled; import refresh preserves existing draft, settings cancel keeps the prepared package');
  const unknown=join(output,'unknown.sodeck-plugin');await writeFile(unknown,await fixturePackage(join(baseline,'terminal.sodeck-plugin'),e=>{const m=JSON.parse(e.get('manifest.json'));m.version='0.13.8';e.set('manifest.json',Buffer.from(JSON.stringify(m)));}));
  await writeFile(unknown,await fixturePackage(unknown,e=>{const s=JSON.parse(e.get('signature.json'));s.keyId='unknown-publisher';e.set('signature.json',Buffer.from(JSON.stringify(s)));},{resign:false}));
  const u=crypto.randomUUID();await rpc(app.page,'plugins.package.prepare',{operationId:u,path:unknown});assert.equal((await operation(u)).signature.status,'unverified');
  const bad=join(output,'bad-signature.sodeck-plugin');await writeFile(bad,await fixturePackage(unknown,e=>e.set('signature.json',Buffer.from('{}')),{resign:false}));const b=crypto.randomUUID();await rpc(app.page,'plugins.package.prepare',{operationId:b,path:bad});assert.equal((await operation(b)).code,'PACKAGE_SIGNATURE_INVALID');
  rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.find(r=>r.pluginId==='sodeck.terminal').pending.signature.status,'unverified');note('Unknown publisher is marked unverified; malformed signature is rejected without replacing the candidate');
  const first=await rpc(app.page,'plugins.management.list',{limit:1});assert.ok(first.nextCursor);const pref=await rpc(app.page,'plugins.enablement.get');await rpc(app.page,'plugins.enablement.commit',{generation:pref.generation,expectedRevision:pref.revision,changes:[{pluginId:'sodeck.files',enabled:false}],operationId:crypto.randomUUID()});await assert.rejects(rpc(app.page,'plugins.management.list',{cursor:first.nextCursor,limit:1}),e=>e.code==='CONFLICT');await assert.rejects(rpc(app.page,'plugins.management.list',{limit:101}),e=>e.code==='INVALID_PARAMS');note('Catalog cursor binds saved revision and over-limit page requests reject');
  await stop(app);assert.equal(app.shutdown.forced,false);app=await launch({data:join(output,'data'),shellPort:13301,pluginPort:13302});rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.find(r=>r.pluginId==='example.third-party').runtimeStatus,'running');assert.equal(rows.find(r=>r.pluginId==='sodeck.terminal').selected.signature.status,'unverified');
  const active=join(output,'data/plugins/packages/sodeck.terminal/0.13.8/worker/main.js');await stop(app);app=undefined;await writeFile(active,'throw Error("MUST_NOT_EXECUTE")');app=await launch({data:join(output,'data'),shellPort:13301,pluginPort:13302,allowStartupWarning:true});rows=(await rpc(app.page,'plugins.management.list')).items;assert.equal(rows.find(r=>r.pluginId==='sodeck.terminal').selected.valid,false);assert.equal((await rpc(app.page,'diagnostics.get')).pluginInstances.some(r=>r.pluginId==='sodeck.terminal'),false);
  note('Unsigned third-party and unknown-signer packages really start; tampered selected worker is rejected on next startup and remains listed');
}catch(e){failure={message:e.message,stack:e.stack};throw e;}
finally{if(app){if(settings&&!settings.isClosed()){await settings.getByRole('button',{name:'继续编辑',exact:true}).click({timeout:500}).catch(()=>{});await settings.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});await settings.getByRole('button',{name:'放弃更改',exact:true}).click({timeout:500}).catch(()=>{});}await stop(app);}const result={passed:!failure,output,checks,failure};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t028/import-latest.json',JSON.stringify(result,null,2));}
