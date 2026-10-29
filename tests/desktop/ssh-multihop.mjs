import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,delimiter} from 'node:path';
import {lookup} from 'node:dns/promises';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';

const output=resolve(process.env.MH_OUTPUT??'.qa/t013-multihop/native');await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile(process.env.MH_CONFIG??'.qa/t013-multihop/linux/config.json','utf8'));
assert.equal(config.adminPort,22323);
const old=process.env.MH_OLD==='1',filesOnly=process.env.MH_FILES_ONLY==='1';
const packages=old?'.qa/t013/baseline':'resources';
process.env.SODECK_PLUGIN_PACKAGES=(filesOnly?['files']:['terminal','files']).map(p=>resolve(packages,`${p}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9862,'sodeck.terminal':9863});
const results=[],browsers=[];let app;
const note=value=>{results.push(value);console.log(value);};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const snapshot=page=>invoke(page,'files.snapshot',{count:100});
const screen=name=>app.page.screenshot({path:join(output,`${name}.png`)});
async function plugin(port,workspace){const browser=await until(()=>chromium.connectOverCDP(`http://127.0.0.1:${port}`),30000);browsers.push(browser);return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===workspace)return p;},30000);}
async function create(node,jump,name=node.name){return rpc(app.page,'servers.routes.create',{name,host:node.host,port:node.port,username:node.username,authType:'key',accessPath:jump?{kind:'sshJump',jumpServerId:jump.id}:{kind:'direct'}});}
async function credential(server,node,encrypted=false){await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:encrypted?node.encryptedKey:node.key,passphrase:encrypted?'fixture-passphrase':'',remember:false});}
async function connect(server,count){for(let i=0;i<=count;i++){try{return await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED',e.message);assert.equal(e.details.hopCount,count);await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}}throw Error('Path did not finish');}
async function release(lease){await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}
async function probe(server,count){const lease=await connect(server,count);const pty=await rpc(app.page,'pty.open',{leaseId:lease.leaseId});await rpc(app.page,'pty.write',{...pty,inputSeq:1,bytes:Buffer.from("printf 'MH%s\\n' _OPENSSH_PROOF\r").toString('base64')});await until(async()=>{const r=await rpc(app.page,'pty.read',{...pty,after:0});return r.frames.some(f=>Buffer.from(f.bytes,'base64').toString().includes('MH_OPENSSH_PROOF'));},30000);await rpc(app.page,'pty.close',pty);await release(lease);}
async function edit(server,changes){const {id,revision,hasSecret,...body}=server;return rpc(app.page,'servers.routes.update',{...body,...changes,serverId:id,expectedRevision:revision});}
try{
 for(const n of config.nodes.slice(1))await assert.rejects(()=>lookup(n.host));
 app=await launch({data:join(output,`${old?'old':filesOnly?'files-only':'current'}-${Date.now()}`),shellPort:9861,pluginPort:9862,allowStartupWarning:filesOnly,packagePath:resolve(packages,`${filesOnly?'files':'terminal'}.sodeck-plugin`)});
 assert.ok((await rpc(app.page,'capabilities.list')).capabilities.includes('connections.route'));
 const installed=await rpc(app.page,'plugins.list');
 if(filesOnly)assert.deepEqual(installed.map(p=>p.pluginId),['sodeck.files']);
 if(old){assert.equal(installed.find(p=>p.pluginId==='sodeck.terminal').activeVersion,'0.6.0');assert.equal(installed.find(p=>p.pluginId==='sodeck.files').activeVersion,'0.3.2');}
 if(old)await writeFile(join(output,'old-installed.json'),JSON.stringify(installed,null,2));
 const nodes=[];
 for(const [i,node]of config.nodes.entries()){const server=await create(node,nodes.at(-1),`${node.name} · 多级验证`);await credential(server,node,i%2===1);nodes.push(server);}
 const target=nodes.at(-1);
 if(!old&&!filesOnly){
   const directNode={host:'127.0.0.1',port:22323,username:'tester',key:resolve('.qa/t008/linux/test-key'),name:'直连回归'};
   const direct=await create(directNode);await credential(direct,directNode);await probe(direct,1);
   for(const count of [2,3]){const node={...config.nodes[count-1],port:2224};const endpoint=await create(node,nodes[count-2],`${count-1} 层跳板目标`);await credential(endpoint,node);await probe(endpoint,count);}
   await probe(target,4);note('OpenSSH: direct, one, two and three jumps execute real shell output; private names are unavailable on client');
 }else await release(await connect(target,4));
 await rpc(app.page,'workspaces.open',{serverId:target.id});
 if(filesOnly){const wb=await rpc(app.page,'workbench.snapshot');assert.ok(wb.views.every(v=>v.pluginId==='sodeck.files'));assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);}
 const files=await plugin(9862,target.id);await until(()=>snapshot(files));
 if(filesOnly)await rpc(app.page,'commands.execute',{pluginId:'sodeck.files',commandId:'files.connect',workspaceId:target.id,operationId:crypto.randomUUID()});
 await until(async()=>(await snapshot(files)).state==='ready',45000);
 const proof=`/home/${config.nodes.at(-1).username}/proof`;await invoke(files,'files.locate',{text:proof});
 const s=await until(async()=>{const s=await snapshot(files);return s.path===proof&&!s.loading?s:false;});
 await invoke(files,'files.toggle',{id:s.selected});await until(async()=>(await snapshot(files)).rows.some(r=>r.name==='node.txt'));
 await files.screenshot({path:join(output,`${old?'old-':filesOnly?'only-':''}files.png`)});
 note(`${old?'Unrebuilt API 4 plugins':filesOnly?'Files alone':'Current plugins'}: target directory, stat and mount/exec work through A/B/C`);
 if(!filesOnly){
   const terminal=await plugin(9863,target.id);await terminal.locator('.xterm-helper-textarea').waitFor();
   await terminal.locator('.xterm-helper-textarea').pressSequentially("cd /tmp; printf 'MH%s\\n' _NATIVE_OK\r");
   await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('MH_NATIVE_OK'));
   if(!old)await until(async()=>(await snapshot(files)).path==='/tmp',30000);
   await terminal.screenshot({path:join(output,`${old?'old-':''}terminal.png`)});
   note('Native terminal input/output and target cwd tracking succeeded through three jumps');
 }
 if(!old&&!filesOnly){
   const before=(await rpc(app.page,'resources.list')).ptys;
   nodes[1]=await edit(nodes[1],{name:'B · 中间配置已修改'});
   await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()});
   await until(async()=>(await snapshot(files)).follow.message==='连接不同，暂不跟随',30000);
   await rpc(app.page,'views.activate',{viewId:before[0].viewId});await until(async()=>(await snapshot(files)).follow.message!=='连接不同，暂不跟随');
   const dependencies=await rpc(app.page,'servers.routes.dependencies',{serverId:nodes[0].id});
   assert.ok(dependencies.dependencies.some(d=>d.kind==='saved'&&d.server.id===target.id&&!d.direct));
   assert.ok(dependencies.dependencies.filter(d=>d.kind==='running').length>=2);
   await assert.rejects(()=>rpc(app.page,'servers.delete',{serverId:nodes[0].id,stopSessions:true}),e=>e.code==='SERVER_DEPENDENTS');
   note('Middle configuration changes preserve old connections; transitive dependencies and cwd connection isolation are enforced');
   await app.page.locator('.connection-status').click();await app.page.getByRole('button',{name:'查看完整路径',exact:true}).first().click();await app.page.getByText('4/4 · 目标 · D · 多级验证',{exact:true}).waitFor();await screen('path-details');await app.page.getByRole('button',{name:'关闭对话框'}).click();
   const ids=(await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort();
   await rpc(app.page,'workspaces.move',{workspaceId:target.id,operationId:crypto.randomUUID()});
   const wb=await rpc(app.page,'workbench.snapshot'),owner=wb.workspaces.find(w=>w.workspaceId===target.id).ownerWindowId;
   await rpc(app.page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()});
   assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),ids);
   const main=(await nativeWindow(app,'list')).find(w=>w.visible);await nativeWindow(app,'close',{WindowHandle:main.handle});await delay(250);assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),ids);await nativeWindow(app,'show',{WindowHandle:main.handle});
   note('Workspace detach/merge and tray hide/restore retain the entire multihop path');
   for(const mode of ['dopamine','light','dark']){const preference=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:preference.revision});await delay(200);await screen(`workspace-${mode}`);}
 }
 await rpc(app.page,'workspaces.close',{workspaceId:target.id});
 await until(async()=>{const r=await rpc(app.page,'connections.route.list');return r.usage.sessions===0&&r.usage.bridges===0;},5000);
 assert.equal((await rpc(app.page,'resources.list')).ptys.length,0);assert.deepEqual(app.errors,[]);
 note('Closing the target workspace releases all target and intermediate transports; no page errors');
}finally{for(const b of browsers)await b.close().catch(()=>{});await stop(app);await writeFile(join(output,`${old?'old-':filesOnly?'files-only-':''}results.json`),JSON.stringify(results,null,2));}
