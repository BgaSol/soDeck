import assert from 'node:assert/strict';
import {cp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {launch,rpc,until,stop} from './harness.mjs';
const first=resolve(process.argv[2]),output=resolve('.qa/t023/package-smoke',String(Date.now()));await mkdir(output,{recursive:true});const checks=[];let app,page;
async function run(folder,port,expected){
  app=await launch({ordinary:true,executable:join(folder,'sodeck-desktop.exe'),data:join(folder,'data'),cwd:output,shellPort:port,startupTimeout:60000});
  const info=await rpc(app.page,'app.runtimeInfo.get');assert.equal(info.mode,'portable');assert.equal(resolve(info.dataPath),join(folder,'data'));assert.equal((await rpc(app.page,'plugins.list')).length,3);
  const snapshot=(await rpc(app.page,'settings.page.get',{pageId:'plugin:sodeck.terminal:general'})).snapshot;assert.equal(snapshot.values.find(v=>v.key==='fontSize').value,expected);
  await app.page.getByRole('button',{name:'设置',exact:true}).click();page=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});await page.getByRole('button',{name:/^终端/}).click();await page.getByLabel('字号',{exact:true}).fill(String(expected===14?18:19));await page.getByRole('button',{name:'应用',exact:true}).click();await until(()=>page.getByRole('button',{name:'应用',exact:true}).isDisabled());
  await until(()=>page.getByRole('button',{name:'确定',exact:true}).isEnabled());await page.getByText('更改已应用',{exact:true}).waitFor();
  await page.screenshot({path:join(output,'release-settings-'+port+'.png')});await page.getByRole('button',{name:'确定',exact:true}).click();await until(()=>page.isClosed());await stop(app);assert.equal(app.shutdown.forced,false);assert.equal(app.process.exitCode,0);app=null;
}
try{
  await run(first,9891,14);checks.push('Release ordinary portable launch loads exactly three signed plugins with fresh data and no source-directory fallback');
  const relocated=join(output,'relocated');await cp(first,relocated,{recursive:true});await run(relocated,9893,18);checks.push('Relocated package retains saved plugin preference, resolves its new data path, and exits normally');
  const build=JSON.parse(await readFile(join(first,'build.json'),'utf8'));assert.equal(build.profile,'release');await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,build,original:first,relocated},null,2));await writeFile('.qa/t023/settings-package-latest.json',JSON.stringify({output,passed:true}));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}finally{if(page&&!page.isClosed())await page.getByRole('button',{name:'取消',exact:true}).click().catch(()=>{});await stop(app);}
