// T-025: real Tauri/WebView2 and an isolated OpenSSH guest on loopback 22345.
import assert from 'node:assert/strict';
import net from 'node:net';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,pluginRpc,pluginPages,until,stop,nativeWindow,delay} from './harness.mjs';

const output=resolve('.qa/t025/native-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t025/linux/config.json','utf8'));
assert.equal(config.port,22345);
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`.qa/t025/packages/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':12552,'sodeck.files':12553,'sodeck.network-proxy':12554});
for(const port of [12551,12552,12553,12554]){const listener=net.createServer();await new Promise((ok,no)=>listener.once('error',no).listen(port,'127.0.0.1',ok));await new Promise(ok=>listener.close(ok));}
const checks=[];const note=text=>{checks.push(text);console.log(text);};let app,settings;
async function terminalFor(viewId){return until(async()=>{for(const p of await pluginPages(app)){if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===viewId)return p;}},30000);}
async function home(page,intent={kind:'home'}){await rpc(page,'ui.connectionHome.open',{operationId:crypto.randomUUID(),intent});await page.getByRole('tab',{name:'新建连接',exact:true}).waitFor();}
async function saveSsh(page,name,host,port,{connect=false,jumps=[]}={}){
  await home(page,{kind:'newSsh'});await page.getByLabel('主机地址',{exact:true}).fill(host);await page.getByLabel('SSH 端口',{exact:true}).fill(String(port));await page.getByLabel('用户名',{exact:true}).fill('tester');
  await page.getByLabel('私钥',{exact:true}).check();await page.getByLabel('私钥文件',{exact:true}).fill(config.privateKeyPath);await page.getByLabel('显示名称',{exact:true}).fill(name);
  if(jumps.length){await page.getByLabel('SSH 跳板',{exact:true}).check();for(const name of jumps){await page.getByRole('combobox',{name:'选择已有服务器'}).fill(name);await page.getByRole('option',{name:new RegExp(name)}).click();}}
  await page.getByRole('button',{name:connect?'保存并连接':'仅保存',exact:true}).click();
  if(!connect)await page.getByText('连接已保存',{exact:true}).waitFor();
  return until(async()=>(await rpc(page,'servers.list')).servers.find(s=>s.name===name));
}
async function trustUntilConnected(page,serverId){
  await until(async()=>{const dialog=page.getByRole('dialog',{name:'确认服务器身份',exact:true});if(await dialog.count())await dialog.getByRole('button',{name:'信任并连接',exact:true}).click();return (await rpc(page,'resources.list')).ptys.some(p=>p.state==='active');},60000);
  const layout=await rpc(page,'workbench.snapshot');return layout.views.find(v=>v.workspaceId===serverId&&v.pluginId==='sodeck.terminal');
}
try{
  app=await launch({data:join(output,'data'),shellPort:12551,pluginPort:12552,packagePath:resolve('.qa/t025/packages/terminal.sodeck-plugin')});const page=app.page;
  await page.getByRole('heading',{name:'新建 SSH 连接',exact:true}).waitFor();
  assert.equal((await rpc(page,'workbench.snapshot')).workspaces.length,0);assert.equal((await rpc(page,'resources.list')).ptys.length,0);
  const server=await saveSsh(page,'T025 direct','127.0.0.1',config.port);assert.equal((await rpc(page,'workbench.snapshot')).workspaces.length,0);
  assert.equal((await rpc(page,'servers.get',{serverId:server.id})).hasSecret,true);
  note('Native startup and only-save create no workspace/PTY; conditional private-key preset persists');
  await page.getByRole('listitem',{name:'打开服务器 T025 direct'}).click();const view=await trustUntilConnected(page,server.id);const terminal=await terminalFor(view.viewId);
  await terminal.locator('.xterm-helper-textarea').waitFor();await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('tester@'),30000);await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'T025_%s\\n' REAL_SSH");await terminal.locator('.xterm-helper-textarea').press('Enter');await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('T025_REAL_SSH'),30000);
  const before=await rpc(page,'resources.list'),mount=view.mountGeneration;
  await home(page,{kind:'newSsh'});await page.getByLabel('主机地址',{exact:true}).fill('draft-kept.invalid');
  await until(async()=>!(await pluginRpc(terminal,'views.presentation',{viewId:view.viewId})).visible);
  await rpc(page,'workbench.surface',{windowId:'main',viewId:view.viewId,mountGeneration:mount,bounds:{x:44,y:80,width:900,height:500},visible:true});
  assert.equal((await pluginRpc(terminal,'views.presentation',{viewId:view.viewId})).visible,false);
  await assert.rejects(pluginRpc(terminal,'views.requestFocus',{viewId:view.viewId,userGesture:true}),e=>e.code==='STALE_MOUNT');
  assert.equal((await rpc(page,'workbench.snapshot')).focusedViewId,null);
  assert.ok((await nativeWindow(app,'list')).some(w=>w.title==='新建连接 — SoDeck'));
  await page.getByRole('tab',{name:'T025 direct',exact:true}).click();await until(async()=>(await pluginRpc(terminal,'views.presentation',{viewId:view.viewId})).visible);
  assert.deepEqual((await rpc(page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
  assert.equal((await rpc(page,'workbench.snapshot')).views.find(v=>v.viewId===view.viewId).mountGeneration,mount);
  await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'T025_%s\\n' RESUMED");await terminal.locator('.xterm-helper-textarea').press('Enter');await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('T025_RESUMED'));
  note('Real PTY/mount survive system-page selection; late layout and hidden focus are rejected; native title and resumed input verified');
  const domBefore=await page.locator('*').count();
  for(let cycle=0;cycle<100;cycle++){
    await home(page);await page.getByRole('tab',{name:'T025 direct',exact:true}).click();
  }
  await until(async()=>(await pluginRpc(terminal,'views.presentation',{viewId:view.viewId})).visible);
  const afterCycles=await rpc(page,'resources.list');assert.deepEqual(afterCycles.ptys.map(p=>p.id),before.ptys.map(p=>p.id));assert.equal(afterCycles.connections.length,before.connections.length);assert.equal(afterCycles.leases.length,before.leases.length);
  assert.equal((await rpc(page,'workbench.snapshot')).views.find(v=>v.viewId===view.viewId).mountGeneration,mount);assert.ok(await page.locator('*').count()<=domBefore+4);
  note('100 native home/workspace switches retain one PTY, unchanged connection/lease counts and mount, with no growing shell DOM');
  // Dirty home remains hidden while application-wide close is reviewed.
  await rpc(page,'ui.settings.open',{operationId:crypto.randomUUID(),pageId:'plugin:sodeck.terminal:general'});
  settings=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()))if(await p.evaluate(()=>!!window.__SODECK_SETTINGS_CONTEXT__).catch(()=>false))return p;});
  await settings.getByLabel('字号',{exact:true}).fill('21');
  await assert.rejects(rpc(page,'app.exit'),e=>e.code==='CLOSE_REVIEW_REQUIRED');
  await page.getByRole('dialog',{name:'关闭前处理未完成配置'}).waitFor();await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  await settings.getByRole('button',{name:'继续编辑',exact:true}).click();
  await until(()=>page.getByRole('dialog',{name:'关闭前处理未完成配置'}).count().then(n=>n===0));
  await home(page);assert.equal(await page.getByLabel('主机地址',{exact:true}).inputValue(),'draft-kept.invalid');assert.equal(await settings.getByLabel('字号',{exact:true}).inputValue(),'21');
  note('Two-phase app exit: allowing hidden home then canceling settings preserves both drafts and live PTY');
  await settings.getByRole('button',{name:'取消',exact:true}).click();await settings.getByRole('button',{name:'放弃更改',exact:true}).click();await until(()=>settings.isClosed());settings=undefined;
  const win=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title==='新建连接 — SoDeck');
  await nativeWindow(app,'close',{WindowHandle:win.handle});await until(async()=>!(await nativeWindow(app,'list')).find(w=>w.handle===win.handle)?.visible);assert.equal(app.process.exitCode,null);await rpc(page,'app.show');
  assert.equal(await page.getByRole('dialog').count(),0);note('Native main-window close hides to tray without a draft prompt');
  // Source-child draft must be reviewed before its final workspace is moved out.
  await rpc(page,'workspaces.move',{workspaceId:server.id,operationId:crypto.randomUUID()});
  const owner=(await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.serverId===server.id).ownerWindowId;
  const child=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId).catch(()=>null)===owner)return p;});
  await home(child,{kind:'newSsh'});await child.getByLabel('主机地址',{exact:true}).fill('child-kept.invalid');
  await assert.rejects(rpc(page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()}),e=>e.code==='CLOSE_REVIEW_REQUIRED');
  await child.getByRole('button',{name:'继续编辑',exact:true}).click();await child.getByRole('dialog',{name:'关闭前处理未完成配置'}).waitFor({state:'hidden'});assert.equal((await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.serverId===server.id).ownerWindowId,owner);
  await assert.rejects(rpc(page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()}),e=>e.code==='CLOSE_REVIEW_REQUIRED');await child.getByRole('button',{name:'放弃修改',exact:true}).click();
  await until(async()=>(await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.serverId===server.id).ownerWindowId==='main');await until(()=>child.isClosed());
  assert.deepEqual((await rpc(page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));note('Last-workspace merge is atomic with child draft review; cancel moves nothing, confirmed merge preserves PTY');
  await home(page);assert.equal(await page.getByLabel('主机地址',{exact:true}).inputValue(),'draft-kept.invalid');
  await page.getByRole('button',{name:'取消',exact:true}).last().click();await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  // Saved jump nodes only. Their own access paths are deliberately ignored by explicit ordering.
  await saveSsh(page,'T025 gateway 1','127.0.0.1',22345);
  await saveSsh(page,'T025 gateway 2','127.0.0.1',2201);
  const target=await saveSsh(page,'T025 multi-hop','127.0.0.1',2202,{connect:true,jumps:['T025 gateway 1','T025 gateway 2']});
  await until(async()=>{const d=page.getByRole('dialog',{name:'确认服务器身份',exact:true});if(await d.count())await d.getByRole('button',{name:'信任并连接',exact:true}).click();const auth=page.getByRole('dialog',{name:'连接认证',exact:true});if(await auth.count()){await auth.getByLabel('私钥文件路径',{exact:true}).fill(config.privateKeyPath);await auth.getByRole('button',{name:'连接',exact:true}).click();}return (await rpc(page,'resources.list')).ptys.length===2;},60000);
  const route=await rpc(page,'connections.route.get',{serverId:target.id,workspaceId:target.id});assert.equal(route.saved.accessMode,'hostTunnel');assert.equal(route.running[0].resources.sessions,3);assert.equal(route.running[0].resources.bridges,2);
  note('New UI saved two jump nodes and connected a target through three real OpenSSH sessions / two bridges');
  await page.screenshot({path:join(output,'workbench.png')});
  assert.deepEqual(app.errors,[]);await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,data:app.data},null,2));
}catch(error){if(app)for(const p of await pluginPages(app).catch(()=>[])){await writeFile(join(output,'terminal-failure.txt'),await p.locator('body').innerText().catch(()=>''));}await app?.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(error),stack:error.stack,data:app?.data},null,2));throw error;}
finally{
  if(app){
    const pages=()=>app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed());
    for(const p of pages())await p.getByRole('button',{name:'继续编辑',exact:true}).click({timeout:400}).catch(()=>{});
    await rpc(app.page,'app.exit').catch(()=>{});
    for(const p of pages())for(const name of ['放弃修改','放弃并退出'])await p.getByRole('button',{name,exact:true}).click({timeout:700}).catch(()=>{});
    await until(()=>app.process.exitCode!==null,10000).catch(()=>{});await stop(app);
  }
  await writeFile('.qa/t025/native-latest.json',JSON.stringify({output,checks,shutdown:app?.shutdown},null,2));
}
