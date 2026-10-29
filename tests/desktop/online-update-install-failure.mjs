// Native post-cleanup failure. Computer Use selects the displayed reopen action.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {chromium} from 'playwright';
import {launchInstalled} from './installed-updates-harness.mjs';
import {rpc,until,stop} from './harness.mjs';
const base=resolve('.qa/t029/release-local'),config=JSON.parse(await readFile(join(base,'config.json'),'utf8')),built=JSON.parse(await readFile(join(base,'failure-A/built.json'),'utf8'));
const install=join(base,'install'),data=join(process.env.APPDATA,config.identifier),output=join(base,'install-failure-'+Date.now());await mkdir(output,{recursive:true});
const product=built.products.find(p=>p.componentId==='core');let app,browser,failure;const checks=[];
try{
  assert.equal(spawnSync(join(base,'failure-A/assets',product.name),['/S','/D='+install],{windowsHide:true,timeout:120000}).status,0);
  app=await launchInstalled({config,install});assert.equal((await rpc(app.page,'updates.status.get')).buildId,product.buildId);
  const plan=await rpc(app.page,'updates.plan.get',{componentIds:['core']}),downloadId=crypto.randomUUID();assert.deepEqual(plan.blockedReasons,[]);
  await rpc(app.page,'updates.download',{operationId:downloadId,planRevision:plan.planRevision,componentIds:['core']});await until(async()=>{const op=await rpc(app.page,'updates.operation.get',{operationId:downloadId});if(op.error)throw Error(op.error.message);return op.stage==='downloaded';},30000);
  const installId=crypto.randomUUID();await rpc(app.page,'updates.install',{operationId:installId,planRevision:plan.planRevision,componentIds:['core']});
  const journal=await until(async()=>{const s=JSON.parse(await readFile(join(data,'updates/state.json'),'utf8'));return s.handoff.operationId===installId&&s.handoff.stage==='failed'&&s;},30000);
  assert.equal(journal.operations.find(o=>o.operationId===installId).error.code,'UPDATE_INSTALL_FAILED');
  await writeFile(join(output,'ready.json'),JSON.stringify({pid:app.process.pid,installId,output}));console.log(JSON.stringify({stage:'native-failure-dialog-ready',pid:app.process.pid,output}));
  await until(()=>app.process.exitCode!==null,180000);await app.browser.close().catch(()=>{});app=undefined;
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:13501'),45000);const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));await page.waitForSelector('#root > *');
  const status=await rpc(page,'updates.status.get');assert.equal(status.buildId,product.buildId);assert.equal(status.appVersion,product.appVersion);assert.equal((await rpc(page,'updates.operation.get',{operationId:installId})).stage,'failed');assert.equal((await rpc(page,'resources.list')).ptys.length,0);
  checks.push('Verified download succeeded, native installer staging failed only after close/cleanup, and failure receipt is explicit','Native reopen action starts the original A build without retrying installation or restoring sessions');
  await rpc(page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'core:updates'});const settings=await until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});await settings.getByRole('heading',{name:'更新',exact:true}).waitFor();await settings.screenshot({path:join(output,'reopened-A.png')});await settings.getByRole('button',{name:'取消',exact:true}).click();await rpc(page,'app.exit').catch(()=>{});
  await until(async()=>{try{await fetch('http://127.0.0.1:13501/json/version');return false;}catch{return true;}},15000);
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{if(app)await stop(app);await browser?.close().catch(()=>{});const result={passed:!failure,kind:'native-installer-staging-failure',injection:'QA source copy changes TEMP/TMP only at the actual Tauri install call; root/delivery unchanged',output,checks,failure};await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t029/installer-failure.json',JSON.stringify(result,null,2));}
