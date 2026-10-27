import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import net from 'node:net';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t014/load-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t014/linux/config.json','utf8'));
process.env.SODECK_PLUGIN_PACKAGES=resolve('resources/network-proxy.sodeck-plugin');process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9912});
let app,browser,reference,refErrors='';const sockets=[],metrics={flows:[]};
const invoke=(p,command,args={})=>pluginRpc(p,'plugin.invoke',{command,args});
async function fetchLarge(port){const time=performance.now();const r=await promisify(execFile)('curl.exe',['-sSf','--max-time','90','--noproxy','*',`http://127.0.0.1:${port}/large`],{encoding:'buffer',maxBuffer:70*1024*1024,windowsHide:true});assert.equal(r.stdout.length,64*1024*1024);return performance.now()-time;}
try {
  app=await launch({data:join(output,'data'),shellPort:9911,pluginPort:9912,packagePath:resolve('resources/network-proxy.sodeck-plugin'),allowStartupWarning:true});
  let target;for(const n of config.nodes){const s=await rpc(app.page,'servers.routes.v2.create',{name:n.name,host:n.host,port:n.port,username:n.username,authType:'key',connectionRoute:target?{kind:'sshJump',jumpServerId:target.id}:{kind:'direct'}});await rpc(app.page,'auth.set',{serverId:s.id,privateKeyPath:n.key,remember:false});target=s;}
  for(let i=0;i<5;i++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:target.id,workspaceId:target.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});break;}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
  await rpc(app.page,'workspaces.open',{serverId:target.id});const view=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9912'));const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin.localhost')));await page.getByRole('button',{name:'新建内网代理',exact:true}).waitFor();
  const rules=[];for(let i=0;i<16;i++){
    const rule=await invoke(page,'network.save',{expectedRevision:0,rule:{name:`并发规则 ${i}`,type:'port-forward',bindMode:'auto',port:0,autoReconnect:false,targetHost:'service-e.t014.invalid',targetPort:18080}});
    await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:view.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});rules.push(rule);
  }
  const snapshot=await invoke(page,'network.snapshot');const ports=snapshot.items.map(i=>i.run.address.port);assert.equal(ports.length,16);
  const latencies=[];
  for(let i=0;i<64;i++){
    const socket=net.connect(ports[i%16],'127.0.0.1');socket.on('error',()=>{});sockets.push(socket);
    const flow={index:i,bytes:0,started:Date.now()};metrics.flows.push(flow);socket.on('data',b=>{flow.bytes+=b.length;if(!flow.header)flow.header=b.subarray(0,100).toString();});socket.on('close',()=>flow.closed=Date.now());socket.on('error',e=>flow.error=e.code);
    await new Promise((r,j)=>{socket.once('connect',r);socket.once('error',j);});
    socket.write('GET /continuous HTTP/1.1\r\nHost: service-e.t014.invalid\r\nConnection: close\r\n\r\n');
    if(i<16){socket.pause();setTimeout(()=>socket.resume(),10000);}else socket.resume();
  }
  await until(async()=>(await invoke(page,'network.snapshot')).items.reduce((n,i)=>n+i.run.connections,0)===64,30000);
  const begin=Date.now();metrics.minimumConnections=64;
  while(Date.now()-begin<60000){const time=performance.now();const state=await invoke(page,'network.snapshot');latencies.push(performance.now()-time);assert.ok(state.items.every(i=>i.run.status==='ready'),JSON.stringify(state.items.map(i=>i.run)));metrics.minimumConnections=Math.min(metrics.minimumConnections,state.items.reduce((n,i)=>n+i.run.connections,0));if(metrics.minimumConnections<64)break;await delay(200);}
  assert.equal(metrics.minimumConnections,64);metrics.sustainedMs=Date.now()-begin;
  sockets.forEach(s=>s.destroy());await delay(500);
  metrics.controlP95=latencies.sort((a,b)=>a-b)[Math.floor(latencies.length*.95)];assert.ok(metrics.controlP95<=200,JSON.stringify(metrics));
  console.log('16 simultaneous rules / 64 TCP flows / 16 slow readers, 60 seconds; control p95 '+metrics.controlP95.toFixed(1)+' ms');
  const held=net.createServer();await new Promise(r=>held.listen(0,'127.0.0.1',r));const referencePort=held.address().port;await new Promise(r=>held.close(r));
  const sshConfig=config.nodes.map((n,i)=>`Host nt${i}\n HostName ${n.host}\n Port ${n.port}\n User ${n.username}\n IdentityFile "${n.key.replaceAll('\\','/')}"\n IdentitiesOnly yes\n StrictHostKeyChecking no\n UserKnownHostsFile NUL\n${i?` ProxyJump nt${i-1}\n`:''}`).join('\n');
  await writeFile(join(output,'ssh-config'),sshConfig);
  reference=spawn('C:/Windows/System32/OpenSSH/ssh.exe',['-F',join(output,'ssh-config'),'-N','-L',`127.0.0.1:${referencePort}:service-e.t014.invalid:18080`,'nt3'],{windowsHide:true,stdio:['ignore','ignore','pipe']});reference.stderr.on('data',b=>refErrors+=b);
  await until(()=>new Promise(r=>{const s=net.connect(referencePort,'127.0.0.1');s.once('connect',()=>{s.destroy();r(true);});s.once('error',()=>r(false));}),30000);
  metrics.reference64MiBMs=await fetchLarge(referencePort);metrics.splice64MiBMs=await fetchLarge(ports[0]);metrics.spliceThroughputRatio=metrics.reference64MiBMs/metrics.splice64MiBMs;
  assert.ok(metrics.spliceThroughputRatio>=.7,JSON.stringify(metrics));console.log('Native splice / OpenSSH reference throughput ratio '+metrics.spliceThroughputRatio.toFixed(2));
  for(const rule of rules){const time=performance.now();await invoke(page,'network.stop',{id:rule.id});assert.ok(performance.now()-time<1000);}
  await until(async()=>(await rpc(app.page,'net.resources.list')).items.length===0,5000);
  console.log('All load resources released; starting 100 lifecycle cycles with receipt-retention pacing');
  // Capacity intentionally includes operation tombstones. Respect its window;
  // retries never switch operation IDs after an unknown result.
  await rpc(app.page,'workspaces.close',{workspaceId:target.id});await stop(app);app=undefined;await browser.close();browser=undefined;
  // Persisted rules survive, the plugin process and resources start fresh.
  app=await launch({data:join(output,'data'),shellPort:9911,pluginPort:9912,packagePath:resolve('resources/network-proxy.sodeck-plugin'),allowStartupWarning:true});
  for(const n of config.nodes){const servers=await rpc(app.page,'servers.routes.v2.list');const s=servers.servers.find(s=>s.host===n.host);await rpc(app.page,'auth.set',{serverId:s.id,privateKeyPath:n.key,remember:false});}
  await rpc(app.page,'workspaces.open',{serverId:target.id});const next=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9912'));const p=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin.localhost')));await p.getByRole('button',{name:'新建',exact:true}).waitFor();
  const baseline=(await invoke(p,'network.snapshot')).items;assert.ok(baseline.every(i=>!i.run));
  metrics.cycles=0;metrics.maxStopMs=0;
  for(let i=0;i<100;i++){
    const time=Date.now();await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:next.viewId,operationId:crypto.randomUUID(),args:{id:rules[0].id}});
    const started=performance.now();await invoke(p,'network.stop',{id:rules[0].id});const duration=performance.now()-started;metrics.maxStopMs=Math.max(metrics.maxStopMs,duration);assert.ok(duration<1000);
    await until(async()=>(await rpc(app.page,'net.resources.list')).items.length===0,5000);metrics.cycles++;
    if(i%10===9)console.log('Lifecycle cycles '+metrics.cycles+'/100');await delay(Math.max(0,4000-(Date.now()-time)));
  }
}catch(error){if(app&&browser){const p=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin.localhost'));if(p){const s=await invoke(p,'network.snapshot').catch(()=>({items:[]}));metrics.failureState=s;metrics.failureDiagnostics=await Promise.all(s.items.map(i=>invoke(p,'network.diagnostics',{id:i.rule.id}).catch(()=>null)));}}throw error;}
finally{
  for(const s of sockets)s.destroy();
  // Windows OpenSSH ProxyJump spawns descendants. End the dedicated reference
  // tree, not only its parent, so test tunnels cannot outlive the load test.
  if(reference&&reference.exitCode===null){
    if(process.platform==='win32')await promisify(execFile)('taskkill.exe',['/PID',String(reference.pid),'/T','/F'],{windowsHide:true}).catch(()=>reference.kill());
    else reference.kill();
  }
  await stop(app);await browser?.close().catch(()=>{});await writeFile(join(output,'reference-stderr.log'),refErrors);await writeFile(join(output,'results.json'),JSON.stringify(metrics,null,2));await writeFile('.qa/t014/load-latest.json',JSON.stringify({output,metrics},null,2));
}
