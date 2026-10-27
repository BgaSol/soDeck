import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createInterface} from 'node:readline';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop} from './harness.mjs';

export async function networkHarness(name,shellPort){
  const output=resolve('.qa/t014/'+name+'-'+Date.now());await mkdir(output,{recursive:true});
  const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
  const fixture=spawn(python,['-u','tests/core/network_fixture.py',join(output,'fixture'),'3'],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python')}});
  let stderr='';fixture.stderr.on('data',b=>stderr+=b);
  const config=await new Promise((r,j)=>{createInterface({input:fixture.stdout}).once('line',line=>r(JSON.parse(line)));fixture.once('exit',()=>j(Error(stderr)));});
  process.env.SODECK_PLUGIN_PACKAGES=resolve('resources/network-proxy.sodeck-plugin');process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.network-proxy':shellPort+1});
  let app,browser;const checks=[];let target,view;
  const h={output,checks,get app(){return app;},get target(){return target;},get view(){return view;},
    note(text){checks.push(text);console.log(text);},
    async start(options={}){app=await launch({data:join(output,'data'),shellPort,pluginPort:shellPort+1,packagePath:resolve('resources/network-proxy.sodeck-plugin'),allowStartupWarning:true,...options});return app;},
    async create(){let previous;const nodes=[];for(let i=0;i<3;i++){const s=await rpc(app.page,'servers.routes.v2.create',{name:`隔离 SSH ${i+1}`,host:i===0?'127.0.0.1':`node-${i}.internal`,port:i===0?config.port:22,username:'network-test',connectionRoute:previous?{kind:'sshJump',jumpServerId:previous.id}:{kind:'direct'}});await rpc(app.page,'auth.set',{serverId:s.id,password:'isolated-network-password',remember:false});previous=s;nodes.push(s);}target=previous;return nodes;},
    async auth(nodes){for(const n of nodes)await rpc(app.page,'auth.set',{serverId:n.id,password:'isolated-network-password',remember:false});},
    async trust(){for(let i=0;i<4;i++)try{const lease=await rpc(app.page,'connections.acquire',{serverId:target.id,workspaceId:target.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});return;}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:e.details.serverId,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}},
    async open(){await rpc(app.page,'workspaces.open',{serverId:target.id});view=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:target.id,operationId:crypto.randomUUID()});return h.page();},
    async page(){if(!browser?.isConnected())browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+(shellPort+1)));return until(async()=>{for(const p of browser.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&p.url().includes('sodeck-plugin')&&await p.evaluate(()=>!window.__SODECK_CONTEXT__?.dialog&&window.__SODECK_CONTEXT__?.viewId)===view.viewId){await p.getByRole('button',{name:'创建',exact:true}).waitFor();return p;}return false;});},
    async invoke(command,args={}){return pluginRpc(await h.page(),'plugin.invoke',{command,args});},
    async save(changes={}){return h.invoke('network.save',{expectedRevision:0,rule:{name:'内网入口',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false,...changes}});},
    async run(rule,restart=false,command='network.start'){return rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:command,viewId:view.viewId,operationId:crypto.randomUUID(),args:{id:rule.id,restart}});},
    async state(rule){return (await h.invoke('network.snapshot',{selected:rule.id})).selected.run;},
    async curl(port,scheme='http',host='http.internal'){return (await promisify(execFile)('curl.exe',['-sf','--max-time','15','--noproxy','','--proxy',`${scheme}://127.0.0.1:${port}`,`http://${host}/`],{windowsHide:true})).stdout;},
    async control(action,node=0){fixture.stdin.write(JSON.stringify({action,node})+'\n');await until(async()=>{const events=await h.events();return events.filter(e=>e.event==='control').at(-1)?.action===action;});},
    async events(){return (await readFile(join(output,'fixture/events.jsonl'),'utf8')).trim().split('\n').filter(Boolean).map(s=>JSON.parse(s));},
    async stop(){await stop(app);app=undefined;await browser?.close().catch(()=>{});browser=undefined;},
    async close(){await h.stop();fixture.stdin.end();await until(()=>fixture.exitCode!==null,5000).catch(()=>fixture.kill());await writeFile(join(output,'results.json'),JSON.stringify({checks,stderr},null,2));await writeFile('.qa/t014/'+name+'-latest.json',JSON.stringify({output,checks},null,2));}
  };return h;
}
