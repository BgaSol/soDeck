import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {lookup} from 'node:dns/promises';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve('.qa/t013/native');await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t013/linux/config.json','utf8'));
assert.equal(config.host,'127.0.0.1');assert.equal(config.port,22314);
const old=process.env.T013_OLD==='1',fileOnly=process.env.T013_FILES_ONLY==='1';
const packages=old?'.qa/t013/baseline':'resources';
process.env.SODECK_PLUGIN_PACKAGES=(fileOnly?['files']:['terminal','files']).map(p=>resolve(packages,`${p}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9812,'sodeck.terminal':9813});
const results=[],browsers=[];let app;
const note=text=>{results.push(text);console.log(text);};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const snapshot=page=>invoke(page,'files.snapshot',{count:300});
async function plugin(port,workspace){const browser=await until(()=>chromium.connectOverCDP(`http://127.0.0.1:${port}`),30000);browsers.push(browser);return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.workspaceId)===workspace)return p;},30000);}
async function create(name,authType,jump){return rpc(app.page,'servers.routes.create',{name,host:jump?config.targetHost:config.host,port:jump?config.targetPort:config.port,username:jump?config.targetUser:config.jumpUser,authType,connectionRoute:jump?{kind:'sshJump',jumpServerId:jump.id}:{kind:'direct'}});}
async function authenticate(server,key,password,encrypted=false){return rpc(app.page,'auth.set',{serverId:server.id,remember:false,...(key?{privateKeyPath:key,passphrase:encrypted?'fixture-passphrase':''}:{password})});}
async function connect(server){for(let i=0;i<5;i++){try{return await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(e){if(!['HOST_KEY_REQUIRED','HOST_KEY_CHANGED'].includes(e.code))throw e;await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}}throw Error('Too many trust challenges');}
async function release(lease){await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}
async function edit(server,changes){const {id,revision,hasSecret,...body}=server;return rpc(app.page,'servers.routes.update',{...body,...changes,serverId:id,expectedRevision:revision});}
async function screen(name){await app.page.screenshot({path:join(output,name+'.png')});}
try{
 await assert.rejects(()=>lookup(config.targetHost));note('JP-03: client DNS cannot resolve target.t013; target has no host forwarded port');
 app=await launch({data:join(output,`${old?'old':fileOnly?'files-only':'current'}-${Date.now()}`),shellPort:9811,pluginPort:9812,allowStartupWarning:fileOnly});
 if(!old&&!fileOnly){
   for(const [a,b,encrypted]of [['password','password',false],['key','password',false],['password','key',true],['key','key',true]]){
     const gateway=await create(`认证 ${a}`,a),target=await create(`认证 ${b}`,b,gateway);
     await authenticate(gateway,a==='key'?(encrypted?config.jumpEncryptedKey:config.jumpKey):null,'jump-fixture-password',encrypted);
     await authenticate(target,b==='key'?(encrypted?config.targetEncryptedKey:config.targetKey):null,'target-fixture-password',encrypted);
     const lease=await connect(target);const pty=await rpc(app.page,'pty.open',{leaseId:lease.leaseId});
     await rpc(app.page,'pty.write',{...pty,inputSeq:1,bytes:Buffer.from("printf 'T013_AUTH_OK\\n'\r").toString('base64')});
     await until(async()=>{const r=await rpc(app.page,'pty.read',{...pty,after:0});return r.frames.some(f=>Buffer.from(f.bytes,'base64').toString().includes('T013_AUTH_OK'));});
     await rpc(app.page,'pty.close',pty);await release(lease);
   }
   note('JP-04: OpenSSH password/password, key/password, password/encrypted-key and encrypted-key/encrypted-key succeeded');
 }
 let gateway=await create('入口机 A','key');const target=await create('内网服务器 B','key',gateway);
 await authenticate(gateway,config.jumpKey);await authenticate(target,config.targetKey);await release(await connect(target));
 await rpc(app.page,'workspaces.open',{serverId:target.id});
 const file=await plugin(9812,target.id);
 await until(()=>snapshot(file));
 if(fileOnly)await rpc(app.page,'commands.execute',{pluginId:'sodeck.files',commandId:'files.connect',workspaceId:target.id,operationId:crypto.randomUUID()});
 await until(async()=>(await snapshot(file)).state==='ready',30000);
 await invoke(file,'files.locate',{text:'/home/target13/jump-proof'});
 let s=await until(async()=>{const s=await snapshot(file);return s.path==='/home/target13/jump-proof'&&!s.loading?s:false;},30000);
 await invoke(file,'files.toggle',{id:s.selected});
 await until(async()=>(await snapshot(file)).rows.some(r=>r.name==='proof.txt'));
 await file.screenshot({path:join(output,(old?'old-':fileOnly?'only-':'')+'files.png')});
 note(`${old?'Old API 4 packages':fileOnly?'File plugin alone':'Current packages'}: target SFTP tree and mount/exec query work through gateway`);
 if(!fileOnly){
   const terminal=await plugin(9813,target.id);await terminal.locator('.xterm-helper-textarea').waitFor();
   await terminal.locator('.xterm-helper-textarea').pressSequentially("cd /tmp; printf 'T013_NATIVE_OK\\n'\r");
   await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('T013_NATIVE_OK'));
   if(!old)await until(async()=>(await snapshot(file)).path==='/tmp',30000);
   await terminal.screenshot({path:join(output,(old?'old-':'')+'terminal.png')});
   note('JP-03/07: terminal I/O and file consumers share the same target path');
   if(!old){
     const original=(await rpc(app.page,'resources.list')).ptys.find(p=>p.serverId===target.id);
     gateway=await edit(gateway,{name:'入口机 A · 已修改'});
     await rpc(app.page,'commands.execute',{pluginId:'sodeck.terminal',commandId:'terminal.new',workspaceId:target.id,operationId:crypto.randomUUID()});
     await until(async()=>(await snapshot(file)).follow.message==='连接不同，暂不跟随',30000);
     assert.equal((await snapshot(file)).path,'/tmp');
     await rpc(app.page,'views.activate',{viewId:original.viewId});
     await until(async()=>(await snapshot(file)).follow.message!=='连接不同，暂不跟随',30000);
     note('JP-09: saved route changes create a new connection; file follow refuses another connection and resumes on old matching terminal');
     const deps=await rpc(app.page,'servers.routes.dependencies',{serverId:gateway.id});assert.equal(deps.saved.length,1);assert.equal(deps.running.length,2);
     await assert.rejects(()=>rpc(app.page,'servers.delete',{serverId:gateway.id,stopSessions:true}),e=>e.code==='SERVER_DEPENDENTS');
     await app.page.getByRole('button',{name:'运行资源',exact:true}).click();await screen('resources');await app.page.getByRole('button',{name:'关闭对话框'}).click();
     const before=(await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort();
     await rpc(app.page,'workspaces.move',{workspaceId:target.id,operationId:crypto.randomUUID()});
     const wb=await rpc(app.page,'workbench.snapshot'),owner=wb.workspaces.find(w=>w.workspaceId===target.id).ownerWindowId;
     await rpc(app.page,'windows.merge',{sourceWindowId:owner,targetWindowId:'main',operationId:crypto.randomUUID()});
     assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),before);
     note('JP-02/11: saved/runtime dependencies are listed; deletion blocked; detach/merge preserves PTYs and path');
     const main=(await nativeWindow(app,'list')).find(w=>w.visible);
     await nativeWindow(app,'close',{WindowHandle:main.handle});await delay(300);
     assert.deepEqual((await rpc(app.page,'resources.list')).ptys.map(p=>p.id).sort(),before);
     await nativeWindow(app,'show',{WindowHandle:main.handle});
     note('JP-11: hiding the main window to tray and restoring it preserves target PTYs');
   }
 }
 if(!old&&!fileOnly){
   for(const mode of ['light','dark']){
     const preference=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.select',{mode,expectedRevision:preference.revision});await delay(300);await screen('workspace-'+mode);
   }
   const calls=await rpc(app.page,'calls.list',{method:'connections.route.stage',limit:100});
   assert.ok((calls.records??calls.calls).some(c=>c.summary.hopIndex===1&&c.summary.stage==='authenticate'));
   assert.ok(!(JSON.stringify(calls).includes('fixture-password')));note('JP-15: internal stages are correlated to origin operations and contain no fixture passwords');
 }
 await rpc(app.page,'workspaces.close',{workspaceId:target.id});
 await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.connections.length&&!r.ptys.length&&!r.leases.length;});
 assert.deepEqual(app.errors,[]);note('JP-11: workspace close returns all target leases, PTYs and connections to zero');
}finally{await writeFile(join(output,(old?'old-':fileOnly?'files-only-':'')+'results.json'),JSON.stringify(results,null,2));await stop(app);for(const b of browsers)await b.close().catch(()=>{});}
