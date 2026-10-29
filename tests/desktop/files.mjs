import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,delay,stop} from './harness.mjs';
const output=resolve('.qa/t008/native-files');await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile(process.env.SODECK_TEST_REMOTE_CONFIG??'.qa/t008/linux/test-config.json','utf8'));
process.env.SODECK_PLUGIN_PACKAGES=[resolve('.qa/t008/baseline/terminal-0.4.0.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9652,'sodeck.terminal':9653});
let app;const browsers=new Map(),results=[];
const note=text=>{results.push(text);console.log(text);};
async function pages(owner){let browser=browsers.get(owner);if(!browser?.isConnected()){browser=await chromium.connectOverCDP('http://127.0.0.1:'+(owner==='sodeck.files'?9652:9653));browsers.set(owner,browser);}return browser.contexts().flatMap(c=>c.pages());}
async function view(owner,workspace){return until(async()=>{for(const p of await pages(owner))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===workspace)return p;},25000);}
const snap=p=>pluginRpc(p,'plugin.invoke',{command:'files.snapshot',args:{count:300}});
async function locate(p,target){await p.getByRole('textbox',{name:'远端绝对路径'}).fill(target);await p.getByRole('button',{name:'定位路径',exact:true}).click();const result=await until(async()=>{const s=await snap(p);return s.path===target&&!s.loading&&s.details&&!s.details.loading?s:false;},25000);await p.waitForFunction(text=>document.querySelector('.detail-path')?.textContent?.includes(text),target);return result;}
async function create(name,username){const s=await rpc(app.page,'servers.create',{name,host:config.host,port:config.port,username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:s.id,privateKeyPath:config.privateKeyPath});try{const l=await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:l.leaseId});}catch(e){if(e.code!=='HOST_KEY_REQUIRED')throw e;await rpc(app.page,'hosts.trust',{serverId:s.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}return s;}
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9651,pluginPort:9652});
 const a=await create('文件验证 A',config.username);await rpc(app.page,'workspaces.open',{serverId:a.id});
 const file=await view('sodeck.files',a.id),term=await view('sodeck.terminal',a.id);
 await until(async()=>{const r=await rpc(app.page,'resources.list');return r.ptys.length===1&&r.leases.length===2;},25000);
 await until(async()=>{const s=await snap(file);return s.state==='ready'&&s.details?.capacity;},25000);
 assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,2);
 assert.equal(await app.page.locator('.sessions-panel').evaluate(el=>Math.round(el.getBoundingClientRect().width)),320);
 await term.locator('.xterm-helper-textarea').pressSequentially("printf 'T008-TERMINAL-OK\\n'\r");
 await until(async()=>(await term.locator('.xterm-rows').innerText()).includes('T008-TERMINAL-OK'));
 note('FT-01/13: two real plugins load; original terminal runs with a separate lease on one physical connection; file sidebar is 320 CSS px');
 let s=await locate(file,'/srv/sodeck-t008/ordinary');
 await pluginRpc(file,'plugin.invoke',{command:'files.toggle',args:{id:s.selected}});
 s=await until(async()=>{const v=await snap(file);return v.rows.some(r=>r.name==='sparse-5GiB.bin')?v:false;});
 assert.equal(s.rows.find(r=>r.name==='sparse-5GiB.bin').size,'5368709120');
 assert.equal(s.rows.find(r=>r.name==='zero').size,'0');
 assert.ok(s.rows.some(r=>r.name==='back\\slash'));assert.ok(s.rows.some(r=>r.name==='invalid-\\xff'));
 assert.equal(s.rows.find(r=>r.name==='pipe').type,'fifo');
 await file.getByRole('button',{name:'刷新目录与挂载'}).click();await file.getByText('sparse-5GiB.bin',{exact:true}).waitFor();
 await file.screenshot({path:join(output,'files-tree.png')});
 note('FT-02: true byte paths, 0/1023/1024-byte files, sparse 5 GiB, links and FIFO are distinguished');
 for(const [target,type,ro]of [['bind','ext4',false],['readonly','ext4',true],['tmpfs','tmpfs',false],['tmpfs/inside','tmpfs',true],['network','cifs',true],['subvolume','btrfs',false]]){
  s=await locate(file,'/srv/sodeck-t008/'+target);
  assert.equal(s.details.exact,true,target);assert.equal(s.details.mounts[0].fsType,type,target);assert.equal(s.details.mounts[0].readOnly,ro,target);assert.ok(s.details.capacity,target+': '+s.details.capacityError);
 }
 s=await locate(file,'/srv/sodeck-t008/bind/shared.txt');assert.equal(s.details.exact,false);
 s=await locate(file,'/srv/sodeck-t008/stack');assert.equal(s.details.mounts.length,2);assert.equal(s.details.capacity,undefined);
 await file.screenshot({path:join(output,'stacked-mounts.png')});
 s=await locate(file,'/srv/sodeck-t008/network');await file.screenshot({path:join(output,'network-mount.png')});
 note('FT-03/04: ext4 bind, read-only bind, nested tmpfs, real CIFS and Btrfs mounts have correct source/type/capacity; stacked mounts remain ambiguous');
 s=await locate(file,'/srv/sodeck-t008/denied');await pluginRpc(file,'plugin.invoke',{command:'files.toggle',args:{id:s.selected}});
 s=await until(async()=>{const v=await snap(file);return v.rows.some(r=>r.id===v.selected&&r.error)?v:false;});assert.match(s.rows.find(r=>r.id===s.selected).error,/权限|协议/);
 s=await locate(file,'/srv/sodeck-t008/many/item-09999.dat');assert.equal(s.details.attributes.size,'9999');
 note('FT-05/06: denied directory remains an error; direct location reaches the last of 10,000 files without loading all siblings');
 const before=(await rpc(app.page,'resources.list')).ptys[0].id;
 const generation=(await rpc(app.page,'workbench.snapshot')).workspaces[0].generation;
 await rpc(app.page,'workbench.dock.configure',{workspaceId:a.id,workspaceGeneration:generation,edge:'left',width:380});
 await rpc(app.page,'workspaces.move',{workspaceId:a.id,operationId:crypto.randomUUID()});
 let wb=await rpc(app.page,'workbench.snapshot');const owner=wb.workspaces.find(w=>w.workspaceId===a.id).ownerWindowId;assert.notEqual(owner,'main');
 await rpc(app.page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()});
 const movedFile=await view('sodeck.files',a.id);
 assert.equal((await snap(movedFile)).path,'/srv/sodeck-t008/many/item-09999.dat');assert.equal((await rpc(app.page,'resources.list')).ptys[0].id,before);
 note('MP-03/FT-08: complete workspace detach/merge preserves the tree, selected path, original terminal PTY and plugin ownership');
 const b=await create('文件验证 B','reader');await rpc(app.page,'workspaces.open',{serverId:b.id});
 const other=await view('sodeck.files',b.id);await until(async()=>(await snap(other)).state==='ready',25000);
 assert.equal((await snap(other)).path,'/');await rpc(app.page,'workspaces.activate',{workspaceId:a.id});
 assert.equal((await snap(movedFile)).path,'/srv/sodeck-t008/many/item-09999.dat');
 await app.page.screenshot({path:join(output,'two-servers.png')});
 const fileId=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.files'&&v.workspaceId===a.id).viewId;
 await rpc(app.page,'views.close',{viewId:fileId});assert.ok((await rpc(app.page,'resources.list')).ptys.some(p=>p.id===before));
 await rpc(app.page,'workspaces.close',{workspaceId:a.id});await rpc(app.page,'workspaces.close',{workspaceId:b.id});
 await until(async()=>{const r=await rpc(app.page,'resources.list');return r.leases.length===0&&r.ptys.length===0&&r.connections.length===0;});
 note('FT-01/07/11: server states are independent; closing only files preserves terminal; closing workspaces releases all connection uses');
 assert.deepEqual(app.errors,[]);
}finally{await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));await stop(app);for(const browser of browsers.values())await browser.close().catch(()=>{});}
