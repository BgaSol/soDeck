import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,copyFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';
const output=resolve('.qa/t011/compatibility');await mkdir(output,{recursive:true});
const oldTerm=resolve('.local/dev/SoDeck-v2-T010/resources/terminal.sodeck-plugin'),oldFiles=resolve('.local/dev/SoDeck-v2-T010/resources/files.sodeck-plugin'),newTerm=resolve('resources/terminal.sodeck-plugin'),newFiles=resolve('resources/files.sodeck-plugin');
const checks=[];let app,browsers=[];
const close=async()=>{await stop(app);app=undefined;for(const b of browsers)await b.close().catch(()=>{});browsers=[];};
let port=9878;
async function start(options){port+=3;process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':port+1,'sodeck.terminal':port+2});return launch({...options,shellPort:port,pluginPort:port+1});}
async function page(port){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port),25000);browsers.push(b);const p=await until(()=>b.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));await p.waitForSelector('#root > *');return p;}
try{
 for(const [name,t,f] of [['old-old',oldTerm,oldFiles],['old-new',oldTerm,newFiles],['new-old',newTerm,oldFiles],['new-new',newTerm,newFiles]]){
  process.env.SODECK_PLUGIN_PACKAGES=[t,f].join(delimiter);app=await start({data:join(output,name+'-'+Date.now()),packagePath:t});
  const s=await rpc(app.page,'servers.create',{name:'兼容性 · '+name,host:'127.0.0.1',port:1,username:'fixture',authType:'password'});await rpc(app.page,'workspaces.open',{serverId:s.id});
  const fp=await page(port+1),tp=await page(port+2),p=await rpc(app.page,'ui.theme.preference.get');assert.equal(p.legacyPlugins.length,Number(t===oldTerm)+Number(f===oldFiles));
  const next=await rpc(app.page,'ui.theme.select',{mode:'light',expectedRevision:p.revision});
  for(const [view,old]of [[tp,t===oldTerm],[fp,f===oldFiles]]){const theme=await pluginRpc(view,'ui.theme.get');assert.equal(theme.schemaVersion,old?1:2);assert.equal(typeof theme.revision,old?'number':'string');if(old){assert.equal(await view.evaluate(()=>document.documentElement.dataset.theme),'soft-dark');assert.equal(theme.highContrast,undefined);}else await until(async()=>await view.evaluate(()=>document.documentElement.dataset.themeRevision)===next.revision);}
  checks.push(name+': version negotiation, v1 strict shape and mounted-old/live-new behavior');await close();
 }
 process.env.SODECK_PLUGIN_PACKAGES=[newTerm,newFiles].join(delimiter);
 for(const [name,contents] of [['corrupt-preference','{broken'],['invalid-v1',JSON.stringify({...JSON.parse(await readFile('themes/graphite.json','utf8')),colors:{}})],['invalid-v2',JSON.stringify({...JSON.parse(await readFile('themes/soft-light.json','utf8')),mode:'invalid'})]]){
  const data=join(output,name+'-'+Date.now());await mkdir(data,{recursive:true});if(name==='corrupt-preference')await writeFile(join(data,'appearance-preferences.json'),contents);else{const path=join(data,'invalid-theme.json');await writeFile(path,contents);process.env.SODECK_THEME_FILE=path;}
  app=await start({data,allowStartupWarning:true});assert.equal((await rpc(app.page,'ui.theme.get')).id,'soft-dark');assert.ok((await rpc(app.page,'app.info')).startupError);const p=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.select',{mode:'light',expectedRevision:p.revision});assert.equal(JSON.parse(await readFile(join(data,'appearance-preferences.json'),'utf8')).mode,'light');checks.push(name+': whole-palette fallback, visible diagnostic and repair');await close();delete process.env.SODECK_THEME_FILE;
 }
 const oldCore=join(output,'old-core.exe');await copyFile('.local/dev/SoDeck-v2-T010/sodeck-desktop.exe',oldCore);
 app=await start({data:join(output,'old-core-'+Date.now()),executable:oldCore,allowStartupWarning:true});const status=await rpc(app.page,'plugins.list');assert.ok(status.every(p=>!p.activeVersion));assert.match((await rpc(app.page,'app.info')).startupError,/4\.3|capabilit|能力|兼容|API/);checks.push('old core rejects new plugin requirements without mounting incompatible packages');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks},null,2));console.log(checks.join('\n'));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}finally{await close();delete process.env.SODECK_THEME_FILE;}
