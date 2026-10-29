import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve('.local/test-results/t015/files-native-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse((await readFile('.qa/t010/linux/test-config.json','utf8')).replace(/^\uFEFF/,''));assert.equal(config.host,'127.0.0.1');assert.equal(config.port,22281);
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9842,'sodeck.terminal':9843});
let app,browser,file;const checks=[];const invoke=(command,args={})=>pluginRpc(file,'plugin.invoke',{command,args});
const snapshot=()=>invoke('files.snapshot',{countUnits:150});
const note=s=>{checks.push(s);console.log(s);};
async function locate(text){await file.getByRole('textbox',{name:'远端绝对路径'}).fill(text);await file.getByRole('button',{name:'定位路径',exact:true}).click();await until(async()=>{const s=await snapshot();return s.path===text&&!s.loading;},30000);}
try{
 app=await launch({data:join(output,'data'),shellPort:9841,pluginPort:9842});
 const server=await rpc(app.page,'servers.create',{name:'T015 files Linux',host:config.host,port:config.port,username:config.username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:config.privateKeyPath});
 try{await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
 await rpc(app.page,'workspaces.open',{serverId:server.id});browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9842'));file=await until(()=>browser.contexts().flatMap(c=>c.pages())[0]);
 await until(async()=>(await snapshot()).path==='/home/'+config.username,30000);await invoke('follow.setEnabled',{enabled:false});
 await file.getByText('.bashrc',{exact:true}).waitFor();assert.equal(await file.getByRole('button',{name:'显示隐藏文件'}).count(),0);assert.equal(await file.getByRole('region',{name:'节点详情'}).count(),0);
 note('Default hidden files visible over real OpenSSH; no hidden toggle or node-details region');
 await locate('/');await until(async()=>(await snapshot()).rows.find(r=>r.id==='Lw==')?.capacity?.state==='ready');
 const root=(await snapshot()).rows.find(r=>r.id==='Lw==');assert.equal(root.units,2);assert.ok(BigInt(root.capacity.value.total)>0n);
 await file.getByRole('progressbar').first().waitFor();await file.screenshot({path:join(output,'mount-capacity.png')});
 const before=await rpc(app.page,'resources.list'),fileId=await file.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await rpc(app.page,'workbench.configure',{dockViewId:fileId,visible:false});await until(async()=>{const d=await invoke('files.diagnostics');return d.capacities.active===0&&d.capacities.visible===0;});
 await delay(500);assert.equal((await invoke('files.diagnostics')).capacities.active,0);assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
 await rpc(app.page,'workbench.configure',{dockViewId:fileId,visible:true});
 note('Mounted root shows real capacity inline; hiding releases the capacity queue and preserves terminal PTY');
 await locate('/srv/sodeck-t008/many/item-09999.dat');await file.getByRole('treeitem',{selected:true}).filter({hasText:'item-09999.dat'}).waitFor();
 const geometry=await file.getByRole('treeitem',{selected:true}).evaluate(e=>{const r=e.getBoundingClientRect(),v=document.querySelector('.file-tree').getBoundingClientRect();return {top:r.top,bottom:r.bottom,viewportTop:v.top,viewportBottom:v.bottom};});assert.ok(geometry.bottom>geometry.viewportTop&&geometry.top<geometry.viewportBottom,JSON.stringify(geometry));
 await file.screenshot({path:join(output,'located-file.png')});
 const diagnostics=await invoke('files.diagnostics');assert.ok(diagnostics.capacities.entries<=256);assert.ok(diagnostics.capacities.active<=2);assert.ok(await file.getByRole('treeitem').count()<=300);
 note('Location into a 10,000-file directory paints selected row in viewport; rendered rows and capacity work remain bounded');
 const windows=await nativeWindow(app,'list');await nativeWindow(app,'capture',{WindowHandle:windows.find(w=>w.visible).handle,OutputPath:join(output,'native-window.png')});
 await rpc(app.page,'views.close',{viewId:fileId});assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>{const r=await rpc(app.page,'resources.list');return r.ptys.length===0&&r.leases.length===0&&r.connections.length===0;});
 note('Closing file tool keeps terminal; closing workspace releases all PTYs, leases and connections');
 await writeFile(join(output,'measurements.json'),JSON.stringify({root,geometry,diagnostics},null,2));
}catch(e){if(file)await file.screenshot({path:join(output,'failure.png')}).catch(()=>{});throw e;}finally{await stop(app);await browser?.close().catch(()=>{});await writeFile(join(output,'results.json'),JSON.stringify({checks,environment:'Windows native WebView2 + isolated Debian OpenSSH'},null,2));await writeFile('.local/test-results/t015/files-native-latest.json',JSON.stringify({output}));}
