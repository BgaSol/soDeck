// Real Windows WebViews and persistence; no public release or installer is used here.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,until,stop} from './harness.mjs';
const output=resolve('.qa/t029/native',String(Date.now())),data=join(output,'data');await mkdir(output,{recursive:true});
const baseline=resolve('.local/dev/SoDeck-v2-T028-r1/resources');
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy','monitor'].map(n=>join(baseline,n+'.sodeck-plugin')).join(delimiter);
const checks=[];let app,settings,failure;
async function start(){app=await launch({data,shellPort:13401,pluginPort:13402,keysPath:join(baseline,'development-public-key.json')});}
async function finish(){if(app){await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;}}
try{
  await start();const status=await rpc(app.page,'updates.status.get');assert.equal(status.channel,'development');assert.equal(status.configured,false);assert.equal(status.appVersion,'2.0.0-dev.21');
  assert.equal((await rpc(app.page,'plugins.management.list')).items.length,4);
  await assert.rejects(rpc(app.page,'updates.check',{operationId:crypto.randomUUID()}),e=>e.code==='UPDATE_NOT_CONFIGURED');
  await assert.rejects(rpc(app.page,'updates.download',{operationId:crypto.randomUUID(),planRevision:'unknown',componentIds:['core'],url:'http://example.com'}),e=>e.code==='INVALID_PARAMS');
  checks.push('Native development identity is explicit, automatic production checks are disabled, arbitrary URL input is rejected');
  await rpc(app.page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:updates'});
  settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});
  await settings.getByRole('heading',{name:'更新',exact:true}).waitFor();await settings.getByLabel('自动检查并提示更新',{exact:true}).uncheck();
  await settings.getByRole('button',{name:/^插件管理/}).click();await settings.getByRole('button',{name:/^更新/}).click();assert.equal(await settings.getByLabel('自动检查并提示更新',{exact:true}).isChecked(),false);
  await settings.getByRole('button',{name:'应用',exact:true}).click();await settings.getByText('更改已应用',{exact:true}).waitFor();assert.equal((await rpc(app.page,'updates.status.get')).policy.autoCheck,false);
  await settings.screenshot({path:join(output,'native-updates.png')});checks.push('Native settings shares policy drafts across pages and applies a durable revisioned preference');
  const policy=(await rpc(app.page,'updates.status.get')).policy;await assert.rejects(rpc(app.page,'updates.policy.commit',{generation:policy.generation,expectedRevision:'0',autoCheck:true,operationId:crypto.randomUUID()}),e=>e.code==='CONFLICT');
  await settings.getByRole('button',{name:'确定',exact:true}).click();await until(()=>settings.isClosed());
  const importId=crypto.randomUUID();await rpc(app.page,'plugins.package.prepare',{operationId:importId,path:join(baseline,'terminal.sodeck-plugin')});
  const imported=await until(async()=>{const r=await rpc(app.page,'plugins.package.operation.get',{operationId:importId});if(r.state==='failed')throw Error(r.error);return r.state==='prepared'&&r;});
  const sources=JSON.parse(await readFile(join(data,'updates/sources.json'),'utf8'));assert.equal(sources.plugins['sodeck.terminal'].followOfficial,false);assert.equal(sources.plugins['sodeck.terminal'].packages[imported.digest],'local');
  checks.push('Actual manual same-byte import marks the candidate local before activation and stops official following');
  await finish();await start();assert.equal((await rpc(app.page,'updates.status.get')).policy.autoCheck,false);assert.equal((await rpc(app.page,'plugins.management.list')).items.length,4);
  checks.push('Full application restart preserves update policy and all four plugin selections');
}catch(e){failure={message:e.message,stack:e.stack};if(app)await app.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});throw e;}
finally{if(settings&&!settings.isClosed()){await settings.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});await settings.getByRole('button',{name:'放弃更改',exact:true}).click({timeout:500}).catch(()=>{});}await finish();const result={passed:!failure,kind:'native-windows-development',installer:false,publicGithub:false,output,checks,failure};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t029/native-latest.json',JSON.stringify(result,null,2));}
