import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createInterface} from 'node:readline';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import net from 'node:net';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';

const output=resolve('.qa/t020/native-'+Date.now());await mkdir(output,{recursive:true});
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u','tests/core/socks_route_fixture.py',join(output,'fixture')],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python'),SODECK_TEST_SOCKS_TARGET_PORT:'22320'}});
let stderr='';fixture.stderr.on('data',b=>stderr+=b);
const config=await new Promise((r,j)=>{createInterface({input:fixture.stdout}).once('line',line=>r(JSON.parse(line)));fixture.once('exit',()=>j(Error(stderr)));});
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':9942,'sodeck.files':9943,'sodeck.network-proxy':9944});
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:20000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args),contrast=(...args)=>ps('tests/desktop/high-contrast.ps1',args);
const original={dpi:await dpi(),contrast:await contrast()};
let app;const browsers=new Map(),checks=[];
const note=text=>{checks.push(text);console.log(text);};
async function plugin(name,port){let browser=browsers.get(name);if(!browser?.isConnected()){browser=await chromium.connectOverCDP('http://127.0.0.1:'+port);browsers.set(name,browser);}return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(p.url().includes('sodeck-plugin.localhost')&&await p.evaluate(name=>window.__SODECK_CONTEXT__?.pluginId==='sodeck.'+(name==='network'?'network-proxy':name),name))return p;return false;});}
async function screenshot(name){await app.page.screenshot({path:join(output,name+'.png')});}
try{
 app=await launch({data:join(output,'data'),shellPort:9941,pluginPort:9942});
 const page=app.page;
 await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
 await page.getByRole('button',{name:'管理网络代理',exact:true}).click();
 await page.getByRole('button',{name:'添加网络代理',exact:true}).waitFor();await screenshot('proxy-manager');await page.keyboard.press('Escape');
 await page.getByRole('dialog').getByRole('button',{name:'添加服务器',exact:true}).click();
 await page.getByLabel('显示名称',{exact:true}).fill('SOCKS 目标 · OpenSSH');await page.getByLabel('主机地址',{exact:true}).fill('target.internal');await page.getByLabel('用户名',{exact:true}).fill('tester');await page.getByRole('combobox',{name:/^认证方式/}).selectOption('key');
 assert.deepEqual(await page.getByRole('combobox',{name:/^访问方式/}).locator('option').evaluateAll(nodes=>nodes.map(n=>n.value)),['direct','hostTunnel','networkProxy']);
 await page.getByRole('combobox',{name:/^访问方式/}).selectOption('networkProxy');await page.getByRole('button',{name:'添加网络代理',exact:true}).click();
 await page.getByLabel('代理名称',{exact:true}).fill('尚未保存');await page.keyboard.press('Escape');
 assert.equal(await page.getByLabel('显示名称',{exact:true}).inputValue(),'SOCKS 目标 · OpenSSH');assert.equal(await page.getByRole('button',{name:'添加网络代理',exact:true}).evaluate(e=>e===document.activeElement),true);
 await page.getByRole('button',{name:'添加网络代理',exact:true}).click();await page.getByLabel('代理名称',{exact:true}).fill('独立 SOCKS 服务');await page.getByLabel('代理地址',{exact:true}).fill('127.0.0.1');await page.getByLabel('端口',{exact:true}).filter({visible:true}).fill(String(config.authPort));await page.getByRole('combobox',{name:/^代理认证/}).selectOption('usernamePassword');await page.getByLabel('代理用户名',{exact:true}).fill('proxy-user');await page.getByRole('button',{name:'保存并选用',exact:true}).click();
 await page.getByLabel('显示名称',{exact:true}).waitFor();assert.equal((await rpc(page,'servers.list')).servers.length,0);assert.equal((await rpc(page,'workbench.snapshot')).workspaces.length,0);
 const win=(await nativeWindow(app,'list')).find(w=>w.visible);await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:900,Height:600});
 if(process.env.SODECK_TEST_SKIP_IME!=='1'){
 const ime=(action,args=[])=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(win.handle),...args]);
 const name=page.getByLabel('显示名称',{exact:true});await nativeWindow(app,'show',{WindowHandle:win.handle});await name.fill('');await name.focus();
 await page.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
 const imeOriginal=await ime('state');let shifted=false;
 try{
  await nativeWindow(app,'show',{WindowHandle:win.handle});await name.focus();await ime('language',['-Layout','0000000008040804']);
  for(let n=0;n<2;n++){await ime('keys',['-Keys','zhongwen']);await delay(200);if(await page.evaluate(()=>window.__imeComposing))break;await name.fill('');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;}
  assert.equal(await page.evaluate(()=>window.__imeComposing),true);await screenshot('ime-composition');await ime('keys',['-Keys','SPACE']);
  await until(async()=>!await page.evaluate(()=>window.__imeComposing));const events=await page.evaluate(()=>window.__imeEvents);
  assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));
  assert.equal((await rpc(page,'servers.routes.list')).servers.length,0);await writeFile(join(output,'ime-events.json'),JSON.stringify(events,null,2));note('Native Microsoft Pinyin composition commits Chinese text without submitting the target form');
 }finally{if(await page.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await nativeWindow(app,'show',{WindowHandle:win.handle});await ime('language',['-Layout',imeOriginal.focusLayout]);await name.fill('SOCKS 目标 · OpenSSH');}
 }
 for(const themeId of ['dopamine','soft-light','soft-dark']){const pref=await rpc(page,'ui.theme.preference.get');await rpc(page,'ui.theme.choose',{themeId,expectedRevision:pref.revision});await delay(100);const box=await page.getByRole('button',{name:'保存并打开',exact:true}).boundingBox();const height=await page.evaluate(()=>innerHeight);assert.ok(box.y>=0&&box.y+box.height<=height);await screenshot('editor-'+themeId);}
 for(const percent of [100,200]){await dpi('-Action','set','-Percent',String(percent));await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:900*percent/100,Height:600*percent/100});await delay(250);await page.getByRole('combobox',{name:/^访问方式/}).scrollIntoViewIfNeeded();const button=page.getByRole('button',{name:'保存并打开',exact:true});const box=await button.boundingBox();assert.ok(box.y+box.height<=await page.evaluate(()=>innerHeight));await button.focus();await page.keyboard.press('Tab');assert.equal(await page.getByRole('button',{name:'关闭对话框'}).evaluate(e=>e===document.activeElement),true);await screenshot('editor-dpi-'+percent);}
 await contrast('-Action','set','-Flags',String(original.contrast.flags|1),'-Scheme',original.contrast.scheme??'');await until(async()=>(await rpc(page,'ui.theme.preference.get')).highContrast);await screenshot('editor-high-contrast');await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');
 await dpi('-Action','set','-Percent',String(original.dpi.percent));await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:1200,Height:800});
 await page.emulateMedia({reducedMotion:'reduce'});await screenshot('editor-reduced-motion');
 note('Native editor: three access modes, independent proxy management, child Escape/focus/draft, three themes at 900x600');
 await page.getByRole('button',{name:'保存并打开',exact:true}).click();await page.getByRole('dialog',{name:'网络代理认证',exact:true}).waitFor();await until(async()=>(await page.locator('.connection-status').innerText()).startsWith('网络代理'));await screenshot('proxy-auth');
 assert.equal(await page.getByLabel('私钥文件路径').count(),0);await page.getByLabel('代理密码',{exact:true}).fill('proxy-password');await page.getByRole('button',{name:'继续连接目标',exact:true}).click();
 await page.getByRole('dialog',{name:'确认服务器身份',exact:true}).waitFor();await screenshot('target-trust');await page.getByRole('button',{name:'信任并连接',exact:true}).click();
 await page.getByLabel('私钥文件路径',{exact:true}).fill(resolve('.qa/t008/linux/test-key'));await page.getByRole('button',{name:'连接',exact:true}).click();
 await until(async()=>(await rpc(page,'resources.list')).ptys.some(p=>p.state==='active'),60000);
 const target=(await rpc(page,'servers.routes.list')).servers[0];const status=await rpc(page,'connections.route.get',{serverId:target.id,workspaceId:target.id});assert.deepEqual(status.running[0].resources,{sessions:1,bridges:0});assert.equal(status.saved.accessMode,'networkProxy');
 note('Real OpenSSH via authenticated SOCKS: proxy auth, then target-only host trust/key login; one SSH session, zero bridges');
 const terminal=await plugin('terminal',9942);await terminal.locator('.xterm-helper-textarea').waitFor();await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('tester@'),30000);await terminal.locator('.xterm-helper-textarea').pressSequentially("printf 'T020_%s\\n' NATIVE_OK");await terminal.locator('.xterm-helper-textarea').press('Enter');await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('T020_NATIVE_OK'),30000);await terminal.screenshot({path:join(output,'terminal.png')});
 const workspace=(await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.serverId===target.id);const scope={workspaceId:workspace.workspaceId,workspaceGeneration:workspace.generation};
 const lease=await rpc(page,'connections.acquireExisting',{scope,serverId:target.id,configRevision:target.revision,operationId:crypto.randomUUID()});
 const common={scope,leaseId:lease.leaseId,connectionGeneration:lease.generation};
 const stat=await rpc(page,'files.stat',{...common,operationId:crypto.randomUUID(),path:{bytesBase64:Buffer.from('/etc/os-release').toString('base64')}});assert.ok(['file','symlink'].includes(stat.type));
 const files=await plugin('files',9943);await rpc(page,'commands.execute',{pluginId:'sodeck.files',commandId:'files.connect',workspaceId:target.id,operationId:crypto.randomUUID()});const fsnap=await until(async()=>{const s=await pluginRpc(files,'plugin.invoke',{command:'files.snapshot',args:{count:100}});return s.state==='ready'&&!s.loading&&s.rows?.length>1?s:false;},45000);assert.ok(fsnap.rows.length);await rpc(page,'commands.execute',{pluginId:'sodeck.files',commandId:'files.connect',workspaceId:target.id,operationId:crypto.randomUUID()});assert.equal((await pluginRpc(files,'plugin.invoke',{command:'files.snapshot',args:{count:100}})).state,'ready');await delay(200);await files.screenshot({path:join(output,'files.png')});
 const exec=await rpc(page,'exec.start',{...common,operationId:crypto.randomUUID(),command:"printf 'socks-real-exec'; exit 7"});let text='',after=0;await until(async()=>{const r=await rpc(page,'exec.read',{...common,execId:exec.execId,afterSequence:after});for(const f of r.frames){text+=Buffer.from(f.bytesBase64,'base64').toString();after=f.sequence;}await rpc(page,'exec.ack',{...common,execId:exec.execId,sequence:after});if(r.state==='completed'){assert.equal(r.exitCode,7);return true;}return false;},15000);assert.ok(text.includes('socks-real-exec'));await rpc(page,'exec.close',{...common,execId:exec.execId});
 await rpc(page,'connections.release',{leaseId:lease.leaseId});note('SFTP stat and official files plugin directory listing share the target connection');
 const opened=await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});
 const network=await until(()=>plugin('network',9944),30000);const invoke=(command,args={})=>pluginRpc(network,'plugin.invoke',{command,args});
 await network.getByRole('button',{name:'创建',exact:true}).waitFor();await until(()=>invoke('network.snapshot'),15000);
 const rule=await invoke('network.save',{expectedRevision:0,rule:{name:'OpenSSH 测试入口',type:'port-forward',bindMode:'auto',port:0,autoReconnect:false,targetHost:'127.0.0.1',targetPort:22}});
 await rpc(page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:opened.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});
 const run=await until(async()=>{const s=await invoke('network.snapshot',{selected:rule.id});return s.selected.run?.status==='ready'?s.selected.run:false;},45000);
 const socket=net.connect(run.address.port,'127.0.0.1');let banner='';socket.on('data',b=>banner+=b.toString());socket.on('error',()=>{});try{await until(()=>banner.startsWith('SSH-2.0-OpenSSH'),15000);}finally{socket.destroy();}
 const routes=await rpc(page,'connections.route.list',{});assert.ok(routes.connections.length>=2);assert.ok(routes.connections.every(r=>r.resources.sessions===1&&r.resources.bridges===0));await network.getByText('OpenSSH 测试入口',{exact:true}).click();await network.getByText('就绪',{exact:true}).waitFor();await network.getByText('连接路径',{exact:true}).click();await network.getByText('SOCKS5 网络代理 · 1 台主机',{exact:true}).waitFor();await network.screenshot({path:join(output,'network.png')});await invoke('network.stop',{id:rule.id});note('Official network plugin uses its own SOCKS-backed pool and forwards a real OpenSSH banner');
 await page.locator('.connection-status').click();await screenshot('path-details');await page.keyboard.press('Escape');
 const proxy=(await rpc(page,'proxies.socks5.list')).proxies[0];await assert.rejects(rpc(page,'proxies.socks5.delete',{proxyId:proxy.id}),e=>e.code==='PROXY_DEPENDENTS');
 await rpc(page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()});await until(async()=>(await rpc(page,'resources.list')).ptys.length===2);
 const before=(await rpc(page,'resources.list')).ptys.map(p=>p.id).sort();await rpc(page,'workspaces.move',{workspaceId:target.id,operationId:crypto.randomUUID()});const owner=(await rpc(page,'workbench.snapshot')).workspaces.find(w=>w.workspaceId===target.id).ownerWindowId;await rpc(page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()});assert.deepEqual((await rpc(page,'resources.list')).ptys.map(p=>p.id).sort(),before);
 await rpc(page,'app.hide');await delay(200);assert.equal((await rpc(page,'resources.list')).ptys.length,2);await nativeWindow(app,'show',{WindowHandle:win.handle});
 const terms=(await rpc(page,'workbench.snapshot')).views.filter(v=>v.pluginId==='sodeck.terminal');await rpc(page,'views.close',{viewId:terms[0].viewId});assert.equal((await rpc(page,'resources.list')).ptys.length,1);await rpc(page,'workspaces.close',{workspaceId:target.id});await until(async()=>{const r=await rpc(page,'resources.list');return !r.connections.length&&!r.ptys.length&&!r.leases.length;});note('Two terminals, close one, workspace detach/merge, tray hide/restore and complete cleanup preserve ownership');
 assert.deepEqual(app.errors,[]);
}catch(error){console.error(error);for(const [name,browser] of browsers){for(const [i,p] of browser.contexts().flatMap(c=>c.pages()).entries()){await p.screenshot({path:join(output,name+'-failure-'+i+'.png')}).catch(()=>{});if(name==='files')await pluginRpc(p,'plugin.invoke',{command:'files.snapshot',args:{count:100}}).then(v=>writeFile(join(output,'files-failure.json'),JSON.stringify(v,null,2))).catch(()=>{});}}await app?.page.screenshot({path:join(output,"failure.png")}).catch(()=>{});throw error;}finally{
 await stop(app);await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await dpi('-Action','set','-Percent',String(original.dpi.percent));for(const b of browsers.values())await b.close().catch(()=>{});
 await until(async()=>{const text=await readFile(join(output,'fixture/proxy-events.jsonl'),'utf8').catch(()=> '');return (text.match(/"event": "open"/g)??[]).length===(text.match(/"event": "close"/g)??[]).length;},5000);
 fixture.stdin.end();await until(()=>fixture.exitCode!==null,5000).catch(()=>fixture.kill());
 await writeFile(join(output,'results.json'),JSON.stringify({checks,stderr,shutdown:app?.shutdown,restored:{dpi:await dpi(),contrast:await contrast()},original},null,2));await writeFile('.qa/t020/native-latest.json',JSON.stringify({output,checks},null,2));
}
