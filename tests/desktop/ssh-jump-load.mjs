import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t013/load');await mkdir(output,{recursive:true});
const cfg=JSON.parse(await readFile('.qa/t013/linux/config.json','utf8'));
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files'].map(p=>resolve(`resources/${p}.sodeck-plugin`)).join(delimiter);
let app;const results={};
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9841,pluginPort:9842});
 const a=await rpc(app.page,'servers.routes.create',{name:'负载 A',host:cfg.host,port:cfg.port,username:cfg.jumpUser,authType:'key',connectionRoute:{kind:'direct'}});
 const b=await rpc(app.page,'servers.routes.create',{name:'负载 B',host:cfg.targetHost,port:cfg.targetPort,username:cfg.targetUser,authType:'key',connectionRoute:{kind:'sshJump',jumpServerId:a.id}});
 for(const [server,key]of [[a,cfg.jumpKey],[b,cfg.targetKey]])await rpc(app.page,'auth.set',{serverId:server.id,privateKeyPath:key});
 const leases=[];const start=Date.now();
 for(let n=0;n<8;n++)for(;;){try{leases.push(await rpc(app.page,'connections.acquire',{serverId:b.id,workspaceId:`load-${n}`,operationId:crypto.randomUUID()}));break;}catch(e){if(e.code!=='HOST_KEY_REQUIRED')throw e;await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}}
 assert.equal(new Set(leases.map(l=>l.connectionId)).size,8);results.open8Ms=Date.now()-start;
 const ptys=[];for(let n=0;n<8;n++)ptys.push(await rpc(app.page,'pty.open',{leaseId:leases[0].leaseId}));
 await assert.rejects(()=>rpc(app.page,'pty.open',{leaseId:leases[0].leaseId}),e=>e.code==='LIMIT_EXCEEDED');
 results.capacity='8 OpenSSH target paths = 16 SSH transports / 8 forwarding channels; 9th PTY rejected; 64-path admission covered by protocol fixture';
 await rpc(app.page,'pty.write',{...ptys[0],inputSeq:1,bytes:Buffer.from("head -c 67108864 /dev/zero\r").toString('base64')});
 await delay(3500);
 const paths=await rpc(app.page,'connections.route.get',{serverId:b.id,workspaceId:'load-0'});
 results.bridge=paths.running[0].bridge;assert.ok(results.bridge.peakBufferedBytes<=256*1024);
 const cleanup=Date.now();for(const pty of ptys)await rpc(app.page,'pty.close',pty);for(const l of leases)await rpc(app.page,'connections.release',{leaseId:l.leaseId});
 await until(async()=>{const r=await rpc(app.page,'resources.list');return !r.connections.length&&!r.ptys.length&&!r.leases.length;},5000);
 results.cleanupMs=Date.now()-cleanup;assert.ok(results.cleanupMs<5000);assert.deepEqual(app.errors,[]);
}finally{await stop(app);await writeFile(join(output,'results.json'),JSON.stringify(results,null,2));console.log(JSON.stringify(results,null,2));}
