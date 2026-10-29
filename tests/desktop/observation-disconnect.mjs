import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,mkdtemp} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t016/disconnect');await mkdir(output,{recursive:true});const data=await mkdtemp(join(output,'data-'));
const target=JSON.parse(await readFile('.qa/t016/config.json','utf8')).nodes.find(n=>n.platform==='linux');
const ports={'sodeck.terminal':9632,'sodeck.files':9633,'sodeck.network-proxy':9634},browsers=new Map(),checks=[];let app;
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(ports);
const invoke=(p,command,args={})=>pluginRpc(p,'plugin.invoke',{command,args});const note=text=>{checks.push(text);console.log(text);};
async function pageFor(plugin,workspace){return until(async()=>{let b=browsers.get(plugin);if(!b?.isConnected()){b=await chromium.connectOverCDP(`http://127.0.0.1:${ports[plugin]}`);browsers.set(plugin,b);}for(const p of b.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>!window.__SODECK_CONTEXT__?.dialog&&window.__SODECK_CONTEXT__?.workspaceId).catch(()=>null)===workspace)return p;},30000);}
async function events(type){const filter={type,fromUtc:Date.now()-600000,toUtc:Date.now()+1000};let cursor,items=[];do{const p=await rpc(app.page,'events.v2.query',{filter,limit:200,...(cursor?{cursor}:{})});items.push(...p.items);cursor=p.nextCursor;}while(cursor);return items;}
try{
 app=await launch({data,shellPort:9631,pluginPort:9632});const policy=await rpc(app.page,'observability.v2.policy.get');await rpc(app.page,'observability.v2.policy.set',{expectedRevision:policy.revision,patch:{logLevel:'debug',debugUntil:Date.now()+300000}});
 const server=await rpc(app.page,'servers.create',{name:'T016 disconnect isolated',host:target.host,port:target.port,username:target.username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:target.key});
 for(let i=0;i<3;i++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});break;}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...e.details});}
 await rpc(app.page,'workspaces.open',{serverId:server.id});const terminal=await pageFor('sodeck.terminal',server.id),files=await pageFor('sodeck.files',server.id);
 let session=await until(async()=>(await invoke(terminal,'sessions.list',{workspaceId:server.id})).find(s=>['running','active'].includes(s.status)),45000);await until(async()=>(await invoke(files,'files.snapshot')).state==='ready',45000);
 await until(async()=>(await events('plugin.sodeck.terminal.integration.state.changed')).some(e=>e.attributes.state==='ready'),15000);
 const before=(await events('plugin.sodeck.terminal.integration.state.changed')).length;
 for(const command of ['','cd /tmp','','cd /','']){await terminal.locator('.xterm-helper-textarea').fill(command);await terminal.locator('.xterm-helper-textarea').press('Enter');await delay(350);}
 await delay(1000);assert.equal((await events('plugin.sodeck.terminal.integration.state.changed')).length,before);
 note('Repeated shell prompts and cd keep ready directory integration without repeated health-state events in debug mode');
 await terminal.locator('.xterm-helper-textarea').fill('exit');await terminal.locator('.xterm-helper-textarea').press('Enter');
 await until(async()=>(await invoke(terminal,'sessions.list',{workspaceId:server.id})).some(s=>s.sessionId===session.sessionId&&['ended','closed','disconnected'].includes(s.status)),15000);
 const ended=await until(async()=>(await events('plugin.sodeck.terminal.session.state.changed')).find(e=>e.attributes.sessionId===session.sessionId&&['ended','closed','disconnected'].includes(e.attributes.to)));
 assert.ok(['core-response','ui-reported'].includes(ended.attributes.observationBasis));assert.equal(ended.attributes.exitCode,undefined);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.reconnect',viewId:session.viewId,operationId:crypto.randomUUID()});
 session=await until(async()=>(await invoke(terminal,'sessions.list',{workspaceId:server.id})).find(s=>['running','active'].includes(s.status)),45000);
 note('Real remote shell exit is observed without inventing an exit code; explicit reconnect preserves the logical session');
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});const proxy=await pageFor('sodeck.network-proxy',server.id);
 await until(()=>invoke(proxy,'network.snapshot'));
 const rule=await invoke(proxy,'network.save',{expectedRevision:0,rule:{name:'isolated proxy',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}}),proxyView=await proxy.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:proxyView,operationId:crypto.randomUUID(),args:{id:rule.id}});
 const resources=await rpc(app.page,'resources.list'),pty=resources.ptys.find(p=>p.id===session.ptyId);assert.ok(pty);
 const leases=resources.leases.filter(l=>l.connectionId===pty.connectionId);assert.ok(leases.length>=2,'terminal and files hold shared connection leases');
 const beforeEnds=new Set((await events('core.connection.ended')).map(e=>e.eventId));
 await rpc(app.page,'resources.stop',{resourceId:pty.connectionId});await invoke(files,'files.refresh');
 await until(async()=>(await invoke(files,'files.snapshot')).state==='disconnected');await until(async()=>(await invoke(terminal,'sessions.list',{workspaceId:server.id})).find(s=>s.sessionId===session.sessionId)?.status==='ended');
 const ends=await until(async()=>{const values=(await events('core.connection.ended')).filter(e=>!beforeEnds.has(e.eventId));return values.length&&values;});assert.equal(ends.length,1);
 const connectionRef=ends[0].attributes.resourceRef;
 const termImpact=(await events('plugin.sodeck.terminal.session.state.changed')).find(e=>e.attributes.sessionId===session.sessionId&&e.attributes.to==='ended'&&e.targets.some(t=>t.ref===connectionRef));assert.ok(termImpact);
 const fileImpact=await until(async()=>(await events('plugin.sodeck.files.directory.operation.completed')).find(e=>e.attributes.operation==='refresh'&&e.targets.some(t=>t.ref===connectionRef)));
 assert.notEqual(fileImpact.outcome,'succeeded','disconnected refresh must not claim success');
 const proxyRun=(await invoke(proxy,'network.snapshot',{selected:rule.id})).selected.run;assert.equal(proxyRun.status,'ready');
 note('Stopping the actual shared terminal/files connection records one core end and resource-linked impacts; the separate proxy transport remains ready');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,data,checks,ends,termImpact,fileImpact,proxyRunId:proxyRun.id},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,data,checks,events:await events('plugin.sodeck.files.directory.operation.completed').catch(()=>[])},null,2));throw e;}
finally{await stop(app);for(const browser of browsers.values())await browser.close().catch(()=>{});}
