import assert from 'node:assert/strict';
import {mkdir,writeFile,readFile,mkdtemp,copyFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve,join,delimiter} from 'node:path';
import {build} from 'esbuild';
import {launch,rpc,until,stop,delay} from './harness.mjs';
const output=resolve('.qa/t016/performance');await mkdir(output,{recursive:true});
const packages=['terminal','files','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`));
const ids=Array.from({length:16},(_,n)=>`fixture.observation.${n}`);
for(const [n,pluginId]of ids.entries()){
 const source=join(output,`producer-${n}`);await mkdir(join(source,'ui'),{recursive:true});await mkdir(join(source,'diagnostics'),{recursive:true});
 const type=`plugin.${pluginId}.sample`,descriptor={schemaVersion:1,pluginId,events:[{type,typeVersion:1,kind:'domain',fields:{sample:{type:'integer',classification:'count',minimum:0}}}],actions:[],snapshots:[],metrics:[]};
 const manifest={pluginId,version:'1.0.0',requires:{core:'>=2.0.0, <3.0.0',pluginApi:'^5.0.0',platforms:['windows'],capabilities:['observability.events.v2','observability.context.v1','observability.snapshots.v1','observability.metrics.v1','observability.history-feed.v1']},entry:{ui:'ui/index.html',worker:'worker/main.js'},dataSchemaVersion:1,contributes:{views:[],commands:['load.start','load.results'].map(id=>({id,title:id,scope:'global'}))},observability:{descriptorVersion:1,path:'diagnostics/observability.json'}};
 await writeFile(join(source,'ui/index.html'),'<!doctype html><title>Observation test fixture</title>');await writeFile(join(source,'manifest.json'),JSON.stringify(manifest));await writeFile(join(source,'diagnostics/observability.json'),JSON.stringify(descriptor));
 await build({entryPoints:['tests/desktop/observation-load-worker.ts'],bundle:true,format:'iife',platform:'neutral',target:'es2022',outfile:join(source,'worker/main.js'),define:{__LOAD_PLUGIN__:JSON.stringify(pluginId)}});
 const file=join(output,`producer-${n}.sodeck-plugin`);execFileSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','terminal','--source',source,'--out',file],{windowsHide:true,stdio:'pipe'});packages.push(file);
}
process.env.SODECK_PLUGIN_PACKAGES=packages.join(delimiter);process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':9462,'sodeck.files':9463,'sodeck.network-proxy':9464});
const fixture=JSON.parse(await readFile('.qa/t016/config.json','utf8')).nodes.find(n=>n.platform==='linux');
const run=await mkdtemp(join(output,'run-'));let data=join(run,'data'),app;const measurements=[];
let executable;
if(process.env.SODECK_PERF_EXECUTABLE){
 executable=join(run,'sodeck-desktop.exe');await copyFile(resolve(process.env.SODECK_PERF_EXECUTABLE),executable);
 await writeFile(join(run,'portable.json'),JSON.stringify({schemaVersion:1}));await mkdir(join(run,'resources'));
 for(const name of ['development-public-key.json','terminal.sodeck-plugin','files.sodeck-plugin','network-proxy.sodeck-plugin'])await copyFile(resolve('resources',name),join(run,'resources',name));
 await writeFile(join(run,'resources/bundled-plugins.json'),JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:['terminal','files','network-proxy'].map(n=>({pluginId:'sodeck.'+n,path:n+'.sodeck-plugin'}))}));
}
const command=(pluginId,commandId,args={})=>rpc(app.page,'commands.execute',{pluginId,commandId,operationId:crypto.randomUUID(),args});
const percentile=(values,p)=>values.toSorted((a,b)=>a-b)[Math.min(values.length-1,Math.ceil(values.length*p)-1)];
try{
 if(executable){app=await launch({data,shellPort:9461,pluginPort:9462});assert.equal((await rpc(app.page,'observability.describe')).sources.length,19);await stop(app);app=undefined;}
 app=await launch({data,shellPort:9461,pluginPort:9462,...(executable?{executable,ordinary:true,bundledResources:true}:{})});
 assert.equal((await rpc(app.page,'observability.describe')).sources.length,19);
 const server=await rpc(app.page,'servers.create',{name:'T016 performance',host:fixture.host,port:fixture.port,username:fixture.username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:server.id,privateKeyPath:fixture.key});
 const acquire=()=>rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});
 let lease;try{lease=await acquire();}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,...e.details});lease=await acquire();}
 await rpc(app.page,'workspaces.open',{serverId:server.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.length>=1,45000);
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:server.id,operationId:crypto.randomUUID()});
 for(const enabled of [false,true]){
   const at=Date.now()+10000,duration=60000;
   for(const plugin of ids)await command(plugin,'load.start',{at,duration,enabled});
   await delay(Math.max(0,at-Date.now()));
   const pty=await rpc(app.page,'pty.open',{leaseId:lease.leaseId,cols:80,rows:24});await rpc(app.page,'pty.write',{ptyId:pty.ptyId,generation:pty.generation,inputSeq:1,bytes:Buffer.from('stty -echo -onlcr; head -c 1073741824 /dev/zero\n').toString('base64')});
   let after=0,bytes=0,reads=0;const began=Date.now(),health=[];let nextHealth=began+5000;
   while(Date.now()<at+duration){const page=await rpc(app.page,'pty.read',{ptyId:pty.ptyId,generation:pty.generation,after});for(const frame of page.frames){bytes+=Buffer.from(frame.bytes,'base64').length;after=frame.sequence;}if(page.frames.length)await rpc(app.page,'pty.ack',{ptyId:pty.ptyId,generation:pty.generation,sequence:after});else await delay(5);reads++;
     if(Date.now()>=nextHealth){const h=await rpc(app.page,'observability.v2.health.get');assert.ok(h.queueBytes<=h.queueBudgetBytes);assert.ok(h.producerCredits.grantedBytes<=h.producerCredits.budgetBytes);health.push(h);nextHealth+=5000;}
   }
   const elapsed=Date.now()-began;const closeAt=Date.now();await rpc(app.page,'pty.close',{ptyId:pty.ptyId,generation:pty.generation});const ptyCloseMs=Date.now()-closeAt;
   const results=[];for(const plugin of ids)results.push(await until(async()=>{const r=await command(plugin,'load.results');return r.done?r:false;},20000));
   const histogram=Array(501).fill(0),close=[];for(const r of results){r.histogram.forEach((n,i)=>histogram[i]+=n);close.push(...r.closeMs);}
   const count=histogram.reduce((a,b)=>a+b,0);let accumulated=0,p95=0;for(let n=0;n<histogram.length;n++){accumulated+=histogram[n];if(accumulated>=count*.95){p95=n*10;break;}}
   const measurement={enabled,bytes,elapsed,bytesPerSecond:bytes*1000/elapsed,reads,ptyCloseMs,eventP95Ms:p95,closeP95Ms:close.length?percentile(close,.95):null,produced:results.reduce((n,r)=>n+r.produced,0),accepted:results.reduce((n,r)=>n+r.accepted,0),rejected:results.reduce((n,r)=>n+r.rejected,0),gaps:results.reduce((n,r)=>n+r.gaps,0),histogram,health,producers:results};measurements.push(measurement);await writeFile(join(output,'measurements.json'),JSON.stringify({data,measurements},null,2));console.log(JSON.stringify({...measurement,histogram:undefined,health:undefined,producers:undefined}));
 }
 const [baseline,loaded]=measurements,decrease=1-loaded.bytesPerSecond/baseline.bytesPerSecond;
 assert.ok(loaded.produced>=11800&&loaded.produced<=12200,`produced ${loaded.produced}`);assert.equal(loaded.rejected,0);assert.equal(loaded.gaps,0);assert.ok(loaded.eventP95Ms<=1000,`event p95 ${loaded.eventP95Ms}`);assert.ok(loaded.closeP95Ms<=1000&&loaded.ptyCloseMs<=1000);assert.ok(decrease<=.1,`SSH throughput decrease ${decrease}`);
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,data,decrease,measurements},null,2));
}finally{await stop(app);}
