import {readFile,mkdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';
export async function monitorHarness(name,basePort=18881){
  const output=resolve('.qa/t024',name);await mkdir(output,{recursive:true});
  const config={host:'127.0.0.1',port:22342,username:'tester',privateKeyPath:resolve('.qa/t008/linux/test-key')};
  const owners=['sodeck.monitor','sodeck.terminal','sodeck.files','sodeck.network-proxy'];
  process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy','monitor'].map(p=>resolve('.qa/t024/packages',p+'.sodeck-plugin')).join(delimiter);
  process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(Object.fromEntries(owners.map((id,index)=>[id,basePort+index+1])));
  const browsers=new Map();let app;
  return {output,config,get app(){return app;},
    async start(options={}){app=await launch({data:join(output,'data-'+Date.now()),shellPort:basePort,pluginPort:basePort+1,executable:resolve(process.env.SODECK_MONITOR_RUNTIME??'.qa/t024/runtime-monitor-r7/sodeck-desktop.exe'),...options});return app;},
    async stop(){await stop(app);for(const b of browsers.values())await b.close().catch(()=>{});browsers.clear();},
    async view(owner,workspace){return until(async()=>{let browser=browsers.get(owner);if(!browser?.isConnected()){browser=await chromium.connectOverCDP('http://127.0.0.1:'+(basePort+owners.indexOf(owner)+1));browsers.set(owner,browser);}for(const page of browser.contexts().flatMap(c=>c.pages()))if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===workspace)return page;},30000);},
    async create(name,overrides={}){const server=await rpc(app.page,'servers.create',{name,...config,...overrides,privateKeyPath:undefined,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:config.privateKeyPath,remember:true});try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){if(e.code!=='HOST_KEY_REQUIRED')throw e;await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}return server;},
  };
}
export async function invoke(page,command,args={}){const viewId=await page.evaluate(()=>window.__SODECK_CONTEXT__.viewId);return pluginRpc(page,'plugin.invoke',{command,args:{...args,viewId}});}
export const snapshot=page=>invoke(page,'monitor.snapshot');
