import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';

const output=resolve('.local/test-results/t018/native-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse((await readFile('.qa/t010/linux/test-config.json','utf8')).replace(/^\uFEFF/,''));
assert.equal(config.host,'127.0.0.1');assert.equal(config.port,22281);
const baseline=resolve('.local/dev/SoDeck-v2-T017-r2');
const original=join(baseline,'resources/files.sodeck-plugin'),candidate=resolve('resources/files.sodeck-plugin');
const fixed=[resolve('target/debug/sodeck-desktop.exe'),join(baseline,'resources/terminal.sodeck-plugin'),join(baseline,'resources/network-proxy.sodeck-plugin')];
const digest=async path=>createHash('sha256').update(await readFile(path)).digest('hex');
const hashes=await Promise.all(fixed.map(digest));
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9902,'sodeck.terminal':9903});
let app,file,term;const browsers=[],checks=[],measurements={};
const invoke=(command,args={})=>pluginRpc(file,'plugin.invoke',{command,args});
const snapshot=()=>invoke('files.snapshot',{countUnits:150});
const note=s=>{checks.push(s);console.log(s);};
const start=async path=>{process.env.SODECK_PLUGIN_PACKAGES=[fixed[1],fixed[2],path].join(delimiter);app=await launch({data:join(output,'data'),shellPort:9901,pluginPort:9902});};
async function connect(port){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));browsers.push(b);return until(()=>b.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));}
async function opened(server){await rpc(app.page,'workspaces.open',{serverId:server.id});file=await connect(9902);term=await connect(9903);await until(async()=>(await snapshot()).path==='/home/'+config.username,30000);}
async function locate(text){
 await file.getByRole('textbox',{name:'远端绝对路径'}).fill(text);await file.getByRole('button',{name:'定位路径',exact:true}).click();
 await until(async()=>{const s=await snapshot();return s.path===text&&!s.loading;},30000);
 const id='node-'+encodeURIComponent(Buffer.from(text).toString('base64'));
 await until(()=>file.getByRole('treeitem',{selected:true}).getAttribute('id').then(value=>value===id));
}
async function close(){await stop(app);app=undefined;for(const b of browsers.splice(0))await b.close().catch(()=>{});file=term=undefined;}
try{
 await start(original);
 const server=await rpc(app.page,'servers.create',{name:'T018 isolated Linux',host:config.host,port:config.port,username:config.username,authType:'key'});
 await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:config.privateKeyPath,remember:true});
 try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
 await opened(server);await invoke('follow.setEnabled',{enabled:false});await locate('/');
 await until(async()=>(await snapshot()).rows.find(r=>r.id==='Lw==')?.capacity?.state==='ready');
 assert.equal((await snapshot()).version,'0.5.0');assert.equal((await snapshot()).rows[0].units,2);
 const pty=(await rpc(app.page,'resources.list')).ptys[0].id;
 await rpc(app.page,'plugins.prepareUpdate',{path:candidate});
 const pending=(await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files');
 assert.equal(pending.activeVersion,'0.5.0');assert.equal(pending.pendingVersion,'0.6.0');assert.equal((await snapshot()).version,'0.5.0');assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,pty);
 note('FC-05: preparing files 0.6.0 retains live 0.5.0, two-unit rows and terminal PTY until restart');
 await close();await start(candidate);
 assert.equal((await rpc(app.page,'plugins.list')).find(p=>p.pluginId==='sodeck.files').activeVersion,'0.6.0');
 assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,0);
 await opened(server);
 await term.locator('.xterm-helper-textarea').pressSequentially('cd /tmp\r');
 await until(async()=>{const s=await snapshot();return s.path==='/tmp'&&!s.follow.loading;},30000);
 note('FC-04: original terminal reports real Bash cwd; new file rows follow /tmp');
 await invoke('follow.setEnabled',{enabled:false});await locate('/');
 await until(async()=>(await snapshot()).rows.find(r=>r.id==='Lw==')?.capacity?.state==='ready');
 const root=(await snapshot()).rows.find(r=>r.id==='Lw==');assert.equal(root.units,1);assert.ok(BigInt(root.capacity.value.total)>0n);
 await file.getByRole('meter').first().waitFor();assert.equal(await file.locator('.mount-label').count(),0);assert.equal(await file.getByRole('region',{name:'节点详情'}).count(),0);
 await file.getByRole('tree').focus();await file.getByRole('tree').press('Home');
 await until(()=>file.getByRole('tree').evaluate(e=>e.scrollTop<1));
 await until(()=>file.getByRole('textbox',{name:'远端绝对路径'}).inputValue().then(value=>value==='/'));
 measurements.root=root;
 for(const themeId of ['dopamine','soft-light','soft-dark']){
  const pref=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId,expectedRevision:pref.revision});
  await until(()=>file.locator('html').getAttribute('data-theme').then(t=>t===themeId));
  const heights=await file.getByRole('treeitem').evaluateAll(es=>es.map(e=>e.getBoundingClientRect().height));assert.ok(heights.every(h=>Math.abs(h-28)<1));
  await file.screenshot({path:join(output,`capacity-${themeId}.png`)});
 }
 note('FC-04: real OpenSSH capacity is one row in all three native WebView2 themes');
 const before=await rpc(app.page,'resources.list'),fileId=await file.evaluate(()=>window.__SODECK_CONTEXT__.viewId);
 await rpc(app.page,'workbench.configure',{dockViewId:fileId,visible:false});await until(async()=>{const d=await invoke('files.diagnostics');return d.capacities.active===0&&d.capacities.visible===0;});
 await delay(400);assert.equal((await invoke('files.diagnostics')).capacities.active,0);assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
 await rpc(app.page,'workbench.configure',{dockViewId:fileId,visible:true});
 await locate('/srv/sodeck-t008/many/item-09999.dat');await file.getByRole('treeitem',{selected:true}).filter({hasText:'item-09999.dat'}).waitFor();
 const geometry=await file.getByRole('treeitem',{selected:true}).evaluate(e=>{const r=e.getBoundingClientRect(),v=document.querySelector('.file-tree').getBoundingClientRect();return {top:r.top,bottom:r.bottom,viewportTop:v.top,viewportBottom:v.bottom};});
 assert.ok(geometry.bottom>geometry.viewportTop&&geometry.top<geometry.viewportBottom,JSON.stringify(geometry));
 const diagnostics=await invoke('files.diagnostics');assert.ok(diagnostics.capacities.entries<=256);assert.ok(diagnostics.capacities.active<=2);assert.ok(await file.getByRole('treeitem').count()<=300);
 Object.assign(measurements,{geometry,diagnostics});
 await file.screenshot({path:join(output,'located-last-file.png')});
 const windows=await nativeWindow(app,'list');await nativeWindow(app,'capture',{WindowHandle:windows.find(w=>w.visible).handle,OutputPath:join(output,'native-window.png')});
 note('FC-04: 10,000-file location, viewport bounds, hidden capacity queue and terminal isolation pass');
 await rpc(app.page,'views.close',{viewId:fileId});assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id),before.ptys.map(p=>p.id));
 await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.ptys.length&&!r.leases.length&&!r.connections.length;});
 assert.deepEqual(await Promise.all(fixed.map(digest)),hashes);
 note('FC-04/05: closing workspace releases resources; debug core and original terminal/network packages retain hashes');
}catch(e){if(file)await file.screenshot({path:join(output,'failure.png')}).catch(()=>{});throw e;}finally{
 await close();await writeFile(join(output,'results.json'),JSON.stringify({checks,measurements,unchanged:fixed.map((path,i)=>({path,sha256:hashes[i]})),environment:'Windows native WebView2, existing dev.14 debug core + isolated Debian OpenSSH'},null,2));
 await writeFile('.local/test-results/t018/native-latest.json',JSON.stringify({output,checks},null,2));
}
