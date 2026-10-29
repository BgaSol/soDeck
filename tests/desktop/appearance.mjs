import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,rename,rmdir} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve(process.env.T023_APPEARANCE_OUTPUT??'.qa/t011/native');await mkdir(output,{recursive:true});
const cfg=JSON.parse(await readFile(process.env.SODECK_TEST_REMOTE_CONFIG??'.qa/t010/linux/test-config.json','utf8'));assert.equal(cfg.host,'127.0.0.1');assert.ok(Number.isInteger(cfg.port)&&cfg.port>1024);
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9792,'sodeck.terminal':9793});
delete process.env.SODECK_THEME_FILE;
const data=join(output,'data-'+Date.now()),checks=[],browsers=[];let app,mainHandle;
const note=s=>{checks.push(s);console.log(s);};
const all=b=>b.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('sodeck'));
async function connect(port){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port),25000);browsers.push(b);return b;}
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function server(name){const s=await rpc(app.page,'servers.create',{name,host:cfg.host,port:cfg.port,username:cfg.username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:s.id,privateKeyPath:cfg.privateKeyPath,remember:true});try{const lease=await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:s.id,...e.details});}return s;}
async function pageFor(b,id){return until(async()=>{for(const p of all(b))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===id)return p;},25000);}
const pref=()=>rpc(app.page,'ui.theme.preference.get');
async function select(mode,page=app.page){const p=await pref();return rpc(page,'ui.theme.select',{mode,expectedRevision:p.revision});}
async function applied(pages,p){await until(async()=>(await Promise.all(pages.map(page=>page.evaluate(()=>document.documentElement.dataset.themeRevision)))).every(r=>r===p.revision),5000);}
const ids=r=>Object.fromEntries(['connections','ptys','leases'].map(k=>[k,r[k].map(v=>v.id??v.ptyId??v.leaseId??v.connectionId).sort()]));
try{
 app=await launch({data,shellPort:9791,pluginPort:9792});mainHandle=(await nativeWindow(app,'list')).find(w=>w.visible).handle;assert.equal((await pref()).themeId,'dopamine');await app.page.screenshot({path:join(output,'welcome-dark.png')});
 const a=await server('开发环境 · 柔和双主题'),b=await server('验证环境 · 长名称服务器');await rpc(app.page,'workspaces.open',{serverId:a.id});
 const fb=await connect(9792),tb=await connect(9793);
 let wb=await rpc(app.page,'workbench.snapshot');const ta=await pageFor(tb,wb.views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===a.id).viewId),fa=await pageFor(fb,wb.views.find(v=>v.pluginId==='sodeck.files'&&v.workspaceId===a.id).viewId);
 await until(async()=>(await invoke(fa,'files.snapshot')).path==='/home/'+cfg.username,30000);await ta.locator('.xterm-helper-textarea').pressSequentially('cd /tmp\r');await until(async()=>(await invoke(fa,'files.snapshot')).path==='/tmp',30000);
 await rpc(app.page,'workspaces.open',{serverId:b.id,target:'newWindow'});wb=await rpc(app.page,'workbench.snapshot');
 const child=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(p!==app.page&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.windowId))return p;});
 wb=await until(async()=>{const w=await rpc(child,'workbench.snapshot');return ['sodeck.terminal','sodeck.files'].every(id=>w.views.some(v=>v.pluginId===id&&v.workspaceId===b.id))?w:false;},30000);const tc=await pageFor(tb,wb.views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===b.id).viewId),fc=await pageFor(fb,wb.views.find(v=>v.pluginId==='sodeck.files'&&v.workspaceId===b.id).viewId);
 await until(async()=>(await invoke(fc,'files.snapshot')).path==='/home/'+cfg.username,30000);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:a.id,operationId:crypto.randomUUID()});
 wb=await rpc(app.page,'workbench.snapshot');
 // Resolve logical IDs from host state; commands may return an operation receipt.
 const firstId=await ta.evaluate(()=>window.__SODECK_CONTEXT__.viewId);const extraId=(await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.terminal'&&v.workspaceId===a.id&&v.viewId!==firstId),30000)).viewId;
 await (await pageFor(tb,extraId)).locator('.xterm-helper-textarea').waitFor();await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===extraId)?.status==='running',30000);
 await rpc(app.page,'views.move',{viewId:extraId,placement:'detached'});const detached=await until(async()=>{for(const p of app.browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.detachedViewId)===extraId)return p;});
 const td=await pageFor(tb,extraId);await td.locator('.xterm-helper-textarea').waitFor();await rpc(app.page,'views.activate',{viewId:firstId});
 const surfaces=[app.page,child,detached,ta,fa,tc,fc,td];for(const p of surfaces)await p.waitForFunction(()=>document.documentElement.dataset.themeRevision);
 const initialStats=(await rpc(app.page,'diagnostics.get')).communication;assert.equal(initialStats.contracts['app.theme.v1'],surfaces.length);
 for(const t of [ta,tc,td])await t.evaluate(()=>{window.__originalTerminal=document.querySelector('.xterm');});
 const before=ids(await rpc(app.page,'resources.list'));await fa.getByLabel('远端绝对路径').fill('/tmp/未提交草稿');
 const latencies=[];for(let i=0;i<20;i++){const p=await select(i%2?'dark':'light',i%2?child:app.page),start=performance.now();await applied(surfaces,p);latencies.push(performance.now()-start);}
 assert.deepEqual(ids(await rpc(app.page,'resources.list')),before);for(const t of [ta,tc,td])assert.equal(await t.evaluate(()=>window.__originalTerminal===document.querySelector('.xterm')),true);
 assert.equal(await fa.getByLabel('远端绝对路径').inputValue(),'/tmp/未提交草稿');assert.ok(latencies.toSorted((a,b)=>a-b)[18]<=300,JSON.stringify(latencies));note('AP-02/04: 3 windows, 8 rendered surfaces, 20 switches converge within 300 ms p95; PTYs, connections, terminal DOM and file draft preserved');
 const prior=await pref(),same=await select(prior.mode);assert.equal(same.revision,prior.revision);const next=await select('light');await assert.rejects(rpc(child,'ui.theme.select',{mode:'dark',expectedRevision:prior.revision}),e=>e.code==='REVISION_CONFLICT');
 const preferenceFile=join(data,'appearance-preferences.json');await rename(preferenceFile,preferenceFile+'.saved');await mkdir(preferenceFile);
 try{await assert.rejects(select('dark'),e=>e.code==='PERSISTENCE_FAILED');assert.equal((await pref()).revision,next.revision);}finally{await rmdir(preferenceFile);await rename(preferenceFile+'.saved',preferenceFile);}
 note('AP-03: same-mode no-op, stale revision rejection and real filesystem write failure preserve committed palette');
 const slow=await rpc(child,'state.open',{contract:'app.theme.v1',target:{app:'current'}});await rpc(child,'state.ack',{subscriptionId:slow.subscriptionId,feedEpoch:slow.feedEpoch,sequence:'0'});
 for(let i=0;i<200;i++){const p=await select(i%2?'light':'dark');await applied([app.page,child,detached,fa,ta],p);}
 await delay(10000);const stalledStats=(await rpc(app.page,'diagnostics.get')).communication;assert.ok(stalledStats.queueBytes<8*1024*1024);assert.equal(stalledStats.contracts['app.theme.v1'],surfaces.length+1);await rpc(child,'state.close',{subscriptionId:slow.subscriptionId,feedEpoch:slow.feedEpoch});assert.deepEqual(ids(await rpc(app.page,'resources.list')),before);note('AP-08: 200 switches with an unacknowledged consumer plus a 10-second stall do not block healthy surfaces or recreate resources');
 await fa.getByLabel('远端绝对路径').press('Escape');
 for(const mode of ['light','dark']){await applied(surfaces,await select(mode));let win=(await nativeWindow(app,'list')).find(w=>w.handle===mainHandle);await nativeWindow(app,'show',{WindowHandle:win.handle});await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'workspace-'+mode+'.png')});const menuId=crypto.randomUUID();await pluginRpc(ta,'ui.menu.open',{menuId,anchor:{x:60,y:60},items:[{id:'copy',label:'复制',enabled:true},{id:'paste',label:'粘贴',enabled:false}]});await nativeWindow(app,'screen',{WindowHandle:win.handle,OutputPath:join(output,'menu-'+mode+'.png')});await select(mode==='light'?'dark':'light',child);await until(async()=>(await pluginRpc(ta,'ui.menu.result',{menuId})).status==='closed');}
 await select('light');try{await rpc(child,'windows.merge',{operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'CLOSE_REVIEW_REQUIRED');}await until(()=>child.isClosed());await rpc(app.page,'views.move',{viewId:extraId,placement:'center'});await rpc(app.page,'workspaces.close',{workspaceId:b.id});await rpc(app.page,'workspaces.close',{workspaceId:a.id});await delay(1100);assert.equal((await rpc(app.page,'ui.theme.get')).mode,'light');
 note('AP-01/02/08: native menu cancels on theme change; detached/workspace merge and closing workspaces retain app theme');
 assert.equal((await rpc(app.page,'diagnostics.get')).communication.contracts['app.theme.v1'],1);
 await stop(app);app=await launch({data,shellPort:9791,pluginPort:9792});assert.equal((await pref()).mode,'light');assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);note('AP-02: complete restart persists light preference with a clean workspace');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,latencies,p95:latencies.toSorted((a,b)=>a-b)[18],initialStats,stalledStats,data},null,2));
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}finally{await stop(app);for(const b of browsers)await b.close().catch(()=>{});}
