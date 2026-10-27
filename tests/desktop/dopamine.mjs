import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,rename,rmdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve('.qa/t012/native');await mkdir(output,{recursive:true});
const cfg=JSON.parse(await readFile('.qa/t010/linux/test-config.json','utf8'));assert.equal(cfg.host,'127.0.0.1');assert.equal(cfg.port,22281);
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9842,'sodeck.terminal':9843});
delete process.env.SODECK_THEME_FILE;
const data=join(output,'data-'+Date.now()),checks=[],browsers=[];let app,mainHandle;
const note=s=>{checks.push(s);console.log(s);};
const all=b=>b.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('sodeck'));
async function connect(port){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port),25000);browsers.push(b);return b;}
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function server(name){const s=await rpc(app.page,'servers.create',{name,host:cfg.host,port:cfg.port,username:cfg.username,authType:'key'});await rpc(app.page,'auth.set',{serverId:s.id,privateKeyPath:cfg.privateKeyPath,remember:true});try{const lease=await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:s.id,...e.details});}return s;}
async function pageFor(b,id){return until(async()=>{for(const p of all(b))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===id)return p;},25000);}
const pref=()=>rpc(app.page,'ui.theme.preference.get');
async function select(mode,page=app.page){const p=await pref();return rpc(page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:p.revision});}
async function applied(pages,p){await until(async()=>(await Promise.all(pages.map(page=>page.evaluate(()=>document.documentElement.dataset.themeRevision)))).every(r=>r===p.revision),5000);}
const ids=r=>Object.fromEntries(['connections','ptys','leases'].map(k=>[k,r[k].map(v=>v.id??v.ptyId??v.leaseId??v.connectionId).sort()]));
try{
 app=await launch({data,shellPort:9841,pluginPort:9842});mainHandle=(await nativeWindow(app,'list')).find(w=>w.visible).handle;assert.equal((await pref()).themeId,'dopamine');await app.page.screenshot({path:join(output,'welcome-dopamine.png')});
 const a=await server('开发环境 · 柔和双主题'),b=await server('验证环境 · 长名称服务器');await rpc(app.page,'workspaces.open',{serverId:a.id});
 const fb=await connect(9842),tb=await connect(9843);
 let wb=await rpc(app.page,'workbench.snapshot');const ta=await pageFor(tb,wb.views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===a.id).viewId),fa=await pageFor(fb,wb.views.find(v=>v.pluginId==='sodeck.files'&&v.workspaceId===a.id).viewId);
 await until(async()=>(await invoke(fa,'files.snapshot')).path==='/home/'+cfg.username,30000);await ta.locator('.xterm-helper-textarea').pressSequentially('cd /tmp\r');await until(async()=>(await invoke(fa,'files.snapshot')).path==='/tmp',30000);
 await rpc(app.page,'workspaces.open',{serverId:b.id,target:'newWindow'});wb=await rpc(app.page,'workbench.snapshot');
 const child=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(p!==app.page&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId))return p;});
 wb=await rpc(child,'workbench.snapshot');const tc=await pageFor(tb,wb.views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===b.id).viewId),fc=await pageFor(fb,wb.views.find(v=>v.pluginId==='sodeck.files'&&v.workspaceId===b.id).viewId);
 await until(async()=>(await invoke(fc,'files.snapshot')).path==='/home/'+cfg.username,30000);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:a.id,operationId:crypto.randomUUID()});
 wb=await rpc(app.page,'workbench.snapshot');
 // Resolve logical IDs from host state; commands may return an operation receipt.
 const firstId=await ta.evaluate(()=>window.__SODECK_CONTEXT__.viewId);const extraId=wb.views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===a.id&&v.viewId!==firstId).viewId;
 await (await pageFor(tb,extraId)).locator('.xterm-helper-textarea').waitFor();await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===extraId)?.status==='running',30000);
 await rpc(app.page,'views.move',{viewId:extraId,placement:'detached'});const detached=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.detachedViewId)===extraId)return p;});
 const td=await pageFor(tb,extraId);await td.locator('.xterm-helper-textarea').waitFor();await rpc(app.page,'views.activate',{viewId:firstId});
 const icons=JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File','tests/desktop/brand-windows.ps1','-AppProcessId',String(app.process.pid),'-OutputDirectory',resolve('.qa/t012/brand')],{windowsHide:true})).stdout);await writeFile('.qa/t012/brand/native-icons.json',JSON.stringify(icons,null,2));assert.equal(icons.filter(w=>w.title.includes('SoDeck')&&w.icon!==0).length,3);
 const surfaces=[app.page,child,detached,ta,fa,tc,fc,td];for(const p of surfaces)await p.waitForFunction(()=>document.documentElement.dataset.themeRevision);
 const initialStats=(await rpc(app.page,'diagnostics.get')).communication;assert.equal(initialStats.contracts['app.theme.v1'],surfaces.length);
 for(const t of [ta,tc,td])await t.evaluate(()=>{window.__originalTerminal=document.querySelector('.xterm');});
 const before=ids(await rpc(app.page,'resources.list'));await fa.getByLabel('远端绝对路径').fill('/tmp/未提交草稿');
 const latencies=[];for(let i=0;i<30;i++){const p=await select(['dopamine','light','dark'][i%3],i%2?child:app.page),start=performance.now();await applied(surfaces,p);latencies.push(performance.now()-start);}
 assert.deepEqual(ids(await rpc(app.page,'resources.list')),before);for(const t of [ta,tc,td])assert.equal(await t.evaluate(()=>window.__originalTerminal===document.querySelector('.xterm')),true);
 assert.equal(await fa.getByLabel('远端绝对路径').inputValue(),'/tmp/未提交草稿');assert.ok(latencies.toSorted((a,b)=>a-b)[28]<=300,JSON.stringify(latencies));note('DP-02/04: 3 windows, 8 rendered surfaces, 30 switches converge within 300 ms p95; PTYs, connections, terminal DOM and file draft preserved');
 await select('dark');const prior=await pref(),same=await select(prior.themeId==='dopamine'?'dopamine':prior.mode);assert.equal(same.revision,prior.revision);const next=await select('light');await assert.rejects(rpc(child,'ui.theme.select',{mode:'dark',expectedRevision:prior.revision}),e=>e.code==='REVISION_CONFLICT');
 const preferenceFile=join(data,'appearance-preferences.json');await rename(preferenceFile,preferenceFile+'.saved');await mkdir(preferenceFile);
 try{await assert.rejects(select('dark'),e=>e.code==='PERSISTENCE_FAILED');assert.equal((await pref()).revision,next.revision);}finally{await rmdir(preferenceFile);await rename(preferenceFile+'.saved',preferenceFile);}
 note('DP-03: same-mode no-op, stale revision rejection and real filesystem write failure preserve committed palette');
 const slow=await rpc(child,'state.open',{contract:'app.theme.v1',target:{app:'current'}});await rpc(child,'state.ack',{subscriptionId:slow.subscriptionId,feedEpoch:slow.feedEpoch,sequence:'0'});
 for(let i=0;i<200;i++){const p=await select(['dopamine','light','dark'][i%3]);await applied([app.page,child,detached,fa,ta],p);}
 await delay(10000);const stalledStats=(await rpc(app.page,'diagnostics.get')).communication;assert.ok(stalledStats.queueBytes<8*1024*1024);assert.equal(stalledStats.contracts['app.theme.v1'],surfaces.length+1);await rpc(child,'state.close',{subscriptionId:slow.subscriptionId,feedEpoch:slow.feedEpoch});assert.deepEqual(ids(await rpc(app.page,'resources.list')),before);note('DP-08: 200 switches with an unacknowledged consumer plus a 10-second stall do not block healthy surfaces or recreate resources');
 await fa.getByLabel('远端绝对路径').press('Escape');
 for(const mode of ['dopamine','light','dark']){await applied(surfaces,await select(mode));let win=(await nativeWindow(app,'list')).find(w=>w.handle===mainHandle);await nativeWindow(app,'show',{WindowHandle:win.handle});await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'workspace-'+mode+'.png')});const menuId=crypto.randomUUID();await pluginRpc(ta,'ui.menu.open',{menuId,anchor:{x:60,y:60},items:[{id:'copy',label:'复制',enabled:true},{id:'paste',label:'粘贴',enabled:false}]});await nativeWindow(app,'screen',{WindowHandle:win.handle,OutputPath:join(output,'menu-'+mode+'.png')});await select(mode==='light'?'dark':'light',child);await until(async()=>(await pluginRpc(ta,'ui.menu.result',{menuId})).status==='closed');}
 // The separate app popup must retain the underlying native WebViews and live PTYs.
 const menuPages=()=>app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('application-menu'));
 async function openApplication(owner){await rpc(owner,'windows.control',{action:'show'});await delay(120);await owner.getByRole('button',{name:'应用菜单'}).click();let p;try{p=await until(()=>menuPages()[0]);}catch(error){console.log('menu open failure',await owner.evaluate(()=>({context:window.__SODECK_CONTEXT__,focus:document.hasFocus(),reason:window.__sodeckApplicationMenuReason,expanded:document.querySelector('[aria-label="应用菜单"]')?.getAttribute('aria-expanded'),errors:[...document.querySelectorAll('[role="alert"]')].map(e=>e.textContent)})));throw error;}try{await p.getByRole('menu').waitFor();}catch(error){console.log('popup closed before ready',await owner.evaluate(()=>({reason:window.__sodeckApplicationMenuReason,focus:document.hasFocus()})));throw error;}return p;}
 for(const mode of ['dopamine','light','dark']){await applied(surfaces,await select(mode));await nativeWindow(app,'show',{WindowHandle:mainHandle});const pop=await openApplication(app.page);await nativeWindow(app,'screen',{WindowHandle:mainHandle,OutputPath:join(output,'application-menu-'+mode+'.png')});await pop.getByRole('menuitem',{name:/外观/}).click();await app.page.getByRole('dialog',{name:'外观',exact:true}).waitFor();await app.page.screenshot({path:join(output,'appearance-'+mode+'.png')});await app.page.keyboard.press('Escape');}
 await applied(surfaces,await select('dopamine'));
 const beforeMenus=ids(await rpc(app.page,'resources.list')),subscriptions=(await rpc(app.page,'diagnostics.get')).communication.subscriptions;
 for(let i=0;i<200;i++){const owner=[app.page,child,detached][i%3];if(i<6)console.log('opening menu',i,await owner.evaluate(()=>window.__SODECK_CONTEXT__));const pop=await openApplication(owner);await pop.keyboard.press(i%2?'Escape':'Tab');await until(()=>menuPages().length===0);if(i%10===0)console.log('application menu cycle',i);}
 assert.deepEqual(ids(await rpc(app.page,'resources.list')),beforeMenus);assert.equal((await rpc(app.page,'diagnostics.get')).communication.subscriptions,subscriptions);assert.equal(menuPages().length,0);note('AM-04: 200 owned popups across three windows release all surfaces, preserve PTYs and add no subscriptions');
 // Wrong popup token and arbitrary core access cannot execute a shell command.
 const pop=await openApplication(app.page);const denied=await pop.evaluate(async()=>{try{await window.__TAURI_INTERNALS__.invoke('application_menu_reply',{token:'wrong',action:'exit',reason:'select'});return false;}catch{return true;}});assert.equal(denied,true);await assert.rejects(rpc(pop,'app.exit'),e=>e.code==='SOURCE_INVALID');await pop.keyboard.press('Escape');
 note('AM-02/05: source-token validation and restricted popup bridge reject forged selection and arbitrary shell RPC');
 await select('light');await rpc(child,'windows.merge',{operationId:crypto.randomUUID()});await rpc(app.page,'views.move',{viewId:extraId,placement:'center'});await rpc(app.page,'workspaces.close',{workspaceId:b.id});await rpc(app.page,'workspaces.close',{workspaceId:a.id});await delay(1100);assert.equal((await rpc(app.page,'ui.theme.get')).mode,'light');
 note('DP-01/02/08: native menu cancels on theme change; detached/workspace merge and closing workspaces retain app theme');
 assert.equal((await rpc(app.page,'diagnostics.get')).communication.contracts['app.theme.v1'],1);
 await stop(app);app=await launch({data,shellPort:9841,pluginPort:9842});assert.equal((await pref()).mode,'light');assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);note('DP-02: complete restart persists light preference with a clean workspace');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,latencies,p95:latencies.toSorted((a,b)=>a-b)[28],initialStats,stalledStats,data},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}finally{await stop(app);for(const b of browsers)await b.close().catch(()=>{});}
