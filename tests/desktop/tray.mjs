import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import net from 'node:net';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay} from './harness.mjs';

const output=resolve('.qa/tray/native-'+Date.now());await mkdir(output,{recursive:true});
const executable=resolve(process.argv[2]??'target/debug/sodeck-desktop.exe');
const python='C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const checks=[],states=[];let app;const fixtures=[],browsers=[];
const note=text=>{checks.push(text);console.log(text);};
async function fixture(file,name,args=[]){const p=spawn(python,['-u',file,join(output,name),...args],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,PYTHONPATH:resolve('.local/python')}});fixtures.push(p);let out='',errors='';p.stdout.on('data',b=>out+=b);p.stderr.on('data',b=>errors+=b);await until(()=>{if(p.exitCode!==null)throw Error(errors);return out.includes('\n');});return JSON.parse(out.split('\n')[0]);}
async function native(action,extra={}){const args=['-NoProfile','-File','tests/desktop/tray-windows.ps1','-AppProcessId',String(app.process.pid),'-Executable',executable,'-Action',action];for(const[k,v]of Object.entries(extra))args.push('-'+k,String(v));const{stdout}=await promisify(execFile)('powershell.exe',args,{windowsHide:true,timeout:20000,maxBuffer:1048576});const state=JSON.parse(stdout);states.push({action,extra,time:Date.now(),state});return state;}
async function pageFor(port,view){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));browsers.push(b);return until(async()=>{for(const p of b.contexts().flatMap(c=>c.pages()))if(!p.isClosed()&&await p.evaluate(()=>window.__SODECK_CONTEXT__?.viewId).catch(()=>null)===view)return p;});}
async function server(name,port,user,password){const s=await rpc(app.page,'servers.create',{name,host:'127.0.0.1',port,username:user,authType:'password'});await rpc(app.page,'auth.ssh.set',{serverId:s.id,password,remember:false});try{const lease=await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});await rpc(app.page,'connections.release',{leaseId:lease.leaseId});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:s.id,...e.details});}return s;}
async function echo(port){return new Promise((resolve,reject)=>{const c=net.connect(port,'127.0.0.1');c.setTimeout(5000,()=>{c.destroy();reject(Error('Echo timed out'));});c.on('error',reject);c.on('connect',()=>c.write('tray-echo'));c.once('data',d=>{c.destroy();resolve(d.toString());});});}
try{
 const ssh=await fixture('tests/core/ssh_fixture.py','ssh'),network=await fixture('tests/core/network_fixture.py','network',['1']);
 process.env.SODECK_PLUGIN_PACKAGES=['terminal','network-proxy'].map(n=>resolve(`resources/${n}.sodeck-plugin`)).join(delimiter);
 process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':9762,'sodeck.network-proxy':9763});
 app=await launch({data:join(output,'app'),shellPort:9761,pluginPort:9762,executable,allowStartupWarning:true});
 const firstState=await native('inspect');const main=firstState.windows.find(w=>w.cls==='Tauri Window'&&w.visible);assert.ok(main);
 const mainState=s=>s.windows.find(w=>w.handle===main.handle);
 const visible=async s=>{const started=Date.now();await until(async()=>{if(mainState(s)?.visible&&!mainState(s)?.minimized&&s.foreground===main.handle&&s.menus.length===0)return true;s=await native('inspect');return false;},5000);states.push({action:'restore-settled',waitMs:Date.now()-started,state:s});};
 const a=await server('托盘终端 A',ssh.port,'test-user','isolated-test-password');await rpc(app.page,'workspaces.open',{serverId:a.id});
 const view=await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.workspaceId===a.id));const terminal=await pageFor(9762,view.viewId);
 await pluginRpc(terminal,'plugin.invoke',{command:'settings.shellIntegration',args:{enabled:false}});
 await until(async()=>(await pluginRpc(terminal,'plugin.invoke',{command:'sessions.list',args:{workspaceId:a.id}})).some(s=>['failed','disconnected','ended'].includes(s.status)));
 await rpc(app.page,'commands.execute',{commandId:'terminal.reconnect',viewId:view.viewId,operationId:crypto.randomUUID()});
 await until(async()=>(await terminal.locator('.xterm-rows').innerText()).includes('fixture-ready'));
 const before=await rpc(app.page,'resources.list');assert.equal(before.ptys.length,1);
 for(let i=0;i<10;i++){await rpc(app.page,'windows.close',{windowId:'main'});assert.equal(mainState(await native('inspect')).visible,false);await visible(await native('left'));}
 assert.deepEqual((await rpc(app.page,'resources.list')).ptys,before.ptys);
 assert.equal((await rpc(app.page,'workbench.snapshot')).views.filter(v=>v.pluginId==='sodeck.terminal').length,1);
 note('TRAY-01: ten real tray left clicks restore the same HWND/workspace/PTY, without menus');
 await rpc(app.page,'windows.control',{action:'minimize'});assert.equal(mainState(await native('inspect')).minimized,true);await visible(await native('left'));
 await rpc(app.page,'windows.control',{action:'toggleMaximize'});assert.equal(mainState(await native('inspect')).maximized,true);await rpc(app.page,'app.hide');await visible(await native('left'));assert.equal(mainState(await native('inspect')).maximized,true);
 await visible(await native('double'));await visible(await native('left',{Count:10}));
 await rpc(app.page,'windows.control',{action:'toggleMaximize'});
 await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await app.page.getByRole('dialog').waitFor();await rpc(app.page,'app.hide');await visible(await native('left'));assert.equal(await app.page.getByRole('dialog').isVisible(),true);await app.page.getByRole('dialog').getByRole('button',{name:'关闭对话框',exact:true}).click();
 note('TRAY-02: minimize, maximize/hide, double/ten clicks and an open shell modal restore correctly');
 await rpc(app.page,'app.hide');let menu=await native('right');assert.equal(mainState(menu).visible,false);assert.deepEqual(menu.menus[0].items.map(i=>i.text),['显示主窗口','退出 SoDeck']);await native('capture',{OutputPath:join(output,'menu.png')});assert.equal(mainState(await native('key')).visible,false);
 await native('right');await native('key',{KeyCode:40});await visible(await native('key',{KeyCode:13}));
 const b=await server('托盘独立 B',ssh.port,'test-user','isolated-test-password');await rpc(app.page,'workspaces.open',{serverId:b.id});await until(async()=>(await rpc(app.page,'resources.list')).ptys.length===2);await until(async()=>{try{return await rpc(app.page,'workspaces.move',{workspaceId:b.id,operationId:crypto.randomUUID()});}catch(e){if(e.code!=='MOVE_IN_PROGRESS')throw e;return false;}});
 // The new child covers the main window; tray activation must return to main.
 await visible(await native('left'));await rpc(app.page,'app.hide');await native('right');assert.equal(mainState(await native('outside')).visible,false);await visible(await native('left'));
 note('TRAY-03: hidden right click, Escape/outside cancellation and keyboard Show; foreground from child');
 const n=await server('托盘转发',network.port,'network-test','isolated-network-password');await rpc(app.page,'workspaces.open',{serverId:n.id});
 const nv=await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.open',workspaceId:n.id,operationId:crypto.randomUUID()});const np=await pageFor(9763,nv.viewId);
 await until(async()=>{try{return await pluginRpc(np,'plugin.invoke',{command:'network.snapshot',args:{}});}catch{return false;}});
 const rule=await pluginRpc(np,'plugin.invoke',{command:'network.save',args:{expectedRevision:0,rule:{name:'托盘回收',type:'port-forward',targetHost:'echo.internal',targetPort:7,bindMode:'auto',port:0,autoReconnect:false}}});
 await rpc(app.page,'commands.execute',{pluginId:'sodeck.network-proxy',commandId:'network.start',viewId:nv.viewId,operationId:crypto.randomUUID(),args:{id:rule.id}});
 const running=await until(async()=>{const s=await pluginRpc(np,'plugin.invoke',{command:'network.snapshot',args:{selected:rule.id}});return s.selected.run?.status==='ready'&&s.selected.run;});await writeFile(join(output,'running.json'),JSON.stringify(running,null,2));
 const port=running.address?.port;assert.ok(port,'Listener port is reported');assert.equal(await echo(port),'tray-echo');
 await rpc(app.page,'app.hide');assert.equal(await echo(port),'tray-echo');await visible(await native('left'));assert.equal((await rpc(app.page,'resources.list')).ptys.length,2);
 await native('right');await native('menu',{MenuIndex:1});await until(()=>app.process.exitCode!==null);assert.equal(app.process.exitCode,0);assert.equal((await native('inspect').catch(()=>({windows:[]}))).windows.length,0);await assert.rejects(echo(port));
 await until(async()=>{const e=(await readFile(join(output,'ssh/events.jsonl'),'utf8')).trim().split('\n').map(JSON.parse);return e.filter(v=>v.event==='connection-open').length===e.filter(v=>v.event==='connection-close').length;});
 note('TRAY-04: active PTYs, detached workspace and live TCP forwarding survive hide; tray Exit closes app and listeners normally');
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks,executable,states},null,2));await writeFile('.qa/tray/native-latest.json',JSON.stringify({output,checks}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,checks,states},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);for(const b of browsers)await b.close().catch(()=>{});for(const p of fixtures)p.kill();console.log(output);}
