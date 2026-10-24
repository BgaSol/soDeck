import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

export const root = resolve(fileURLToPath(new URL('../../', import.meta.url)));
export const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export async function nativeWindow(runtime,action,options={}) {
  const args=['-NoProfile','-File',join(root,'tests/desktop/windows.ps1'),'-Action',action,'-AppProcessId',String(runtime.process.pid)];
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
export async function launch({data, shellPort=9333, pluginPort=9334, executable=join(root,'target/debug/sodeck-desktop.exe'), packagePath=join(root,'resources/terminal.sodeck-plugin'), keysPath=join(root,'resources/development-public-key.json')}={}) {
  data = resolve(data ?? join(root, '.qa/native', String(Date.now())));
  await mkdir(data, {recursive:true});
  const process = spawn(executable, [], {
    cwd: root, windowsHide:true, stdio:['ignore','pipe','pipe'],
    env:{...globalThis.process.env,SODECK_DATA_DIR:data,SODECK_SHELL_DEBUG_PORT:String(shellPort),
      SODECK_WEBVIEW_ARGS:`--remote-debugging-port=${pluginPort}`,
      SODECK_PLUGIN_PACKAGE:packagePath,SODECK_PLUGIN_KEYS:keysPath},
  });
  let output=''; process.stdout.on('data',data=>output+=data); process.stderr.on('data',data=>output+=data);
  const errors=[];
  try {
    await until(async()=>{if(process.exitCode!==null) throw new Error(`App exited ${process.exitCode}: ${output}`);return fetch(`http://127.0.0.1:${shellPort}/json/version`).then(r=>r.ok);},30000);
    const browser=await chromium.connectOverCDP(`http://127.0.0.1:${shellPort}`);
    const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')||p.url().includes('tauri://')));
    page.on('pageerror',error=>errors.push(error.message));
    await page.waitForSelector('#root > *');
    const info=await rpc(page,'app.info');
    if(info.startupError) throw new Error(`Native startup: ${info.startupError}`);
    return {process,browser,page,data,pluginPort,errors,output:()=>output};
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
  if(runtime.process.exitCode===null){try{await rpc(runtime.page,'app.exit');}catch{} }
  try{await until(()=>runtime.process.exitCode!==null,10000);}catch{runtime.process.kill();}
  await runtime.pluginBrowser?.close().catch(()=>{});
  await runtime.browser.close().catch(()=>{});
  await writeFile(join(runtime.data,'process.log'),runtime.output());
}
