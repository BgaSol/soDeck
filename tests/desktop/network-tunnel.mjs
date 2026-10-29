import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {createInterface} from 'node:readline';
import net from 'node:net';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';

const task=process.argv[2]??'t017';if(!['t017','t017-r2'].includes(task))throw Error('Unknown test task');
const output=resolve(`.local/test-results/${task}/network-native-`+Date.now());await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u','tests/core/network_fixture.py',join(output,'fixture'),'3'],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python')}});
let stderr='';fixture.stderr.on('data',b=>stderr+=b);
const config=await new Promise((resolve,reject)=>{const lines=createInterface({input:fixture.stdout});lines.once('line',line=>resolve(JSON.parse(line)));fixture.once('exit',()=>reject(Error(stderr)));});
const packagePath=resolve('resources/network-proxy.sodeck-plugin');
process.env.SODECK_PLUGIN_PACKAGES=packagePath;
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9892});
let app,browser,page;const checks=[];
const note=text=>{checks.push(text);console.log(text);};
const invoke=(command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const snapshot=()=>invoke('network.snapshot');
const curl=async(port,scheme,host)=>{
  const {stdout}=await promisify(execFile)('curl.exe',['--fail','--silent','--show-error','--max-time','15','--noproxy','','--proxy',`${scheme}://127.0.0.1:${port}`,`http://${host}/`],{windowsHide:true});return stdout;
};
async function echo(port,prefix){
  const socket=net.connect(port,'127.0.0.1');const response=[];socket.on('data',b=>response.push(b));socket.on('error',()=>{});
  try{await new Promise((r,j)=>{socket.once('connect',r);socket.once('error',j);});socket.write(prefix);await until(()=>Buffer.concat(response).toString().startsWith(prefix));socket.end();await until(()=>Buffer.concat(response).toString().endsWith('AFTER-EOF'));}finally{socket.destroy();}
}
try {
  app=await launch({data:join(output,'data'),shellPort:9891,pluginPort:9892,packagePath,allowStartupWarning:true});
  let previous;const nodes=[];
  for(let i=0;i<3;i++) {
    const node=await rpc(app.page,'servers.routes.create',{name:`Network node ${i+1}`,host:i===0?'127.0.0.1':`node-${i}.internal`,port:i===0?config.port:22,username:'network-test',accessPath:previous?{kind:'sshJump',jumpServerId:previous.id}:{kind:'direct'}});
    await rpc(app.page,'auth.ssh.set',{serverId:node.id,password:'isolated-network-password',remember:false});nodes.push(node);previous=node;
  }
  const target=nodes.at(-1);await rpc(app.page,'workspaces.open',{serverId:target.id});
  assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0,'network must not open automatically');
  const opened=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9892'));
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin.localhost')));
  await page.getByRole('button',{name:'创建',exact:true}).waitFor();
  await page.screenshot({path:join(output,'empty.png')});
  await page.getByRole('button',{name:'创建',exact:true}).click();
  const editor=await until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages())){try{if(await p.evaluate(()=>!!window.__SODECK_CONTEXT__?.dialog))return p;}catch{}}return false;});
  await editor.getByLabel('连接名称（可选）',{exact:true}).waitFor();
  assert.equal(await editor.getByLabel('连接名称（可选）',{exact:true}).inputValue(),'');
  assert.equal(await editor.getByLabel('自动分配端口',{exact:true}).isChecked(),true);
  assert.equal(await editor.getByLabel('本地端口',{exact:true}).count(),0);
  await editor.screenshot({path:join(output,'create-dialog.png')});
  await editor.getByRole('button',{name:'保存并启动',exact:true}).click();
  for(let hop=0;hop<3;hop++) {
    await app.page.getByRole('button',{name:'信任并连接',exact:true}).waitFor();
    assert.equal(await app.page.locator('.identity-dialog .route-preview').count(),0);
    const box=await app.page.locator('.identity-dialog').boundingBox();assert.ok(box.height<500,JSON.stringify(box));
    if(hop===0){for(const mode of ['dopamine','light','dark']){const pref=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:pref.revision});await delay(120);await app.page.screenshot({path:join(output,`identity-${mode}.png`)});}const pref=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:'dopamine',expectedRevision:pref.revision});}
    await app.page.getByRole('button',{name:'信任并连接',exact:true}).click();
  }
  let s=await until(async()=>{const s=await snapshot();return s.items[0]?.run?.status==='ready'?s:false;},45000);
  const rule=s.items[0].rule,port=s.items[0].run.address.port;
  assert.equal(rule.name,'内网代理');assert.equal(rule.port,0);assert.ok(port>0);
  await page.getByRole('button',{name:'内网代理',exact:true}).click();
  note('Native UI: create/save/start, three core host identity dialogs and one loopback proxy ready');
  assert.equal(await curl(port,'http','http.internal'),'REMOTE-http.internal');
  assert.equal(await curl(port,'socks5h','second.internal'),'REMOTE-second.internal');
  note('Real QuickJS + three-node Paramiko fixture: same port HTTP and SOCKS remote targets');
  await page.getByText('连接检查',{exact:true}).click();
  await page.getByLabel('测试目标主机').fill('echo.internal');await page.getByLabel('测试端口').fill('1234');await page.getByRole('button',{name:'检查连接',exact:true}).click();
  await until(async()=>{const s=await snapshot();return s.items[0].run.check?.state==='succeeded';});
  await page.screenshot({path:join(output,'proxy-ready.png')});
  const fixed=await invoke('network.save',{expectedRevision:0,rule:{name:'固定服务入口',type:'port-forward',bindMode:'auto',port:0,autoReconnect:false,targetHost:'echo.internal',targetPort:1234}});
  await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:opened.viewId,operationId:crypto.randomUUID(),args:{id:fixed.id}});
  s=await snapshot();const fixedPort=s.items.find(i=>i.rule.id===fixed.id).run.address.port;
  await echo(fixedPort,'network-fixed');
  const result=await invoke('network.check',{id:fixed.id});assert.equal(result.state,'succeeded');
  await invoke('network.stop',{id:fixed.id});assert.equal(await curl(port,'http','second.internal'),'REMOTE-second.internal');
  note('Fixed forwarding, real listener checks, reverse half-close and independent rule stop');
  await page.getByText('连接路径',{exact:true}).click();await page.getByRole('button',{name:'编辑服务器连接',exact:true}).click();await app.page.getByRole('dialog',{name:'编辑服务器',exact:true}).waitFor();await app.page.keyboard.press('Escape');
  await page.getByRole('button',{name:'返回列表'}).click();
  for(const mode of ['dopamine','light','dark']){const pref=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:pref.revision});await delay(150);await page.screenshot({path:join(output,`list-${mode}.png`)});}
  await rpc(app.page,'workbench.configure',{dockViewId:opened.viewId,visible:false});assert.equal(await curl(port,'http','http.internal'),'REMOTE-http.internal');await rpc(app.page,'workbench.configure',{dockViewId:opened.viewId,visible:true});
  note('Three themes preserve compact identity/list geometry; hiding the sidebar keeps the proxy running');
  if(task==='t017-r2'){
    const border=await app.page.locator('.sessions-panel[data-edge=right]').evaluate(e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect(),content=e.querySelector('.native-surface').getBoundingClientRect();return {width:s.borderLeftWidth,inset:content.left-r.left};});
    assert.equal(border.width,'1px');assert.equal(border.inset,1);
    await nativeWindow(app,'capture',{OutputPath:join(output,'right-panel-border.png')});
  }

  const runId=(await snapshot()).items.find(item=>item.rule.id===rule.id).run.id;
  for(const placement of ['detached','center','right']) {
    await rpc(app.page,'views.move',{viewId:opened.viewId,placement,operationId:crypto.randomUUID()});
    if(!browser.isConnected())browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9892'));
    page=await until(async()=>{for(const candidate of browser.contexts().flatMap(c=>c.pages())){try{if(await candidate.evaluate(id=>window.__SODECK_CONTEXT__?.viewId===id,opened.viewId))return candidate;}catch{}}return false;});
    await page.getByRole('button',{name:'创建',exact:true}).waitFor();
    const run=(await snapshot()).items.find(item=>item.rule.id===rule.id).run;
    assert.equal(run.id,runId);assert.equal(run.address.port,port);
    assert.equal(await curl(port,'http','http.internal'),'REMOTE-http.internal');
  }
  note('Detached, center and right moves retain the same view, run, actual port and working traffic');
  await rpc(app.page,'views.close',{viewId:opened.viewId});
  await until(async()=>!(await rpc(app.page,'net.resources.list')).items.some(i=>i.kind==='listener'));
  await until(async()=>(await rpc(app.page,'resources.list')).connections.length===0,10000);
  note('Core-owned edit dialog, view-close listener/SSH resource cleanup');
  assert.equal(app.errors.length,0,app.errors.join('\n'));
}catch(error){if(page)await page.screenshot({path:join(output,'failure-plugin.png')}).catch(()=>{});if(app)await app.page.screenshot({path:join(output,'failure-shell.png')}).catch(()=>{});throw error;}
finally {
  await stop(app);await browser?.close().catch(()=>{});fixture.stdin.end();await until(()=>fixture.exitCode!==null,5000).catch(()=>fixture.kill());
  await writeFile(join(output,'results.json'),JSON.stringify({checks,fixture:'Paramiko protocol fixture; not Windows/Linux OpenSSH',stderr},null,2));
  await writeFile(`.local/test-results/${task}/network-native-latest.json`,JSON.stringify({output,checks},null,2));
}
