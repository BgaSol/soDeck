import assert from 'node:assert/strict';
import {cp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {launch,rpc,until,stop,nativeWindow} from './harness.mjs';
const output=resolve('.qa/t023/save-fault',String(Date.now()));await mkdir(output,{recursive:true});
const source=join(output,'source');await cp('plugins/terminal/dist',source,{recursive:true});
const worker=join(source,'worker/main.js');await writeFile(worker,(await readFile(worker,'utf8'))+`
// Test-only: disk commit succeeds; provider never confirms runtime application.
const settingsOriginalInvoke=globalThis.sodeckPlugin.invoke;
globalThis.sodeckPlugin.invoke=async function(command,p,context){
 if(command==='settings.provider.commit'){
  await sodeck.call('storage.preferences.commit',{commitTicket:p.ticket,changes:p.changes});
  await new Promise(()=>{});
 }
 return settingsOriginalInvoke.call(this,command,p,context);
};
`);
const packagePath=join(output,'pending.sodeck-plugin');execFileSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','terminal','--source',source,'--version','0.11.0-rc.5','--out',packagePath],{windowsHide:true,stdio:'pipe'});
let app,page;const checks=[];
try{
  app=await launch({data:join(output,'data'),packagePath,shellPort:9881,pluginPort:9882});
  await rpc(app.page,'ui.settings.open',{pageId:'plugin:sodeck.terminal:general',operationId:crypto.randomUUID()});page=await until(()=>app.browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('settings')));await page.getByLabel('字号',{exact:true}).waitFor();
  const baseline=(await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot;
  await page.getByLabel('字号',{exact:true}).fill('18');await page.getByRole('button',{name:'应用',exact:true}).click();await page.getByText('正在应用…',{exact:true}).waitFor();
  const began=Date.now();await rpc(app.page,'app.info');assert.ok(Date.now()-began<2000);
  const win=(await nativeWindow(app,'list')).find(w=>w.title==='设置');await nativeWindow(app,'close',{WindowHandle:win.handle});await page.getByText('设置仍在保存，请等待结果。').waitFor();await expectDisabled(page.getByRole('button',{name:'放弃更改',exact:true}));await page.getByRole('button',{name:'继续编辑'}).click();
  await until(()=>page.locator('.settings-footer').innerText().then(t=>t.includes('更改已')),18000);
  const calls=await rpc(app.page,'calls.list',{method:'settings.commit',limit:10});const completed=calls.records.find(c=>c.status==='succeeded');assert.ok(completed);assert.equal(completed.summary.synchronization,'pending');
  const current=(await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot;const file=JSON.parse(await readFile(join(app.data,'plugins/data/sodeck.terminal',baseline.storageGeneration,'settings.json'),'utf8'));const saved=file.__sodeckPreferences.groups.general.receipts.at(-1);
  const receipt=await rpc(app.page,'settings.operation.get',{pageId:current.pageId,storageGeneration:current.storageGeneration,operationId:saved.operationId});assert.equal(receipt.state,'saved');assert.equal(current.values.find(v=>v.key==='fontSize').value,18);const captured={result:{result:receipt.result}};
  await until(()=>page.getByRole('button',{name:'应用',exact:true}).isDisabled());await page.getByRole('button',{name:'确定',exact:true}).click();await until(()=>page.isClosed());checks.push('Real worker storage callback completes without locks blocking unrelated RPC','Native close during save preserves busy state until explicit keep-editing','Provider timeout after atomic save returns saved/pending, durable receipt survives and read repairs runtime without another commit');
  await stop(app);assert.equal(app.shutdown.forced,false);app=null;await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,result:captured.result.result},null,2));await writeFile('.qa/t023/settings-save-fault-latest.json',JSON.stringify({output,passed:true}));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack,body:page&&!page.isClosed()?await page.locator('body').innerText():null,calls:app?await rpc(app.page,'calls.list',{method:'settings.commit',limit:10}).catch(()=>null):null},null,2));throw e;}finally{if(page&&!page.isClosed()){await rpc(page,'ui.settings.close.request',{reason:'close'}).catch(()=>{});await page.getByRole('button',{name:'放弃更改',exact:true}).click({timeout:1000}).catch(()=>{});}await stop(app);}
async function expectDisabled(locator){assert.equal(await locator.isDisabled(),true);}
