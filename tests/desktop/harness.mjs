import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

export const root = resolve(fileURLToPath(new URL('../../', import.meta.url)));
export const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export async function nativeWindow(runtime,action,options={}) {
  const args=['-NoProfile','-File',join(root,'tests/desktop/windows.ps1'),'-Action',action,'-AppProcessId',String(runtime.process.pid),'-Executable',runtime.executable??join(root,'target/debug/sodeck-desktop.exe')];
  for(const [key,value] of Object.entries(options))args.push(`-${key}`,String(value));
  const {stdout}=await promisify(execFile)('powershell.exe',args,{windowsHide:true,timeout:15000});
  return action==='list'?JSON.parse(stdout):stdout;
}
export async function until(check, timeout = 15000) {
  const started = Date.now(); let last;
  while (Date.now() - started < timeout) {
    try { const result = await check(); if (result) return result; } catch (error) { last = error; }
    await delay(100);
  }
  throw last ?? new Error(`Condition did not become true in ${timeout} ms`);
}
export async function rpc(page, method, params = {}, requestId) {
  const response = await page.evaluate(async ({method, params, requestId}) => {
    return window.__TAURI_INTERNALS__.invoke('rpc', {request:{apiVersion:1, requestId:requestId ?? crypto.randomUUID(), method, params, timeoutMs:130000}});
  }, {method, params, requestId});
  if (response.error) throw Object.assign(new Error(response.error.message), response.error);
  return response.result;
}
export async function launch({ordinary=false,allowStartupWarning=false,bundledResources=false,data, shellPort=9333, pluginPort=9334, executable=globalThis.process.env.SODECK_TEST_EXE??join(root,'target/debug/sodeck-desktop.exe'), packagePath=join(root,'resources/terminal.sodeck-plugin'), keysPath=join(root,'resources/development-public-key.json'),cwd=root,startupTimeout=30000}={}) {
  data = resolve(data ?? join(root, '.qa/native', String(Date.now())));
  if(resolve(executable)!==join(root,'target/debug/sodeck-desktop.exe')) {
    const marker=JSON.parse(await readFile(join(dirname(executable),'portable.json'),'utf8'));
    if(marker.schemaVersion!==1)throw Error('Isolated executable requires portable.json schemaVersion 1');
  }
  if(ordinary&&data!==resolve(dirname(executable),'data'))throw Error('Ordinary test must use the executable portable data directory');
  await mkdir(data, {recursive:true});
  const launchDebugger=globalThis.process.env.SODECK_TEST_LAUNCH_DEBUGGER;
  const launchEnv={...globalThis.process.env,SODECK_DEV_MODE:'1',SODECK_DATA_DIR:data,SODECK_SHELL_DEBUG_PORT:String(shellPort),SODECK_WEBVIEW_ARGS:`--remote-debugging-port=${pluginPort}`,SODECK_PLUGIN_PACKAGE:packagePath,SODECK_PLUGIN_KEYS:keysPath};
  if(ordinary){for(const name of Object.keys(launchEnv))if(name.startsWith('SODECK_')||name.startsWith('WEBVIEW2_'))delete launchEnv[name];launchEnv.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port='+shellPort;}
  if(bundledResources)for(const name of ['SODECK_PLUGIN_PACKAGES','SODECK_PLUGIN_PACKAGE','SODECK_PLUGIN_KEYS'])delete launchEnv[name];
  const process = spawn(launchDebugger??executable, launchDebugger?[executable,join(data,'debugger.log')]:[], {
    cwd, windowsHide:true, stdio:['ignore','pipe','pipe'],
    env:launchEnv,
  });
  let output=''; process.stdout.on('data',data=>output+=data); process.stderr.on('data',data=>output+=data);
  const errors=[];
  try {
    await until(async()=>{if(process.exitCode!==null) throw new Error(`App exited ${process.exitCode}: ${output}`);return fetch(`http://127.0.0.1:${shellPort}/json/version`).then(r=>r.ok);},startupTimeout);
    const browser=await chromium.connectOverCDP(`http://127.0.0.1:${shellPort}`);
    const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')||p.url().includes('tauri://')));
    page.on('pageerror',error=>errors.push(error.message));
    await page.waitForSelector('#root > *');
    const info=await rpc(page,'app.info');
    if(info.startupError&&!allowStartupWarning) throw new Error(`Native startup: ${info.startupError}`);
    if(launchDebugger){const child=output.match(/SODECK_CHILD_PID=(\d+)/);if(!child)throw new Error('Debugger did not report its child PID');process.pid=Number(child[1]);}
    if(globalThis.process.env.SODECK_TEST_DEBUGGER) {
      const watch=globalThis.process.env.SODECK_TEST_WATCH_CALLBACK?output.match(/SODECK_HANDLER=([a-f0-9]+,\d+)/)?.[1]:undefined;
      const debuggerProcess=spawn(globalThis.process.env.SODECK_TEST_DEBUGGER,[String(process.pid),join(data,'debugger.log'),...(watch?[watch]:[])],{cwd:root,windowsHide:true,stdio:'inherit'});
      debuggerProcess.on('error',error=>errors.push(error.message));
      await delay(1500);
    }
    return {process,browser,page,data,pluginPort,executable,errors,output:()=>output};
  } catch(error) {process.kill();await writeFile(join(data,'launch-error.log'),output);throw error;}
}
export async function pluginPages(runtime) {
  // Closing the last raw Wry surface can end its WebView2 browser process.
  // A later terminal starts a fresh process on the same debug port; retaining
  // the disconnected Playwright Browser would enumerate no new pages forever.
  if(!runtime.pluginBrowser?.isConnected()) {
    const endpoint=`http://127.0.0.1:${runtime.pluginPort}`;
    const version=await fetch(`${endpoint}/json/version`).then(response=>response.json());
    runtime.pluginBrowser=await chromium.connectOverCDP(endpoint);
    const connection={connectedAt:new Date().toISOString(),endpoint:version.webSocketDebuggerUrl};
    (runtime.pluginDebugConnections??=[]).push(connection);
    runtime.pluginBrowser.on('disconnected',()=>connection.disconnectedAt=new Date().toISOString());
  }
  return runtime.pluginBrowser.contexts().flatMap(c=>c.pages()).filter(p=>p.url().includes('sodeck-plugin.localhost'));
}
export async function pluginRpc(page,method,params={}){
  // Context exists before the UI module loads. Let its bridge initialize first,
  // otherwise that initialization can replace our temporary response handler.
  await page.waitForFunction(()=>typeof window.__sodeckReceive==='function');
  const response=await page.evaluate(({method,params})=>new Promise((resolve,reject)=>{
    const requestId=`native-test-${crypto.randomUUID()}`;
    const previous=window.__sodeckReceive;
    const timer=setTimeout(()=>{window.__sodeckReceive=previous;reject(new Error('Plugin test call timed out'));},15000);
    window.__sodeckReceive=response=>{if(response.requestId===requestId){clearTimeout(timer);window.__sodeckReceive=previous;resolve(response);}else previous?.(response);};
    window.ipc.postMessage(JSON.stringify({apiVersion:1,requestId,method,params}));
  }),{method,params});
  if(response.error)throw Object.assign(new Error(response.error.message),response.error);return response.result;
}
export async function stop(runtime) {
  if(!runtime)return;
  const shutdown=runtime.shutdown??={requestedAt:new Date().toISOString(),forced:false};
  if(runtime.process.exitCode===null&&runtime.process.signalCode===null){try{await rpc(runtime.page,'app.exit');}catch(error){shutdown.requestError=error.message;} }
  try{await until(()=>runtime.process.exitCode!==null||runtime.process.signalCode!==null,10000);}catch{
    shutdown.forced=true;runtime.process.kill();
    await until(()=>runtime.process.exitCode!==null||runtime.process.signalCode!==null,10000).catch(()=>{});
  }
  shutdown.exitCode=runtime.process.exitCode;shutdown.signalCode=runtime.process.signalCode;
  await writeFile(join(runtime.data,'shutdown.json'),JSON.stringify(shutdown,null,2));
  await runtime.pluginBrowser?.close().catch(()=>{});
  await runtime.browser.close().catch(()=>{});
  await writeFile(join(runtime.data,'process.log'),runtime.output());
}
