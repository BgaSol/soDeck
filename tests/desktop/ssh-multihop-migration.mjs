import assert from 'node:assert/strict';
import {mkdir,readdir,cp,readFile,writeFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,stop,until} from './harness.mjs';
const output=resolve('.qa/t013-multihop/migration');await mkdir(output,{recursive:true});
const old=resolve('.local/dev/SoDeck-v2-T013');let data=join(output,'source-'+Date.now());
const config=JSON.parse(await readFile('.qa/t013-multihop/linux/config.json','utf8'));
const packages=base=>{process.env.SODECK_PLUGIN_PACKAGES=['terminal','files'].map(p=>join(base,'resources',p+'.sodeck-plugin')).join(delimiter);};
const options={data};let app;const results=[];
async function freePort(){const listener=createServer();await new Promise(resolve=>listener.listen(0,'127.0.0.1',resolve));const port=listener.address().port;await new Promise(resolve=>listener.close(resolve));return port;}
async function start(extra={}){return launch({...options,...extra,shellPort:await freePort(),pluginPort:await freePort()});}
async function connect(server){for(let i=0;i<4;i++){try{return await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',e.details);}}throw Error('too many challenges');}
try{
 packages(old);app=await start({executable:join(old,'sodeck-desktop.exe'),packagePath:join(old,'resources/terminal.sodeck-plugin'),cwd:old});
 const caps=await rpc(app.page,'capabilities.list');assert.ok(caps.capabilities.includes('servers.routes'));assert.ok(!caps.capabilities.includes('servers.routes'));
 const nodes=[];
 for(let i=0;i<2;i++){const node=config.nodes[i];const server=await rpc(app.page,'servers.routes.create',{name:`迁移 ${node.name}`,host:node.host,port:i?2224:node.port,username:node.username,authType:'key',accessPath:i?{kind:'sshJump',jumpServerId:nodes[0].id}:{kind:'direct'}});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:node.key,remember:true});nodes.push(server);}
 let lease=await connect(nodes[1]);await rpc(app.page,'connections.release',{leaseId:lease.leaseId});await stop(app);app=undefined;
 const before=await readdir(join(data,'core','backups'));
 const upgradedData=join(output,'upgraded-'+Date.now());await cp(join(data,'core'),join(upgradedData,'core'),{recursive:true});data=upgradedData;options.data=data;
 results.push('Actual r1 binary created a one-jump route, protected credentials and host trust, and connected to OpenSSH');
 packages(resolve('.'));app=await start({});
 const migrated=await rpc(app.page,'servers.routes.get',{serverId:nodes[1].id});assert.equal(migrated.accessPath.jumpServerId,nodes[0].id);assert.equal(migrated.hasSecret,true);
 lease=await rpc(app.page,'connections.acquire',{serverId:nodes[1].id,workspaceId:nodes[1].id});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});await stop(app);app=undefined;
 const backups=(await readdir(join(data,'core','backups'))).filter(n=>!before.includes(n));assert.equal(backups.length,1);
 results.push('Schema 2→3 migration preserves existing single-jump route, trust and credentials; real connection needs no new prompt');
 // Check downgrade on a copy, then restore the consistent pre-migration backup separately.
 const downgrade=join(output,'downgrade-'+Date.now());await cp(data,downgrade,{recursive:true});
 const child=spawn(join(old,'sodeck-desktop.exe'),[],{cwd:old,windowsHide:true,env:{...process.env,SODECK_DATA_DIR:downgrade},stdio:['ignore','pipe','pipe']});let text='';child.stdout.on('data',v=>text+=v);child.stderr.on('data',v=>text+=v);
 try{await until(()=>child.exitCode!==null,10000);assert.notEqual(child.exitCode,0);assert.match(text,/UNSUPPORTED_DATA_VERSION|数据版本高于/);await writeFile(join(output,'downgrade.log'),text);}finally{if(child.exitCode===null)child.kill();}
 const rollback=join(output,'rollback-'+Date.now());await cp(join(data,'core','backups',backups[0]),join(rollback,'core'),{recursive:true});
 packages(old);app=await start({data:rollback,executable:join(old,'sodeck-desktop.exe'),packagePath:join(old,'resources/terminal.sodeck-plugin'),cwd:old});
 assert.equal((await rpc(app.page,'servers.routes.get',{serverId:nodes[1].id})).accessPath.jumpServerId,nodes[0].id);
 lease=await rpc(app.page,'connections.acquire',{serverId:nodes[1].id,workspaceId:nodes[1].id});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});
 results.push('Actual old binary refuses schema 3; restored backup opens and reconnects successfully with the old binary');
}finally{await stop(app);await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));console.log(results.join('\n'));}
