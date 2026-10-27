import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';

const output=resolve('.local/test-results/t017-r2/ui-upgrade-'+Date.now());await mkdir(output,{recursive:true});
const baseline=resolve('.local/dev/SoDeck-v2-T017/resources/network-proxy.sodeck-plugin'),candidate=resolve('resources/network-proxy.sodeck-plugin');
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9896});
let app,browser;const checks=[];
const start=async path=>{process.env.SODECK_PLUGIN_PACKAGES=path;app=await launch({data:join(output,'data'),shellPort:9895,pluginPort:9896,packagePath:path,allowStartupWarning:true});};
const version=async()=>(await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.network-proxy');
const open=async serverId=>{
  await rpc(app.page,'workspaces.open',{serverId});
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:serverId,operationId:crypto.randomUUID()});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9896'));
  const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin.localhost')));
  await page.getByRole('button',{name:'创建',exact:true}).waitFor();return page;
};
try {
  await start(baseline);assert.equal((await version()).activeVersion,'0.2.1');
  // No SSH connection is started; this tests saved configuration and package activation only.
  const server=await rpc(app.page,'servers.routes.v2.create',{name:'隔离更新验证',host:'127.0.0.1',port:1,username:'test',connectionRoute:{kind:'direct'}});
  let page=await open(server.id);
  const rule=await pluginRpc(page,'plugin.invoke',{command:'network.save',args:{expectedRevision:0,rule:{name:'保留原有规则',type:'network-proxy',bindMode:'fixed',port:10808,autoReconnect:false}}});
  await rpc(app.page,'plugins.prepareUpdate',{path:candidate});
  assert.equal((await version()).activeVersion,'0.2.1');assert.equal((await version()).pendingVersion,'0.2.2');
  checks.push('Preparing 0.2.2 keeps 0.2.1 active until restart');
  await stop(app);app=undefined;await browser.close();browser=undefined;
  await start(candidate);assert.equal((await version()).activeVersion,'0.2.2');
  assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);
  page=await open(server.id);
  await page.getByRole('button',{name:'保留原有规则',exact:true}).waitFor();
  const snapshot=await pluginRpc(page,'plugin.invoke',{command:'network.snapshot',args:{}});
  assert.equal(snapshot.total,1);assert.equal(snapshot.items[0].rule.id,rule.id);assert.equal(snapshot.items[0].rule.port,10808);assert.equal(snapshot.items[0].run,undefined);
  assert.equal(await page.locator('.sd-resource-item').count(),1);
  await page.screenshot({path:join(output,'upgraded-list.png')});
  checks.push('Restart activates redesigned 0.2.2 UI; saved rule identity and fixed port survive without auto-start');
  console.log(checks.join('\n'));
}finally {
  await stop(app);await browser?.close().catch(()=>{});
  await writeFile(join(output,'results.json'),JSON.stringify({checks},null,2));
  await writeFile('.local/test-results/t017-r2/ui-upgrade-latest.json',JSON.stringify({output,checks},null,2));
}
