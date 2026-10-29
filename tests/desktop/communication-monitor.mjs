import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {launch,rpc,until,stop} from './harness.mjs';

const output=resolve('.qa/t019/native-monitor');await mkdir(output,{recursive:true});
const fixture=JSON.parse(await readFile('.qa/t019/config.json','utf8')).nodes.find(n=>n.platform==='linux');
assert.equal(fixture.host,'127.0.0.1');assert.equal(fixture.port,22331);
const checks=[];let app;
const note=value=>{checks.push(value);console.log(value);};
const monitorPage=()=>until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed())){
  if(await p.evaluate(()=>Boolean(window.__SODECK_MONITOR_CONTEXT__)).catch(()=>false)){await p.waitForSelector('.communication-monitor');return p;}
}});
try {
  app=await launch({data:join(output,'data-'+Date.now()),shellPort:9811,pluginPort:9812});
  const before=await rpc(app.page,'workbench.snapshot');
  const server=await rpc(app.page,'servers.create',{name:'T019 隔离 Linux',host:fixture.host,port:fixture.port,username:fixture.username,authType:'key'});
  await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:fixture.key});
  const request={serverId:server.id,workspaceId:'t019-meter',operationId:crypto.randomUUID()};
  try{await rpc(app.page,'connections.acquire',request);assert.fail('Expected isolated host-key challenge');}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,fingerprint:e.details.fingerprint});}
  const lease=await rpc(app.page,'connections.acquire',{...request,operationId:crypto.randomUUID()});
  const pty=await rpc(app.page,'pty.open',{leaseId:lease.leaseId,cols:80,rows:24});
  await rpc(app.page,'pty.write',{ptyId:pty.ptyId,generation:pty.generation,inputSeq:1,bytes:Buffer.from("printf 'T019 monitor real SSH\n'\n").toString('base64')});
  await until(async()=>BigInt((await rpc(app.page,'communication.summary')).receivedBytes)>0n);
  const initial=await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID()});
  let monitor=await monitorPage();await monitor.getByRole('heading',{name:'通信监控 · 全部服务器',exact:true}).waitFor();
  assert.equal(await monitor.getByRole('alert').count(),0);
  const target=(await rpc(app.page,'communication.summary')).target;
  const selected=await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID(),target:{target:{...target,kind:'server',serverId:server.id},layer:'channel'}});
  assert.equal(initial.generation,selected.generation);
  await monitor.getByRole('heading',{name:'通信监控 · T019 隔离 Linux',exact:true}).waitFor();
  assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,before.workspaces.length);
  note('Native singleton navigation uses the same generation and creates no workspace or terminal');
  const a=await rpc(app.page,'communication.connections',{limit:1}),b=await rpc(monitor,'communication.connections',{limit:1});
  assert.notEqual(a.snapshotId,b.snapshotId);
  const expected=await rpc(monitor,'communication.summary');assert.ok(BigInt(expected.sentBytes)>0n&&BigInt(expected.receivedBytes)>0n);
  const theme=await rpc(app.page,'ui.theme.catalog.get');await rpc(app.page,'ui.theme.choose',{themeId:'soft-dark',expectedRevision:theme.revision});
  await monitor.waitForFunction(()=>document.documentElement.dataset.theme==='soft-dark');
  await monitor.screenshot({path:join(output,'real-ssh-monitor.png')});
  note('Real SSH counters, separate window query snapshots and live native theme subscription are available');
  const generations=new Set([initial.generation]);
  for(let i=0;i<100;i++){
    await rpc(monitor,'windows.close').catch(e=>{if(!monitor.isClosed())throw e;});
    await until(()=>monitor.isClosed());
    const context=await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID()});
    assert.ok(!generations.has(context.generation));generations.add(context.generation);
    monitor=await monitorPage();await monitor.waitForFunction(()=>document.querySelector('.sd-metric-pair'));
    if(i%20===19)console.log(`Native monitor reopen ${i+1}/100`);
  }
  await rpc(monitor,'windows.close').catch(e=>{if(!monitor.isClosed())throw e;});await until(()=>monitor.isClosed());
  await until(async()=>!(await rpc(app.page,'diagnostics.get')).communication.contracts['communication.summary.v1']);
  await rpc(app.page,'pty.write',{ptyId:pty.ptyId,generation:pty.generation,inputSeq:2,bytes:Buffer.from("printf 'business survived monitor close\n'\n").toString('base64')});
  await until(async()=>BigInt((await rpc(app.page,'communication.summary')).sentBytes)>BigInt(expected.sentBytes));
  assert.equal(app.errors.length,0,app.errors.join('\n'));
  note('100 native close/reopen cycles isolate generations, remove subscriptions and preserve the SSH session');
  await rpc(app.page,'pty.close',{ptyId:pty.ptyId,generation:pty.generation});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});
  await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,generations:generations.size,data:app.data},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}
finally {await stop(app);}
