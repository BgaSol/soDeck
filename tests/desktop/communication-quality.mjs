import assert from 'node:assert/strict';
import {readFile,mkdir,mkdtemp,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {launch,rpc,until,stop,delay} from './harness.mjs';

const output=resolve('.qa/t019/native-quality');await mkdir(output,{recursive:true});
const data=await mkdtemp(join(output,'data-'));
const fixture=JSON.parse(await readFile('.qa/t019/config.json','utf8')).nodes.find(n=>n.platform==='linux');
assert.equal(fixture.host,'127.0.0.1');assert.equal(fixture.port,22331);
let app;const checks=[];const note=text=>{checks.push(text);console.log(text);};
const monitorPage=()=>until(async()=>{for(const page of app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()))if(await page.evaluate(()=>Boolean(window.__SODECK_MONITOR_CONTEXT__)).catch(()=>false))return page;});
try {
 app=await launch({data,shellPort:9393,pluginPort:9394});
 const server=await rpc(app.page,'servers.create',{name:'诊断验证 · Linux',host:fixture.host,port:fixture.port,username:fixture.username,authType:'key'});
 await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:fixture.key});
 const acquire=()=>({serverId:server.id,workspaceId:'t019-quality',operationId:crypto.randomUUID()});
 try{await rpc(app.page,'connections.acquire',acquire());assert.fail('Expected host-key challenge');}catch(error){assert.equal(error.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,fingerprint:error.details.fingerprint});}
 const lease=await rpc(app.page,'connections.acquire',acquire()),pty=await rpc(app.page,'pty.open',{leaseId:lease.leaseId,cols:80,rows:24});
 const {configEpoch}=await rpc(app.page,'communication.summary'),target={serverId:server.id,configEpoch};
 assert.equal((await rpc(app.page,'quality.policy.get',target)).enabled,false);
 await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID(),target:{target:{kind:'server',...target},layer:'channel'}});
 let monitor=await monitorPage();await monitor.getByRole('tab',{name:'诊断',exact:true}).click();await monitor.getByText('尚无检测记录。主动探测默认关闭；未检测不代表连接异常。').waitFor();
 await monitor.getByRole('button',{name:'检查响应',exact:true}).click();await monitor.getByRole('status').filter({hasText:'已收到 SSH 响应'}).waitFor();
 await until(async()=>{const value=await rpc(monitor,'quality.summary',target);return value.segments[0]?.statistics.successes>=1;});
 assert.equal(await monitor.getByRole('button',{name:'已知悉',exact:true}).count(),0);assert.equal(await monitor.getByRole('region',{name:'连接问题',exact:true}).count(),0);
 let summary=await rpc(monitor,'quality.summary',target);assert.equal(summary.segments[0].statistics.p95Ms,null);
 const events=await rpc(monitor,'events.query',{...target,kind:'domain',fromUtc:Date.now()-60000,toUtc:Date.now()+1000});
 assert.ok(events.items.some(e=>e.type==='quality.response.sample'));assert.ok(events.items.some(e=>e.type==='connections.route.stage'));
 const health=await rpc(monitor,'observability.health.get');assert.equal(health.state,'active');assert.equal(health.partitions.reduce((n,p)=>n+p.budgetBytes,0),512*1024*1024);
 await monitor.locator('.monitor-content').evaluate(e=>e.scrollTop=0);
 await monitor.screenshot({path:join(output,'real-ssh-diagnostics.png')});
 note('Native diagnostic page measured a real existing SSH connection and reported insufficient percentile samples');
 await monitor.getByLabel('持续探测：每 5 秒发送少量 SSH 请求').click();
 await until(async()=>monitor.getByLabel('持续探测：每 5 秒发送少量 SSH 请求').isChecked());
 await until(async()=>{summary=await rpc(monitor,'quality.summary',target);return summary.segments[0].statistics.successes>=2;},10000);
 const before=summary.segments[0].statistics.successes;
 await rpc(monitor,'windows.close').catch(error=>{if(!monitor.isClosed())throw error;});await until(()=>monitor.isClosed());
 await until(async()=>{summary=await rpc(app.page,'quality.summary',target);return summary.segments[0].statistics.successes>before;},10000);
 assert.equal((await rpc(app.page,'resources.list')).connections.length,1);
 await rpc(app.page,'pty.write',{ptyId:pty.ptyId,generation:pty.generation,inputSeq:1,bytes:Buffer.from('echo T019-native-quality-alive\n').toString('base64')});
 note('Closing the monitor released its subscription while explicitly enabled probes and the business PTY stayed usable');
 await rpc(app.page,'ui.communicationMonitor.open',{operationId:crypto.randomUUID(),target:{target:{kind:'server',...target},layer:'channel'}});
 monitor=await monitorPage();await monitor.getByRole('tab',{name:'诊断',exact:true}).click();await monitor.getByLabel('持续探测：每 5 秒发送少量 SSH 请求').waitFor();
 assert.equal(await monitor.getByLabel('持续探测：每 5 秒发送少量 SSH 请求').isChecked(),true);assert.equal(await monitor.getByRole('alert').count(),0);
 await rpc(app.page,'pty.close',{ptyId:pty.ptyId,generation:pty.generation});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});
 await until(async()=>{const value=await rpc(app.page,'quality.summary',target);return value.activeProbes===0&&value.segments.every(s=>s.state==='ended');});
 await stop(app);assert.equal(app.shutdown.forced,false);app=await launch({data,shellPort:9393,pluginPort:9394});
 assert.equal((await rpc(app.page,'quality.policy.get',target)).enabled,true);assert.equal((await rpc(app.page,'resources.list')).connections.length,0);
 await delay(1000);assert.equal((await rpc(app.page,'quality.summary',target)).activeProbes,0);
 note('Restart kept the explicit probe preference, with no restored SSH connection and no automatic remote request');
 await writeFile(join(output,'results.json'),JSON.stringify({checks,summary,data},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({message:error.message,stack:error.stack,checks,data},null,2));throw error;}
finally{if(app)await stop(app);}
