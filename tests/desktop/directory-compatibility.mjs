import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile,copyFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t010/compatibility');await mkdir(output,{recursive:true});
const cfg=JSON.parse((await readFile('.qa/t010/linux/test-config.json','utf8')).replace(/^\uFEFF/,''));assert.equal(cfg.port,22281);
const oldTerm=resolve('.qa/t008/baseline/terminal-0.4.0.sodeck-plugin'),oldFiles=resolve('.local/dev/SoDeck-v2-T008/resources/files.sodeck-plugin');
const newTerm=resolve('resources/terminal.sodeck-plugin'),newFiles=resolve('resources/files.sodeck-plugin');
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9782,'sodeck.terminal':9783});
const results=[];let app,browsers=[];
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const pageFor=async(port,view)=>{let b=browsers.find(b=>b.port===port)?.browser;if(!b?.isConnected()){b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));browsers.push({port,browser:b});}return until(async()=>{for(const p of b.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&p.url().includes('sodeck-plugin')&&(!view||await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===view))return p;});};
const close=async()=>{if(app)await stop(app);app=undefined;for(const b of browsers)await b.browser.close().catch(()=>{});browsers=[];};
async function start(data,terminal,files,extra={}){
 process.env.SODECK_PLUGIN_PACKAGES=[terminal,files].join(delimiter);
 app=await launch({data,shellPort:9781,pluginPort:9782,packagePath:terminal,...extra});return app;
}
async function server(){
 const s=await rpc(app.page,'servers.create',{name:'T010 compatibility',host:cfg.host,port:cfg.port,username:cfg.username,authType:'key'});
 await rpc(app.page,'auth.ssh.set',{serverId:s.id,privateKeyPath:cfg.privateKeyPath,remember:true});
 try{await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:s.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
 return s;
}
async function open(s){
 await rpc(app.page,'workspaces.open',{serverId:s.id});const f=await pageFor(9782),t=await pageFor(9783);await t.locator('.xterm-helper-textarea').waitFor();
 await until(async()=>['ready','partialError'].includes((await invoke(f,'files.snapshot')).state),30000);return {f,t};
}
try{
 for(const [name,terminal,files]of [['old-old',oldTerm,oldFiles],['old-new',oldTerm,newFiles],['new-old',newTerm,oldFiles]]){
  await start(join(output,name+'-'+Date.now()),terminal,files);const s=await server();const {f,t}=await open(s);
  await invoke(f,'files.locate',{text:'/usr'});await until(async()=>(await invoke(f,'files.snapshot')).path==='/usr');
  await t.locator('.xterm-helper-textarea').pressSequentially('cd /tmp\r');await delay(1500);assert.equal((await invoke(f,'files.snapshot')).path,'/usr');
  results.push(name+': ordinary terminal/manual file browsing intact; no unsupported automatic following');await close();
 }
 const data=join(output,'upgrade-'+Date.now());await start(data,oldTerm,oldFiles);const s=await server();await open(s);
 await rpc(app.page,'plugins.prepareUpdate',{path:newTerm});await rpc(app.page,'plugins.prepareUpdate',{path:newFiles});
 let plugins=await rpc(app.page,'plugins.list');assert.equal(plugins.find(p=>p.pluginId==='sodeck.terminal').activeVersion,'0.4.0');assert.equal(plugins.find(p=>p.pluginId==='sodeck.files').activeVersion,'0.1.0');await close();
 await start(data,oldTerm,oldFiles);assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);let {f,t}=await open(s);
 await until(async()=>(await invoke(f,'files.snapshot')).path==='/home/'+cfg.username,30000);
 assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files').activeVersion,'0.2.0');
 await invoke(t,'settings.shellIntegration',{enabled:false});await close();await start(data,oldTerm,oldFiles);({f,t}=await open(s));
 assert.equal((await invoke(t,'settings.get')).shellIntegration,false);await until(async()=>(await invoke(f,'files.snapshot')).follow.status==='unsupported');
 results.push('signed upgrades activate only after restart; new startup is empty; shell integration preference survives restart');await close();
 // Copy the historical executable to an isolated path; never operate on the user's running T008 instance.
 const executable=join(output,'old-core.exe');await copyFile('.local/dev/SoDeck-v2-T008/sodeck-desktop.exe',executable);
 await start(join(output,'old-core-data-'+Date.now()),oldTerm,oldFiles,{executable});
 for(const candidate of [newTerm,newFiles])await assert.rejects(rpc(app.page,'plugins.prepareUpdate',{path:candidate}),e=>e.code==='PACKAGE_INVALID'&&/API|版本|能力|compatib|unknown field `contextProviders`/i.test(e.message));
 assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.terminal').activeVersion,'0.4.0');
 results.push('old API 4.1 core rejects new terminal metadata/contextProviders and new files API before activation; old versions remain usable');await close();
}catch(error){if(app)await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),output:app.output(),workbench:await rpc(app.page,'workbench.snapshot').catch(()=>null),plugins:await rpc(app.page,'plugins.list').catch(()=>null)},null,2));throw error;}
finally{await close();await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));}
