import {readFile,mkdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';

export async function filesHarness(name,basePort=9661) {
  const output=resolve('.qa/t008',name);await mkdir(output,{recursive:true});
  const config=JSON.parse(await readFile(process.env.SODECK_TEST_REMOTE_CONFIG??'.qa/t008/linux/test-config.json','utf8'));
  process.env.SODECK_PLUGIN_PACKAGES=[resolve('.qa/t008/baseline/terminal-0.4.0.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
  process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':basePort+1,'sodeck.terminal':basePort+2});
  const browsers=new Map();let app;
  return {
    output,config,get app(){return app;},
    async start(options={}){app=await launch({data:join(output,'data-'+Date.now()),shellPort:basePort,pluginPort:basePort+1,...options});return app;},
    async stop(){await stop(app);for(const browser of browsers.values())await browser.close().catch(()=>{});browsers.clear();app=undefined;},
    async view(owner,workspace){return until(async()=>{let browser=browsers.get(owner);if(!browser?.isConnected()){browser=await chromium.connectOverCDP('http://127.0.0.1:'+(owner==='sodeck.files'?basePort+1:basePort+2));browsers.set(owner,browser);}for(const page of browser.contexts().flatMap(c=>c.pages()))if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===workspace)return page;},25000);},
    async create(name,username=config.username){const server=await rpc(app.page,'servers.create',{name,host:config.host,port:config.port,username,authType:'key'});await rpc(app.page,'auth.set',{serverId:server.id,privateKeyPath:config.privateKeyPath,remember:true});try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){if(e.code!=='HOST_KEY_REQUIRED')throw e;await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}return server;}
  };
}
export const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
export const snapshot=page=>invoke(page,'files.snapshot',{count:300});
export async function locate(page,text){await page.getByRole('textbox',{name:'远端绝对路径'}).fill(text);await page.getByRole('button',{name:'定位路径',exact:true}).click();return until(async()=>{const s=await snapshot(page);return s.path===text&&!s.loading&&s.details&&!s.details.loading?s:false;},25000);}
