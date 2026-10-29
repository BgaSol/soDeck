import assert from 'node:assert/strict';
import {mkdir,readdir,cp,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,stop} from './harness.mjs';
const output=resolve('.qa/t013/migration');await mkdir(output,{recursive:true});
const old=resolve('.local/dev/SoDeck-v2-T011');const data=join(output,'source-'+Date.now());let app;const results=[];
const packages=base=>{process.env.SODECK_PLUGIN_PACKAGES=['terminal','files'].map(p=>join(base,'resources',p+'.sodeck-plugin')).join(delimiter);};
try{
 packages(old);app=await launch({data,executable:join(old,'sodeck-desktop.exe'),cwd:old,shellPort:9831,pluginPort:9832});
 assert.ok(!(await rpc(app.page,'capabilities.list')).capabilities.includes('servers.routes'));
 const server=await rpc(app.page,'servers.create',{name:'旧版直连测试',host:'127.0.0.1',port:22313,username:'tester',authType:'key'});
 await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});
 try{await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...e.details});}
 await stop(app);app=undefined;results.push('Actual API 4.3 core wrote direct profile, DPAPI credential and host trust');
 packages(resolve('.'));app=await launch({data,shellPort:9831,pluginPort:9832});
 const upgraded=await rpc(app.page,'servers.routes.get',{serverId:server.id});assert.equal(upgraded.accessPath.kind,'direct');assert.equal(upgraded.hasSecret,true);
 const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});
 await stop(app);app=undefined;results.push('New core migrated the real old data and reused direct trust/credentials against OpenSSH');
 const backups=(await readdir(join(data,'core','backups'))).filter(n=>n.startsWith('before-routes-'));assert.equal(backups.length,1);
 const rollback=join(output,'rollback-'+Date.now());await cp(join(data,'core','backups',backups[0]),join(rollback,'core'),{recursive:true});
 packages(old);app=await launch({data:rollback,executable:join(old,'sodeck-desktop.exe'),cwd:old,shellPort:9831,pluginPort:9832});
 assert.equal((await rpc(app.page,'servers.list')).servers[0].id,server.id);
 const oldLease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id});await rpc(app.page,'connections.release',{leaseId:oldLease.leaseId});
 results.push('Migration backup restored into separate directory; actual old core connects successfully without reading new route data');
}finally{await stop(app);await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));console.log(results.join('\n'));}
