import assert from 'node:assert/strict';
import {mkdir,mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';
const output=resolve('.qa/t016/compatibility');await mkdir(output,{recursive:true});
const names=['terminal','files','network-proxy'],versions=['0.8.0','0.6.0','0.2.2'];
const fixture=JSON.parse(await readFile('.qa/t016/config.json','utf8')).nodes.find(n=>n.platform==='linux');
const hash=async path=>createHash('sha256').update(await readFile(path)).digest('hex');
const coreHash=await hash('target/debug/sodeck-desktop.exe'),checks=[];
for(const [round,upgraded] of [null,...names].entries()){
 const packages=names.map((name,i)=>resolve(`resources/${upgraded===name?name:`sodeck.${name}-${versions[i]}`}.sodeck-plugin`));
 process.env.SODECK_PLUGIN_PACKAGES=packages.join(delimiter);
 const port=9501+round*10;
 process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':port+1,'sodeck.files':port+2,'sodeck.network-proxy':port+3});
 const data=await mkdtemp(join(output,'data-'));let app,proxyBrowser;
 try{
  app=await launch({data,shellPort:port,pluginPort:port+1});
  const description=await rpc(app.page,'observability.describe');assert.equal(description.sources.length,3);
  for(const name of names)assert.equal(description.sources.find(s=>s.pluginId===`sodeck.${name}`).coverage,name===upgraded?'full':'audit-only');
  const server=await rpc(app.page,'servers.create',{name:'Compatibility fixture',host:fixture.host,port:fixture.port,username:fixture.username,authType:'key'});
  await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:fixture.key});
  let lease;try{lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(error){assert.equal(error.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...error.details});lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}
  await rpc(app.page,'connections.release',{leaseId:lease.leaseId});await rpc(app.page,'workspaces.open',{serverId:server.id});
  await until(async()=>(await rpc(app.page,'resources.list')).ptys.length>0,45000);
  const view=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});assert.ok(view.viewId);
  if(upgraded==='network-proxy'){
   proxyBrowser=await until(()=>chromium.connectOverCDP(`http://127.0.0.1:${port+3}`),30000);
   const page=await until(async()=>{for(const page of proxyBrowser.contexts().flatMap(c=>c.pages()))if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewId).catch(()=>null)===view.viewId)return page;});
   // Opening an empty manager creates no rule/run fact. Exercise an actual rule change.
   await until(()=>pluginRpc(page,'plugin.invoke',{command:'network.snapshot',args:{}}));
   await pluginRpc(page,'plugin.invoke',{command:'network.save',args:{expectedRevision:0,rule:{name:'Compatibility test',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}}});
  }
  await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===0);
  if(upgraded)await until(async()=>(await rpc(app.page,'events.v2.query',{filter:{pluginId:`sodeck.${upgraded}`,kind:'domain'},limit:10})).items.length>0);
  assert.equal(await hash('target/debug/sodeck-desktop.exe'),coreHash);
  checks.push({upgraded,data,sources:description.sources,packages:await Promise.all(packages.map(async path=>({path,sha256:await hash(path)}))),passed:true});
  console.log(`${upgraded??'all legacy'}: signed unchanged packages, expected coverage, real SSH workspace and resource close passed`);
 }finally{await stop(app);await proxyBrowser?.close().catch(()=>{});}
}
await writeFile(join(output,'results.json'),JSON.stringify({passed:true,coreHash,checks},null,2));
