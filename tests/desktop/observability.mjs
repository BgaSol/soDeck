import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile,mkdtemp,rename} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {pathToFileURL} from 'node:url';
import {build} from 'esbuild';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t016/native');await mkdir(output,{recursive:true});const data=await mkdtemp(join(output,'data-'));
await build({entryPoints:['tests/desktop/observation-consumer.ts'],bundle:true,format:'esm',platform:'node',outfile:join(output,'consumer.mjs')});
const consumer=await import(pathToFileURL(join(output,'consumer.mjs')));
const fixture=JSON.parse(await readFile('.qa/t016/config.json','utf8'));
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
const ports={'sodeck.terminal':9452,'sodeck.files':9453,'sodeck.network-proxy':9454};process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(ports);
let app;const browsers=new Map(),checks=[],servers=[];const note=text=>{checks.push(text);console.log(text);};
async function pageFor(plugin,workspace,viewId){return until(async()=>{let browser=browsers.get(plugin);if(!browser?.isConnected()){browser=await chromium.connectOverCDP(`http://127.0.0.1:${ports[plugin]}`);browsers.set(plugin,browser);}for(const page of browser.contexts().flatMap(c=>c.pages()))if(!page.isClosed()&&await page.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId).catch(()=>null)===workspace&&(!viewId||await page.evaluate(()=>!window.__SODECK_CONTEXT__?.dialog&&window.__SODECK_CONTEXT__?.viewId)===viewId))return page;},30000);}
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function trust(server,node){await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:node.key});for(let n=0;n<3;n++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});return;}catch(error){assert.equal(error.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...error.details});}}
try{
 app=await launch({data,shellPort:9451,pluginPort:9452});
 const info=await rpc(app.page,'app.info');assert.equal(info.version,'2.0.0-dev.16');const description=await rpc(app.page,'observability.describe');assert.equal(description.capabilities.length,7);assert.equal(description.sources.length,3);assert.ok(description.sources.every(s=>s.coverage==='full'));
 for(const node of fixture.nodes){const server=await rpc(app.page,'servers.create',{name:`OBS-${node.platform}`,host:node.host,port:node.port,username:node.username,authType:'key'});servers.push(server);await trust(server,node);await rpc(app.page,'workspaces.open',{serverId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.some(p=>p.serverId===server.id),45000);}
 note('Both isolated Windows and Linux OpenSSH workspaces opened real terminals with signed 4.10 plugins');
 const linux=servers[fixture.nodes.findIndex(n=>n.platform==='linux')];await rpc(app.page,'workspaces.open',{serverId:linux.id});
 const files=await pageFor('sodeck.files',linux.id);await until(async()=>{const s=await invoke(files,'files.snapshot',{count:50});return ['ready','partialError'].includes(s.state);},45000);
 await invoke(files,'files.refresh');await delay(1500);
 const opened=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:linux.id,operationId:crypto.randomUUID()});const proxy=await pageFor('sodeck.network-proxy',linux.id,opened.viewId);await proxy.getByRole('button',{name:'创建',exact:true}).waitFor();
 await until(()=>invoke(proxy,'network.snapshot'),30000);
 const rule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'Observation fixture',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}});const view=await proxy.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:view,operationId:crypto.randomUUID(),args:{id:rule.id}});
 const failed=await invoke(proxy,'network.check',{id:rule.id,host:'127.0.0.1',port:1,protocol:'socks'});assert.equal(failed.state,'failed');
 await delay(11000);await invoke(proxy,'network.stop',{id:rule.id});
 const filter={fromUtc:Date.now()-300000,toUtc:Date.now()+1000};const transport={call:(method,params)=>rpc(app.page,method,params)};
 const read=await consumer.readEvidence(transport,filter);
 for(const plugin of Object.keys(ports))assert.ok(read.events.some(e=>e.source.pluginId===plugin&&e.type.startsWith('plugin.')),`Missing ${plugin} domain observations`);
 const checked=read.events.find(e=>e.type==='plugin.sodeck.network-proxy.check.completed');assert.equal(checked.outcome,'failed');
 const terminal=read.events.find(e=>e.type==='plugin.sodeck.terminal.session.open.completed'&&e.outcome==='succeeded');assert.ok(terminal.traceId);assert.ok(terminal.targets.some(t=>t.kind==='resource'));
 const trace=await rpc(app.page,'traces.get',{traceId:terminal.traceId});assert.ok(trace.items.some(e=>e.kind==='call'));assert.ok(trace.items.some(e=>e.initiator.assurance==='verified'));
 assert.ok(read.snapshots.items.length>=3);assert.ok(read.metrics.items.length>0);note('Public SDK reads all plugin events, linked terminal trace, snapshots and minute metrics; fulfilled failed check remains failed');
 for(const server of servers)await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===0);
 await until(async()=>['succeeded','partial'].includes((await rpc(app.page,'evidence.get',{jobId:read.jobId})).job.state));const saved=await consumer.readPackage(transport,read.jobId);const records=saved.content.trim().split('\n').map(JSON.parse),manifest=records.at(-1);assert.equal(manifest.recordType,'manifest');const prefix=saved.content.slice(0,saved.content.lastIndexOf('{"recordType":"manifest"'));assert.equal(manifest.value.contentSha256,createHash('sha256').update(prefix).digest('hex'));assert.equal(manifest.value.bytes,Buffer.byteLength(saved.content));
 for(const node of fixture.nodes)assert.ok(!saved.content.includes(node.key));assert.ok(!saved.content.includes('Observation fixture'));
 await writeFile(join(output,'evidence.jsonl'),saved.content);await app.page.screenshot({path:join(output,'native.png')});note('Self-contained package remains readable after views close, checksum and size match, and private key paths/rule text are absent');
 await stop(app);assert.equal(app.shutdown.forced,false);app=undefined;
 await rename(join(data,'plugins'),join(data,'plugins-producers-ended'));
 process.env.SODECK_PLUGIN_PACKAGES=join(output,'intentionally-unavailable.sodeck-plugin');
 app=await launch({data,shellPort:9451,pluginPort:9452,allowStartupWarning:true});
 assert.equal((await rpc(app.page,'observability.describe')).sources.length,0);
 const historical=await consumer.readEvidence(transport,filter);
 for(const plugin of Object.keys(ports))assert.ok(historical.events.some(e=>e.source.pluginId===plugin&&e.type.startsWith('plugin.')));
 assert.ok(historical.snapshots.items.length>=3);assert.ok(historical.metrics.items.length>0);
 assert.equal((await consumer.readPackage(transport,read.jobId)).content,saved.content);
 note('After normal app restart with zero producers, public SDK still reads historical plugin events, descriptors, snapshots, metrics and the identical package');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,data,checks,events:read.events.length,snapshots:read.snapshots.items.length,metrics:read.metrics.items.length,package:manifest.value},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,data,error:String(error),code:error.code,stack:error.stack},null,2));throw error;}finally{await stop(app);for(const browser of browsers.values())await browser.close().catch(()=>{});}
