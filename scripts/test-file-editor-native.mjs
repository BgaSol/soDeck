// Windows WebView2 with an actual file plugin archive and isolated SSH/app data.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import net from 'node:net';import http from 'node:http';
import {spawn} from 'node:child_process';import {setTimeout as delay} from 'node:timers/promises';import {chromium} from 'playwright';
import {fileDocument} from './file-editor-fixture.mjs';
assert.equal(process.platform,'win32','Native editor harness requires WebView2');
const executable=process.argv[2],plugin=process.env.SODECK_TEST_FILES_PLUGIN;
assert(executable&&fs.existsSync(executable));assert(plugin&&fs.existsSync(path.join(plugin,'plugin.json')));
const ssh=JSON.parse(fs.readFileSync(process.env.SODECK_TEST_SSH_CONFIG,'utf8'));
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-file-editor-native-'));
fs.writeFileSync(path.join(directory,'plugins.json'),JSON.stringify([{directory:path.resolve(plugin),manifest:JSON.parse(fs.readFileSync(path.join(plugin,'plugin.json'),'utf8')),enabled:true}]));
let child,browser,page;const errors=[];let runtimeLog='',probeError='';
const until=async(fn,message='Native editor condition timed out')=>{const deadline=Date.now()+25000;while(!await fn()){assert(Date.now()<deadline,typeof message==='function'?message():message);await delay(50);}};
const debuggerReady=port=>new Promise(resolve=>{const request=http.get({hostname:'127.0.0.1',port,path:'/json/version',agent:false,timeout:300},response=>{probeError=`HTTP ${response.statusCode}`;response.resume();resolve(response.statusCode===200);});request.on('timeout',()=>request.destroy(new Error('debugger socket timed out')));request.on('error',error=>{probeError=error.message;resolve(false);});});
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
try{
 const socket=net.createServer();await new Promise(resolve=>socket.listen(0,'127.0.0.1',resolve));const port=socket.address().port;await new Promise(resolve=>socket.close(resolve));
 child=spawn(path.resolve(executable),['--interactive-test','--test-data-dir',directory],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`,WEBVIEW2_USER_DATA_FOLDER:path.join(directory,'webview2')}});
 const log=data=>{runtimeLog=(runtimeLog+data).slice(-16384);};child.stdout.on('data',log);child.stderr.on('data',log);
 let spawnError;child.on('error',error=>spawnError=error);
 await until(async()=>{if(spawnError)throw spawnError;assert.equal(child.exitCode,null,runtimeLog);return debuggerReady(port);},()=>`WebView2 debugger unavailable: ${probeError}\n${runtimeLog}`);
 browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);await until(()=>{page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));return Boolean(page);});page.on('pageerror',error=>errors.push(String(error)));
 await until(async()=>(await invoke('snapshot')).plugins.some(p=>p.manifest.id==='sodeck.files'&&p.status==='running'));
 const catalog=await invoke('server_catalog'),changed=await invoke('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'文件编辑器原生验收',host:'127.0.0.1',port:ssh.port,username:ssh.username,authentication:{type:'privateKey',privateKeyPath:ssh.privateKeyPath,secret:{action:'clear'}},route:[]}}});
 await invoke('connection',{command:{op:'start',serverId:changed.selectedId}});await until(async()=>{const [view]=await invoke('connection',{command:{op:'snapshot'}});assert.notEqual(view.state,'failed',JSON.stringify(view.error));if(view.question)await invoke('connection',{command:{op:'answer',attemptId:view.attemptId,questionId:view.question.id,answer:{kind:'hostKey',accept:true}}});return view.state==='connected';});
 const workspace=(await invoke('workspace_snapshot')).workspaces[0];
 const opened=await invoke('component_ui',{operation:'action',args:{context:workspace.context,action:{op:'open',key:'sodeck.files:editor',requestId:'native-file-editor',resourceKey:ssh.remoteFile,params:{path:ssh.remoteFile}}}});
 const frame=page.frameLocator(`.component-surface[data-view-id="${opened.viewId}"] iframe`),document=fileDocument(frame),host=frame.locator('.code-editor');await document.waitFor();await until(()=>document.isEnabled());
 const draft='server {\n  server_name example.com;\n}\n';await document.fill(draft);await frame.getByLabel('文件语言').selectOption('nginx');await frame.getByRole('button',{name:'搜索',exact:true}).click();await frame.getByRole('textbox',{name:'查找当前文件'}).fill('server');await until(()=>frame.locator('.editor-find-count').textContent().then(text=>text.endsWith('2 处')));await frame.getByRole('button',{name:'下一处',exact:true}).click();assert.equal(await host.evaluate(el=>el.sodeckEditor.model.getLanguageId()),'nginx');assert(await frame.locator('.minimap canvas').count()>0);
 await host.evaluate(()=>new Promise((resolve,reject)=>{const worker=globalThis.MonacoEnvironment.getWorker('','editor');worker.addEventListener('error',event=>reject(new Error(event.message)));setTimeout(resolve,500);}));
 assert.equal(await host.evaluate(el=>el.sodeckEditor.setText('x'.repeat(1048577))),false);assert.equal(await document.inputValue(),draft);
 await frame.getByRole('button',{name:'保存',exact:true}).click();await frame.getByRole('status').filter({hasText:'已保存到服务器'}).waitFor();assert.equal(fs.readFileSync(ssh.remoteFile,'utf8'),draft);
 await frame.getByRole('button',{name:'关闭搜索'}).click();fs.mkdirSync('artifacts/files-ui',{recursive:true});await page.screenshot({path:'artifacts/files-ui/editor-native.png'});assert.deepEqual(errors,[]);
 const pids=(await invoke('snapshot')).plugins.map(p=>p.pid);await invoke('plugin:window|close',{label:'main'});await until(()=>child.exitCode!==null);assert.equal(child.exitCode,0);await until(()=>pids.every(pid=>{try{process.kill(pid,0);return false;}catch{return true;}}));
 fs.writeFileSync('artifacts/files-ui/native-editor.json',JSON.stringify({platform:process.platform,arch:process.arch,plugin:JSON.parse(fs.readFileSync(path.join(plugin,'plugin.json'),'utf8')).version,executable:path.resolve(executable),checks:['local ESM and worker under actual CSP','syntax','minimap','search','byte limit','safe SSH save','plugin exit']},null,2)+'\n');
 console.log('Native WebView2 file editor passed: local worker/CSP, syntax, minimap, search, byte limit, SSH save and exit');
}finally{if(child?.exitCode===null){child.kill();await new Promise(resolve=>child.once('exit',resolve));}await browser?.close().catch(()=>{});assert.equal(path.dirname(path.resolve(directory)),path.resolve(os.tmpdir()));assert(path.basename(directory).startsWith('sodeck-file-editor-native-'));fs.rmSync(directory,{recursive:true,force:true,maxRetries:8,retryDelay:250});}
