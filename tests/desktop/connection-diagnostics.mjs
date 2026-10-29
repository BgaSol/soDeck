// Native WebView2 + isolated real SSH/SOCKS protocols. Never reads user data.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,unlink} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {spawn} from 'node:child_process';
import net from 'node:net';
import {createInterface} from 'node:readline';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';

const output=resolve('.qa/t027/diagnostics-native-'+Date.now()),fixture=join(output,'fixture');await mkdir(fixture,{recursive:true});
await writeFile(join(fixture,'native-shell'),'');await writeFile(join(fixture,'method-eof-2'),'0');
const python=process.env.SODECK_TEST_PYTHON;if(!python)throw Error('Set SODECK_TEST_PYTHON');
const child=spawn(python,['-u','tests/core/mixed_route_fixture.py',fixture,'ssh,ssh,socks,ssh'],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python')}});
const lines=createInterface({input:child.stdout});const config=await new Promise((resolve,reject)=>{lines.once('line',line=>resolve(JSON.parse(line)));child.once('error',reject);child.once('exit',code=>reject(Error('fixture exited '+code)));});
const names=['terminal','files','network-proxy','monitor'];process.env.SODECK_PLUGIN_PACKAGES=names.map(n=>resolve(`.qa/t027/packages/${n}.sodeck-plugin`)).join(delimiter);
const ports=Object.fromEntries(names.map((n,i)=>['sodeck.'+n,12862+i]));process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(ports);
const browsers=new Map(),checks=[];let app,failed;
const note=text=>{checks.push(text);console.log(text);};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function surface(plugin){let browser=browsers.get(plugin);if(!browser){browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+ports[plugin]));browsers.set(plugin,browser);}return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.pluginId).catch(()=>null)===plugin){await p.waitForSelector('#root > *');return p;}});}
const attempts=async(server)=>rpc(app.page,'connections.diagnostics.list',{workspaceId:server.id,serverId:server.id,purpose:'session'});
async function phases(record){const list=[];let cursor=null;do{const page=await rpc(app.page,'connections.diagnostics.phases',{reference:record.reference,revision:record.revision,cursor});list.push(...page.phases);cursor=page.nextCursor;}while(cursor);return list;}
try{
 app=await launch({data:join(output,'data'),shellPort:12861,pluginPort:12862,packagePath:resolve('.qa/t027/packages/terminal.sodeck-plugin')});const page=app.page;
 assert.ok((await rpc(page,'capabilities.list')).capabilities.includes('connections.diagnostics.v1'));
 const servers=[];
 for(const i of [0,1]){const e=config.steps[i];const s=await rpc(page,'servers.routes.create',{name:`诊断 SSH ${i+1}`,host:e.host,port:e.port,username:'fixture',authType:'password',accessPath:{steps:[]}});servers.push(s);}
 const e=config.steps[2],proxy=await rpc(page,'proxies.socks5.create',{name:'诊断回环代理',host:e.host,port:e.port,username:'proxy',authType:'usernamePassword'});
 const target=await rpc(page,'servers.routes.create',{name:'诊断目标',host:config.steps[3].host,port:22,username:'fixture',authType:'password',accessPath:{steps:[...servers.map(s=>({stepId:crypto.randomUUID(),kind:'ssh',serverId:s.id})),{stepId:crypto.randomUUID(),kind:'socks5',proxyId:proxy.id,resolve:'proxy'}]}});
 for(const s of [...servers,target]){const p=await rpc(page,'credentials.configuration.prepare',{target:{kind:'ssh',id:s.id},expectedOwnerRevision:s.revision,expectedDependencies:s.accessPath.steps.map(step=>{const owner=step.kind==='ssh'?servers.find(s=>s.id===step.serverId):proxy;return {stepId:step.stepId,kind:step.kind,id:owner.id,host:owner.host,port:owner.port,username:owner.username,authType:owner.authType};})});await rpc(page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:p.contextToken,password:'mixed-route-fixture',remember:true});}
 await rpc(page,'workspaces.open',{serverId:target.id,target:'currentWindow',operationId:crypto.randomUUID()});
 const confirmed=new Set();let proxyConfirm=0;
 async function answer(){const dialog=page.getByRole('dialog',{name:'确认身份与登录',exact:true});if(await dialog.count()){const title=await dialog.innerText();confirmed.add(title.match(/诊断 SSH \d|诊断目标/)?.[0]??title);assert.equal(await dialog.getByText('使用此连接已保存的凭据。',{exact:false}).count(),1);await dialog.getByRole('button',{name:'信任并连接',exact:true}).click();return;}
 const proxyDialog=page.getByRole('dialog',{name:'网络代理认证',exact:true});if(await proxyDialog.count()){proxyConfirm++;await proxyDialog.getByLabel('代理密码',{exact:true}).fill('mixed-proxy-fixture');await proxyDialog.getByRole('button',{name:'继续连接目标'}).click();}}
 let eof=await until(async()=>{await answer();const a=(await attempts(target)).attempts[0];return a?.state==='failed'&&a.error?.code==='PROXY_DISCONNECTED'?a:null;},60000);
 assert.equal(confirmed.size,2);assert.equal(proxyConfirm,1);assert.equal(eof.error.receivedBytes,0);assert.equal(eof.error.expectedBytes,2);
 const rows=await phases(eof);assert.ok(rows.some(p=>p.stepIndex===1&&p.stage==='authenticate'&&p.state==='succeeded'));assert.ok(rows.some(p=>p.stepIndex===2&&p.stage==='forward'&&p.state==='succeeded'));assert.ok(!rows.some(p=>p.stepIndex===3));
 note('Native combined identity/login confirmed A and B once each; proxy confirmation separate; real SSH forward succeeds before SOCKS EOF 0/2; target never runs.');
 const terminal=await surface('sodeck.terminal');await until(async()=>await terminal.getByRole('button',{name:'查看连接详情',exact:true}).count());
 const events=async()=>JSON.parse(JSON.stringify((await readFile(join(fixture,'events.jsonl'),'utf8')).trim().split('\n').map(JSON.parse)));
 const opens=(await events()).filter(e=>e.event==='open').length;
 await terminal.getByRole('button',{name:'查看连接详情',exact:true}).click();const details=page.getByRole('dialog',{name:'连接详情',exact:true});await details.waitFor();await details.getByText(/本片段收到 0 字节/).waitFor();
 assert.match(await details.innerText(),/未执行/);assert.doesNotMatch(await details.innerText(),/端口未监听|密码错误/);
 for(const themeId of ['soft-light','soft-dark','dopamine']){const pref=await rpc(page,'ui.theme.preference.get');await rpc(page,'ui.theme.choose',{themeId,expectedRevision:pref.revision});await page.screenshot({path:join(output,`details-${themeId}.png`)});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
 await details.getByRole('button',{name:'复制诊断报告'}).click();await details.getByText('诊断报告已复制').waitFor();const report=await rpc(page,'clipboard.readText');const text=typeof report==='string'?report:report.text;assert.match(text,/收到 0 字节/);assert.doesNotMatch(text,/mixed-route-fixture|mixed-proxy-fixture|登录用户|@/);await writeFile(join(output,'report.txt'),text);
 await delay(500);assert.equal((await events()).filter(e=>e.event==='open').length,opens);await details.getByRole('button',{name:'关闭',exact:true}).click();
 note('Exact plugin diagnostic opens in owning native window, three themes render, safe report copies, and reading/copying creates no new connection.');
 await writeFile(join(fixture,'method-eof-2'),'1');await rpc(page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()}).catch(error=>assert.equal(error.code,'PROXY_DISCONNECTED'));
 const short=await until(async()=>{const a=(await attempts(target)).attempts[0];return a?.error?.receivedBytes===1?a:null;});assert.notEqual(short.reference.attemptId,eof.reference.attemptId);assert.equal((await rpc(page,'connections.diagnostics.get',{reference:eof.reference})).error.receivedBytes,0);
 note('Second real attempt records EOF 1/2 with a new identity; first EOF 0/2 record remains immutable.');
 await rpc(page,'commands.execute',{pluginId:'sodeck.monitor',commandId:'monitor.open',workspaceId:target.id,operationId:crypto.randomUUID()});const monitor=await surface('sodeck.monitor');await until(async()=>await monitor.getByRole('button',{name:'查看连接详情'}).count(),20000);await monitor.screenshot({path:join(output,'monitor-passive.png')});
 const files=await surface('sodeck.files');await until(async()=>await files.getByRole('button',{name:'查看连接详情'}).count(),20000);await files.screenshot({path:join(output,'files-failure.png')});assert.equal(await files.getByRole('button',{name:'查看连接详情'}).count(),1);
 note('Files shows one connection fault; passive monitor exposes the same session diagnostic without dialing or sampling hops.');
 await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});const network=await surface('sodeck.network-proxy'),networkContext=await network.evaluate(()=>window.__SODECK_CONTEXT__);
 const rule=await invoke(network,'network.save',{expectedRevision:0,rule:{name:'诊断网络',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}});
 await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:networkContext.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}}).catch(e=>{if(e.code!=='PROXY_DISCONNECTED')throw Error(e.code+': '+e.message);});
 const failedRun=(await invoke(network,'network.snapshot',{selected:rule.id})).selected.run;assert.ok(failedRun.error.diagnosticRef);const member=await rpc(page,'connections.diagnostics.get',{reference:failedRun.error.diagnosticRef});assert.equal(member.purpose,'network-data');assert.ok(member.memberId);assert.notEqual(member.reference.attemptId,short.reference.attemptId);
 await network.getByText('诊断网络',{exact:true}).click();await until(async()=>await network.getByRole('button',{name:'查看连接详情',exact:true}).count());await network.getByRole('button',{name:'查看连接详情',exact:true}).click();await details.waitFor();await details.getByText(/网络池连接/).waitFor();await details.getByRole('button',{name:'关闭',exact:true}).click();await network.screenshot({path:join(output,'network-failure.png')});
 const blocker=net.createServer();await new Promise(r=>blocker.listen(0,'127.0.0.1',r));try{const local=await invoke(network,'network.save',{expectedRevision:0,rule:{name:'监听冲突',type:'port-forward',targetHost:'127.0.0.1',targetPort:80,bindMode:'fixed',port:blocker.address().port,autoReconnect:false}});await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:networkContext.viewId,operationId:crypto.randomUUID(),args:{id:local.id}}).catch(()=>{});const localRun=(await invoke(network,'network.snapshot',{selected:local.id})).selected.run;assert.ok(localRun.error);assert.equal(localRun.error.diagnosticRef,undefined);const history=await invoke(network,'network.diagnostics',{id:local.id,start:0});assert.ok(history.items.some(d=>d.stage==='listen'));}finally{await new Promise(r=>blocker.close(r));}
 note('Network failure keeps its own pool member and attempt reference; native details distinguishes network-data from session, and local listener conflict has no SSH diagnostic reference.');
 await unlink(join(fixture,'method-eof-2'));await rpc(page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()}).catch(()=>{});
 await until(async()=>{await answer();return(await rpc(page,'resources.list')).ptys.some(p=>p.state==='active');},60000);assert.equal(confirmed.size,3);assert.equal(await page.getByRole('dialog',{name:'将已保存凭据用于此路径',exact:true}).count(),0);
 const ready=(await attempts(target)).attempts.find(a=>a.state==='succeeded');assert.ok(ready);assert.ok((await phases(ready)).some(p=>p.stage==='deliver'&&p.state==='succeeded'));
 const sessions=(await events()).filter(e=>e.event==='session');assert.ok(sessions.every(e=>e.index===3));note('After removing the fixture fault, only target identity is confirmed, the target PTY starts once, and delivery success follows lease registration.');
 child.stdin.end();const disconnected=await until(async()=>{const r=await rpc(page,'connections.diagnostics.get',{reference:ready.reference});return r.lifecycle?r:null;},25000);assert.equal(disconnected.state,'succeeded');assert.equal(disconnected.lifecycle.state,'disconnected');note('Transport outage is appended as a lifecycle event while the original successful connection result stays unchanged.');
 await writeFile(join(output,'records.json'),JSON.stringify({eof,short,ready},null,2));assert.deepEqual(app.errors,[]);
}catch(error){failed={message:error.message,stack:error.stack};if(app){failed.body=await app.page.locator('body').innerText().catch(()=>null);await app.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});}throw error;}
finally{for(const b of browsers.values())await b.close().catch(()=>{});await stop(app);child.stdin.end();await Promise.race([new Promise(r=>child.once('exit',r)),delay(3000).then(()=>child.kill())]);await writeFile(join(output,'results.json'),JSON.stringify({checks,failed,shutdown:app?.shutdown},null,2));await writeFile('.qa/t027/diagnostics-native-latest.json',JSON.stringify({output,checks,failed},null,2));}
