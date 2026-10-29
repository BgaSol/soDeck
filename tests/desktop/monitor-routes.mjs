import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {monitorHarness,invoke,snapshot} from './monitor-harness.mjs';
import {rpc,until} from './harness.mjs';
const h=await monitorHarness('routes',19001),checks=[];let fixture,failure;const keys=new Map();
const connect=async server=>{for(let n=0;n<12;n++)try{return await rpc(h.app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(e){if(e.code==='AUTH_REQUIRED'){await rpc(h.app.page,'auth.ssh.set',{serverId:e.details.serverId,challengeId:e.details.challengeId,privateKeyPath:keys.get(e.details.serverId),remember:false});continue;}assert.equal(e.code,'HOST_KEY_REQUIRED',e.message);await rpc(h.app.page,'hosts.trust',{serverId:e.details.serverId??server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}throw Error('Host trust loop');};
try{
 const app=await h.start(),config=JSON.parse(await readFile('.qa/t024/multihop/linux/config.json','utf8')),nodes=[];
 assert.equal(config.adminPort,22423);
 for(const [i,n]of config.nodes.entries()){
  const s=await rpc(app.page,'servers.routes.create',{name:`T024 route ${n.name}`,host:n.host,port:n.port,username:n.username,authType:'key',accessPath:i===3?{kind:'hostTunnel',jumpServerIds:nodes.map(s=>s.id)}:{kind:'direct'}});
  await rpc(app.page,'auth.ssh.set',{serverId:s.id,privateKeyPath:n.key,remember:true});keys.set(s.id,n.key);nodes.push(s);
 }
 const target=nodes[3],lease=await connect(target);await rpc(app.page,'workspaces.open',{serverId:target.id});let page=await h.view('sodeck.monitor',target.id);
 await until(()=>snapshot(page),15000);await invoke(page,'monitor.setOptions',{options:{showVirtual:true,showLoopback:true}});
 const multi=await until(async()=>{const s=await snapshot(page);return s.cpu.quality==='fresh'&&s.network.value?.some(r=>r.name==='mh-d0')?s:false;},45000);
 const route=await rpc(app.page,'connections.route.get',{serverId:target.id,workspaceId:target.id});assert.equal(route.running[0].route.nodeCount,4);assert.equal(route.running[0].resources.bridges,3);
 await writeFile(join(h.output,'multihop.json'),JSON.stringify({snapshot:multi,route},null,2));await rpc(app.page,'connections.release',{leaseId:lease.leaseId});await rpc(app.page,'workspaces.close',{workspaceId:target.id});
 checks.push('real OpenSSH A→B→C→D: forwarding-only A/B/C, monitor sees final target mh-d0 network namespace');
 const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
 fixture=spawn(python,['-u','tests/core/socks_route_fixture.py',join(h.output,'socks')],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python'),SODECK_TEST_SOCKS_TARGET_PORT:'22342'}});
 let stderr='';fixture.stderr.on('data',b=>stderr+=b);const proxyConfig=await new Promise((r,j)=>{createInterface({input:fixture.stdout}).once('line',line=>r(JSON.parse(line)));fixture.once('exit',()=>j(Error(stderr)));});
 const proxy=await rpc(app.page,'proxies.socks5.create',{name:'T024 isolated SOCKS',host:'127.0.0.1',port:proxyConfig.port,authType:'none'});
 const server=await rpc(app.page,'servers.routes.create',{name:'T024 SOCKS target',host:'127.0.0.1',port:22342,username:'tester',authType:'key',accessPath:{kind:'networkProxy',proxyId:proxy.id,resolve:'proxy'}});
 await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:resolve('.qa/t008/linux/test-key'),remember:true});const sockLease=await connect(server);
 await rpc(app.page,'workspaces.open',{serverId:server.id});page=await h.view('sodeck.monitor',server.id);
 const sample=await until(async()=>{const s=await snapshot(page);return s.cpu.quality==='fresh'&&s.selectedMount?.capacity.quality==='fresh'?s:false;},45000);
 const socksRoute=await rpc(app.page,'connections.route.get',{serverId:server.id,workspaceId:server.id});assert.equal(socksRoute.running[0].route.accessMode,'networkProxy');assert.equal(socksRoute.running[0].resources.bridges,0);
 await writeFile(join(h.output,'socks.json'),JSON.stringify({snapshot:sample,route:socksRoute},null,2));await rpc(app.page,'connections.release',{leaseId:sockLease.leaseId});await rpc(app.page,'workspaces.close',{workspaceId:server.id});
 await until(async()=>(await rpc(app.page,'resources.list')).leases.length===0,15000);checks.push('real Linux metrics and capacity through SOCKS relay; all route test leases released');
}catch(error){failure={message:error.message,stack:error.stack};throw error;}
finally{await writeFile(join(h.output,'results.json'),JSON.stringify({checks,failure},null,2));await h.stop();if(fixture){fixture.stdin.end();await until(()=>fixture.exitCode!==null,5000).catch(()=>fixture.kill());}}
