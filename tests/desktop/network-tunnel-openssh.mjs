import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:http';
import {createHash} from 'node:crypto';
import {lookup} from 'node:dns/promises';
import net from 'node:net';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';

const output=resolve('.qa/t014/openssh-'+Date.now());await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t014/linux/config.json','utf8'));
const local=createServer((req,res)=>{const body='WINDOWS-REMOTE';res.end(body);});await new Promise(r=>local.listen(18081,'127.0.0.1',r));
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':9902,'sodeck.terminal':9903,'sodeck.files':9904});
let app,browser;const checks=[],metrics=[];
const note=text=>{checks.push(text);console.log(text);};
const curl=async(port,scheme,url,extra=[])=>{
  const started=performance.now();const {stdout}=await promisify(execFile)('curl.exe',['--fail','--silent','--show-error','--max-time','90','--noproxy','','--proxy',`${scheme}://127.0.0.1:${port}`,url,...extra],{windowsHide:true,encoding:'buffer',maxBuffer:70*1024*1024});return {bytes:stdout,ms:performance.now()-started};
};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
async function rawProxy(port,request,body=Buffer.alloc(0)){
  return new Promise((resolve,reject)=>{
    const socket=net.connect(port,'127.0.0.1'),chunks=[];
    socket.setTimeout(10000,()=>socket.destroy(Error('Proxy response timed out')));
    socket.on('error',reject);socket.on('data',b=>chunks.push(b));socket.on('end',()=>resolve(Buffer.concat(chunks)));
    socket.on('connect',()=>{socket.write(request);if(body.length)socket.write(body);});
  });
}
async function create(node,jump){const server=await rpc(app.page,'servers.routes.create',{name:node.name,host:node.host,port:node.port,username:node.username,authType:'key',accessPath:jump?{kind:'sshJump',jumpServerId:jump.id}:{kind:'direct'}});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:node.key,remember:false});return server;}
async function trusted(server){for(let i=0;i<8;i++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});return;}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED',e.message);await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}throw Error('Trust did not finish');}
async function workspace(server){await trusted(server);await rpc(app.page,'workspaces.open',{serverId:server.id});const view=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});browser??=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9902'));const page=await until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId)===view.viewId)return p;});await page.getByRole('button',{name:'创建',exact:true}).waitFor();return {server,view,page};}
async function start(w,type='network-proxy',targetHost='',targetPort=0){const rule=await invoke(w.page,'network.save',{expectedRevision:0,rule:{name:type==='network-proxy'?'远端内网':'固定服务',type,bindMode:'auto',port:0,autoReconnect:false,...(type==='port-forward'?{targetHost,targetPort}:{})}});await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:w.view.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});const s=await invoke(w.page,'network.snapshot');return {rule,port:s.items.find(i=>i.rule.id===rule.id).run.address.port};}
try {
  app=await launch({data:join(output,'data'),shellPort:9901,pluginPort:9902});
  const nodes=[];for(const node of config.nodes)nodes.push(await create(node,nodes.at(-1)));
  const w=await workspace(nodes.at(-1)),proxy=await start(w);
  for(const host of ['service-e.t014.invalid','service-f.t014.invalid'])await assert.rejects(()=>lookup(host));
  assert.equal((await curl(proxy.port,'http','http://service-e.t014.invalid:18080/')).bytes.toString(),'REMOTE-E\n');
  assert.equal((await curl(proxy.port,'socks5h','http://service-f.t014.invalid:18080/')).bytes.toString(),'REMOTE-F\n');
  const secure=await curl(proxy.port,'http','https://service-e.t014.invalid:18443/',['--cacert',resolve('.qa/t014/linux/cert.pem')]);assert.equal(secure.bytes.toString(),'REMOTE-E\n');
  const sse=await curl(proxy.port,'http','http://service-e.t014.invalid:18080/events');assert.ok(sse.bytes.toString().includes('data: 2'));
  note('Real Debian OpenSSH A/B/C/D: HTTP, HTTPS CONNECT with certificate validation, SOCKS5 remote DNS and SSE reach separate LAN E/F');
  const early=await rawProxy(proxy.port,'POST http://service-e.t014.invalid:18080/early HTTP/1.1\r\nHost: service-e.t014.invalid:18080\r\nContent-Length: 67108864\r\n\r\n',Buffer.alloc(65536,120));
  assert.match(early.toString(),/^HTTP\/1\.1 413 /);
  const upgrade=await rawProxy(proxy.port,'GET http://service-e.t014.invalid:18080/upgrade HTTP/1.1\r\nHost: service-e.t014.invalid:18080\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n');
  assert.match(upgrade.toString(),/^HTTP\/1\.1 101 /);assert.ok(upgrade.toString().endsWith('UPGRADE-TAIL'));
  note('Real HTTP path returns an early 413 before the declared 64 MiB upload completes; 101 upgrade preserves coalesced trailing bytes');
  const chromiumBrowser=await chromium.launch({headless:true,proxy:{server:`http://127.0.0.1:${proxy.port}`,bypass:'<-loopback>'}});
  try{const p=await chromiumBrowser.newPage();await p.goto('http://service-e.t014.invalid:18080/');assert.match(await p.locator('body').innerText(),/REMOTE-E/);await p.goto('http://service-f.t014.invalid:18080/');assert.match(await p.locator('body').innerText(),/REMOTE-F/);}finally{await chromiumBrowser.close();}
  const decoy=createServer((req,res)=>res.end('LOCAL-DECOY'));let decoyHits=0;decoy.on('request',()=>decoyHits++);await new Promise(r=>decoy.listen(18080,'127.0.0.1',r));
  try {assert.equal((await curl(proxy.port,'http','http://127.0.0.1:18080/')).bytes.toString(),'REMOTE-D-LOOPBACK\n');assert.equal(decoyHits,0);}finally{await new Promise(r=>decoy.close(r));}
  note('Real browser switches remote LAN targets with one proxy; identical loopback address reaches remote marker, never local decoy');
  const aw=await workspace(nodes[0]),ap=await start(aw);
  const isolated=await start(aw,'port-forward','198.18.14.18',18080),lan=await start(w,'port-forward','198.18.14.18',18080);
  const directText=async port=>(await promisify(execFile)('curl.exe',['-sSf','--max-time','15','--noproxy','*',`http://127.0.0.1:${port}/`],{windowsHide:true})).stdout;
  assert.equal(await directText(isolated.port),'REMOTE-A-ISOLATED\n');assert.equal(await directText(lan.port),'REMOTE-E\n');
  assert.equal((await curl(ap.port,'http','http://198.18.14.18:18080/')).bytes.toString(),'REMOTE-A-ISOLATED\n');
  assert.equal((await curl(proxy.port,'http','http://198.18.14.18:18080/')).bytes.toString(),'REMOTE-E\n');
  await invoke(aw.page,'network.stop',{id:isolated.rule.id});assert.equal(await directText(lan.port),'REMOTE-E\n');
  await rpc(app.page,'workspaces.close',{workspaceId:aw.server.id});
  note('Two remote network contexts with identical 198.18.14.18:18080 remain isolated for both fixed forwarding and HTTP proxy; stopping one leaves the other usable');
  const fixed=await start(w,'port-forward','service-e.t014.invalid',18080);
  const large=await curl(proxy.port,'http','http://service-e.t014.invalid:18080/large');assert.equal(large.bytes.length,64*1024*1024);
  const pattern=Buffer.from(Array.from({length:256},(_,i)=>i)),hash=createHash('sha256');for(let i=0;i<262144;i++)hash.update(pattern);assert.equal(createHash('sha256').update(large.bytes).digest('hex'),hash.digest('hex'));
  const directStart=performance.now(),direct=await promisify(execFile)('curl.exe',['--silent','--fail','--noproxy','*',`http://127.0.0.1:${fixed.port}/large`],{windowsHide:true,encoding:'buffer',maxBuffer:70*1024*1024});assert.equal(direct.stdout.length,large.bytes.length);const spliceMs=performance.now()-directStart;
  metrics.push({http64MiBMs:large.ms,splice64MiBMs:spliceMs,httpThroughputRatio:spliceMs/large.ms});
  assert.ok(spliceMs/large.ms>=.25,JSON.stringify(metrics));
  note('64 MiB HTTP and native splice hashes match; ordinary HTTP meets 25% of splice throughput');
  const linuxCheck=await invoke(w.page,'network.check',{id:proxy.rule.id,host:'service-e.t014.invalid',port:18080,protocol:'socks'});assert.equal(linuxCheck.state,'succeeded');
  await delay(1200);const snapshot=await invoke(w.page,'network.snapshot');assert.ok(snapshot.items.find(i=>i.rule.id===proxy.rule.id).run.bytesRead>0);
  await w.page.screenshot({path:join(output,'linux-ready.png')});
  const windowsNode={name:'Windows OpenSSH 临时入口',host:'127.0.0.1',port:22327,username:process.env.USERNAME,key:resolve('.qa/t014/windows/client-clear')};
  const windows=await create(windowsNode),ww=await workspace(windows),wp=await start(ww);
  assert.equal((await curl(wp.port,'http','http://127.0.0.1:18081/')).bytes.toString(),'WINDOWS-REMOTE');
  assert.equal((await curl(wp.port,'socks5h','http://127.0.0.1:18081/')).bytes.toString(),'WINDOWS-REMOTE');
  const wf=await start(ww,'port-forward','127.0.0.1',18081);assert.equal(await directText(wf.port),'WINDOWS-REMOTE');
  note('Real Win32-OpenSSH dedicated loopback process: HTTP and SOCKS5 target connections; existing host SSH service untouched');
  const mixed=[];for(const node of config.nodes)mixed.push(await create(node,mixed.at(-1)??windows));
  const mw=await workspace(mixed.at(-1)),mp=await start(mw);assert.equal((await curl(mp.port,'http','http://service-e.t014.invalid:18080/')).bytes.toString(),'REMOTE-E\n');
  note('Windows intermediate node → Linux A/B/C/D → remote LAN target succeeds');
  await rpc(app.page,'workspaces.close',{workspaceId:mw.server.id});await rpc(app.page,'workspaces.close',{workspaceId:ww.server.id});await rpc(app.page,'workspaces.close',{workspaceId:w.server.id});
  await until(async()=>(await rpc(app.page,'net.resources.list')).items.length===0,15000);
  await until(async()=>(await rpc(app.page,'resources.list')).connections.length===0,15000);
  note('All workspace closures release network, terminal/file connections and listeners');
}finally{await stop(app);await browser?.close().catch(()=>{});await new Promise(r=>local.close(r));await writeFile(join(output,'results.json'),JSON.stringify({checks,metrics},null,2));await writeFile('.qa/t014/openssh-latest.json',JSON.stringify({output,checks,metrics},null,2));}
