// Native API6 consumers on the isolated A -> B -> B-loopback SOCKS -> T topology.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,nativeWindow,delay} from './harness.mjs';

const output=resolve('.qa/t027/native-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t027/linux/config.json','utf8'));
const names=['terminal','files','network-proxy','monitor'];
process.env.SODECK_PLUGIN_PACKAGES=names.map(n=>resolve(`.qa/t027/packages/${n}.sodeck-plugin`)).join(delimiter);
const ports=Object.fromEntries(names.map((n,i)=>['sodeck.'+n,12752+i]));
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify(ports);
const checks=[],browsers=new Map();let app,failed;
const note=s=>{checks.push(s);console.log(s);};
async function viewPage(plugin,viewId){
  let browser=browsers.get(plugin);if(!browser?.isConnected()){browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+ports[plugin]));browsers.set(plugin,browser);}
  return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(p.url().includes('sodeck-plugin')&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.pluginId)===plugin&&(!viewId||await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===viewId)){await p.waitForSelector('#root > *');return p;}});
}
const invoke=(p,command,args={})=>pluginRpc(p,'plugin.invoke',{command,args});
async function preset(server,entry){const p=await rpc(app.page,'credentials.configuration.prepare',{target:{kind:'ssh',id:server.id},expectedOwnerRevision:server.revision,expectedDependencies:[]});await rpc(app.page,'credentials.configuration.set',{operationId:crypto.randomUUID(),contextToken:p.contextToken,privateKeyPath:entry.key,remember:true});}
async function selectSsh(page,index,name){
  await page.getByRole('button',{name:'添加 SSH',exact:true}).click();
  const input=page.getByRole('combobox',{name:`第 ${index} 站 SSH 服务器`});
  await input.click();
  await input.fill(name);
  const list=page.getByRole('listbox',{name:`第 ${index} 站 SSH 服务器`});
  await list.getByRole('option').filter({hasText:name}).first().click();
}
try{
  app=await launch({data:join(output,'data'),shellPort:12751,pluginPort:12752,packagePath:resolve('.qa/t027/packages/terminal.sodeck-plugin')});const page=app.page;
  const installed=await rpc(page,'plugins.list');assert.equal(installed.length,4);assert.ok(installed.every(p=>p.activeVersion&&!p.error&&!p.pendingVersion));
  assert.ok((await rpc(page,'capabilities.list')).capabilities.includes('connections.route.v2'));
  const [a,b,,targetEntry]=config.paths[0];
  const servers=[];for(const entry of [a,b]){const s=await rpc(page,'servers.routes.create',{name:`T027 ${entry.name.toUpperCase()}`,host:entry.host,port:entry.port,username:entry.username,authType:'key',accessPath:{steps:[]}});await preset(s,entry);servers.push(s);}
  await rpc(page,'ui.connectionHome.open',{operationId:crypto.randomUUID(),intent:{kind:'newSsh'}});
  await page.getByLabel('主机地址',{exact:true}).fill(targetEntry.host);await page.getByLabel('用户名',{exact:true}).fill(targetEntry.username);
  await page.getByLabel('私钥',{exact:true}).check();await page.getByLabel('私钥文件',{exact:true}).fill(targetEntry.key);await page.getByLabel('显示名称',{exact:true}).fill('T027 混合目标');
  await page.getByLabel('经由中转',{exact:true}).check();await selectSsh(page,1,'T027 A');await selectSsh(page,2,'T027 B');
  await page.getByRole('button',{name:'添加 SOCKS5',exact:true}).click();await page.getByRole('button',{name:'新建代理',exact:true}).last().click();
  const inline=page.getByRole('region',{name:'就地新建代理'});await inline.getByLabel('代理地址',{exact:true}).fill('127.0.0.1');await inline.getByLabel('代理端口',{exact:true}).fill('12021');await inline.getByRole('button',{name:'保存并选用'}).click();await until(async()=>await inline.count()===0);
  await page.getByText('由SSH 主机 T027 B访问；127.0.0.1 指SSH 主机 T027 B所在的网络环境',{exact:true}).waitFor();
  const win=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title.includes('SoDeck'));
  const scale=await page.evaluate(()=>devicePixelRatio);
  await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(920*scale),Height:Math.round(630*scale)});
  for(const themeId of ['soft-light','soft-dark','dopamine']){const pref=await rpc(page,'ui.theme.preference.get');await rpc(page,'ui.theme.choose',{themeId,expectedRevision:pref.revision});await page.getByText(/127.0.0.1 指SSH/).scrollIntoViewIfNeeded();await page.screenshot({path:join(output,'mixed-'+themeId+'.png')});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
  await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(1280*scale),Height:Math.round(850*scale)});
  await page.getByRole('button',{name:'保存并连接',exact:true}).click();
  const trusted=[],confirmed=[];
  await until(async()=>{
    const trust=page.getByRole('dialog',{name:'确认身份与登录',exact:true});if(await trust.count()){trusted.push(await trust.innerText());await trust.getByRole('button',{name:'信任并连接',exact:true}).click();}
    const preset=page.getByRole('dialog',{name:'将已保存凭据用于此路径',exact:true});if(await preset.count()){confirmed.push(await preset.innerText());await preset.getByRole('checkbox').check();await preset.getByRole('button',{name:'确认并继续',exact:true}).click();}
    return (await rpc(page,'resources.list')).ptys.some(p=>p.state==='active');
  },90000);
  assert.equal(trusted.length,3);assert.equal(confirmed.length,0);
  const target=(await rpc(page,'servers.list')).servers.find(s=>s.name==='T027 混合目标');assert.ok(target);
  const status=await rpc(page,'connections.route.get',{serverId:target.id,workspaceId:target.id});assert.deepEqual([status.saved.sshCount,status.saved.proxyCount,status.saved.stepCount],[3,1,4]);assert.equal(status.currentCount,1);
  console.log('Connected target; locating terminal surface');
  const terminal=await viewPage('sodeck.terminal');
  await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('mr_t@'),30000);
  await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'T027_USER=%s\\n' \"$(id -un)\"; cat ~/t027-marker.txt");
  await terminal.locator('.xterm-helper-textarea').press('Enter');
  await until(async()=>{const text=await terminal.locator('.xterm-rows').innerText();return text.includes('T027_USER=mr_t')&&text.includes('T027_FINAL_TARGET');});
  note('Native ordered editor, B-loopback explanation, three themes, 3 combined identity/login confirmations with no extra preset dialogs; terminal executes only on mr_t');
  const layout=await rpc(page,'workbench.snapshot');assert.ok(layout.workspaces.every(w=>w.serverId===target.id));
  const files=await viewPage('sodeck.files');await until(async()=>{const s=await invoke(files,'files.snapshot',{count:300});return s.state==='ready'&&!s.loading;},30000);
  await invoke(files,'files.locate',{text:'/home/mr_t/t027-marker.txt'});
  const fileSnapshot=await until(async()=>{const s=await invoke(files,'files.snapshot',{count:300});return !s.loading&&JSON.stringify(s).includes('t027-marker.txt')?s:null;},30000);
  await writeFile(join(output,'files.json'),JSON.stringify(fileSnapshot,null,2));
  await rpc(page,'commands.execute',{pluginId:'sodeck.monitor',commandId:'monitor.open',workspaceId:target.id,operationId:crypto.randomUUID()});const monitor=await viewPage('sodeck.monitor');await until(async()=>{const s=await invoke(monitor,'monitor.snapshot',{viewId:await monitor.evaluate(()=>window.__SODECK_CONTEXT__.viewId)});return s.cpu.quality==='fresh';},45000);await monitor.screenshot({path:join(output,'monitor.png')});
  note('Files read target marker over SFTP; resource monitor acquires existing target lease and publishes real Linux CPU samples');
  const networkView=await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});const network=await viewPage('sodeck.network-proxy');
  for(const type of ['network-proxy','port-forward']){
    const rule=await invoke(network,'network.save',{expectedRevision:0,rule:{name:'T027 '+type,type,bindMode:'auto',port:0,autoReconnect:false,...(type==='port-forward'?{targetHost:'127.0.0.1',targetPort:8088}:{})}});
    await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:networkView.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});
    const run=await until(async()=>{const s=(await invoke(network,'network.snapshot',{selected:rule.id})).selected.run;return s?.status==='ready'?s:null;},30000);assert.equal(run.path.proxyCount,1);
    for(const scheme of type==='network-proxy'?['http','socks5h']:['direct']){
      const args=['-sf','--max-time','20','--noproxy','','--proxy',scheme==='direct'?'':`${scheme}://127.0.0.1:${run.address.port}`,`http://127.0.0.1:${scheme==='direct'?run.address.port:8088}/t027-marker.txt`];
      const result=await promisify(execFile)('curl.exe',args,{windowsHide:true});assert.match(result.stdout,/T027_FINAL_TARGET/);
    }
    await invoke(network,'network.stop',{id:rule.id});
  }
  note('Network plugin HTTP proxy, SOCKS5 proxy and fixed forwarding all fetch target-only HTTP content through the mixed chain');
  const firstView=await terminal.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
  const firstPty=(await rpc(page,'resources.list')).ptys.find(p=>p.state==='active');
  const second=await rpc(page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()});
  await until(async()=>(await rpc(page,'resources.list')).ptys.filter(p=>p.state==='active').length===2);
  const retained=(await rpc(page,'resources.list')).ptys.filter(p=>p.state==='active').map(p=>p.id).sort();
  const moved=await rpc(page,'workspaces.move',{workspaceId:target.id,operationId:crypto.randomUUID()});
  try{await rpc(page,'windows.merge',{sourceWindowId:moved.targetWindowId,targetWindowId:'main',operationId:crypto.randomUUID()});}
  catch(error){
    assert.equal(error.code,'CLOSE_REVIEW_REQUIRED');
    const child=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId).catch(()=>null)===moved.targetWindowId)return p;});
    await child.getByRole('button',{name:'放弃修改',exact:true}).click();
  }
  await until(async()=>(await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.serverId===target.id).ownerWindowId==='main');
  await rpc(page,'windows.close',{windowId:'main'});await rpc(page,'windows.control',{action:'show'});
  assert.deepEqual((await rpc(page,'resources.list')).ptys.filter(p=>p.state==='active').map(p=>p.id).sort(),retained);
  await rpc(page,'views.close',{viewId:second.viewId});
  assert.ok((await rpc(page,'resources.list')).ptys.some(p=>p.id===firstPty.id&&p.state==='active'));
  note('Two target terminals survive workspace detach/merge and tray restore with unchanged PTY IDs; closing one preserves the other consumers');
  const q=await rpc(page,'proxies.socks5.create',{name:'T027 Q',host:'127.0.0.1',port:12022,authType:'none',username:''});
  const old=await rpc(page,'servers.routes.get',{serverId:target.id});
  const {id:serverId,revision:expectedRevision,hasSecret,...configuration}=old;
  const edited=await rpc(page,'servers.routes.update',{...configuration,serverId,expectedRevision,accessPath:{steps:[...old.accessPath.steps,{stepId:crypto.randomUUID(),kind:'socks5',proxyId:q.id,resolve:'proxy'}]}});
  const operationId=crypto.randomUUID();let fresh;
  for(let n=0;n<8;n++){
    try{fresh=await rpc(page,'connections.acquire',{serverId:target.id,workspaceId:target.id,operationId});break;}
    catch(error){const d=error.details;if(error.code==='HOST_KEY_REQUIRED'||error.code==='CREDENTIAL_CONFIRM_REQUIRED')await rpc(page,'auth.ssh.confirm',{challengeId:d.challengeId,operationId,trust:error.code==='HOST_KEY_REQUIRED'?'accept':'keep',credential:'preset',remember:true});else throw error;}
  }
  assert.ok(fresh);const changed=await rpc(page,'connections.route.get',{serverId:target.id,workspaceId:target.id});
  assert.equal(changed.saved.proxyCount,2);assert.ok(changed.changedCount>=1);assert.ok(changed.currentCount>=1);
  const originalPage=await viewPage('sodeck.terminal',firstView);await rpc(page,'views.activate',{viewId:firstView});
  await originalPage.locator('.xterm-helper-textarea').pressSequentially("printf 'T027_OLD_SNAPSHOT_STILL_ALIVE\\n'");await originalPage.locator('.xterm-helper-textarea').press('Enter');
  await until(async()=>(await originalPage.locator('.xterm-rows').innerText()).includes('T027_OLD_SNAPSHOT_STILL_ALIVE'));
  await rpc(page,'connections.release',{leaseId:fresh.leaseId});
  await writeFile(join(output,'changed-route.json'),JSON.stringify({edited,changed},null,2));
  note('Adding a second remote SOCKS creates a fresh route epoch while the original terminal continues on its retained snapshot');
  await writeFile(join(output,'target.json'),JSON.stringify({target,servers,status},null,2));
  assert.deepEqual(app.errors,[]);
}catch(error){failed={message:error.message,stack:error.stack};if(app){failed.body=await app.page.locator('body').innerText().catch(()=>null);failed.plugins=await Promise.all([...browsers].flatMap(([plugin,browser])=>browser.contexts().flatMap(c=>c.pages()).map(async p=>({plugin,url:p.url(),body:await p.locator('body').innerText().catch(()=>null)}))));await app.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});}throw error;}
finally{for(const browser of browsers.values())await browser.close().catch(()=>{});await stop(app);await writeFile(join(output,'results.json'),JSON.stringify({checks,failed,shutdown:app?.shutdown},null,2));await writeFile('.qa/t027/native-latest.json',JSON.stringify({output,checks,failed},null,2));}
