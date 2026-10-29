import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile,mkdir,writeFile,access} from 'node:fs/promises';
import {resolve,join,sep} from 'node:path';
import {chromium} from 'playwright';
import {until,rpc,root} from './harness.mjs';

export async function launchInstalled({config,install,shellPort=13501,pluginPort=13502}) {
  assert.equal(config.qa,true);assert.ok(config.identifier.startsWith('com.sodeck.v2.qa.'));
  install=resolve(install);assert.ok(install.startsWith(resolve(root,'.qa/t029')+sep));
  await assert.rejects(access(join(install,'portable.json')));
  const data=join(process.env.APPDATA,config.identifier),executable=join(install,'sodeck-desktop.exe');
  const env={...process.env};for(const name of Object.keys(env))if(name.startsWith('SODECK_')||name.startsWith('WEBVIEW2_'))delete env[name];
  env.SODECK_QA_SHELL_DEBUG_PORT=String(shellPort);env.SODECK_QA_WEBVIEW_ARGS='--remote-debugging-port='+pluginPort;
  env.SODECK_QA_PLUGIN_DEBUG_PORTS=JSON.stringify(Object.fromEntries(['sodeck.terminal','sodeck.files','sodeck.monitor','sodeck.network-proxy'].map((id,i)=>[id,pluginPort+i])));
  await mkdir(data,{recursive:true});
  const processHandle=spawn(executable,[],{cwd:install,windowsHide:true,stdio:['ignore','pipe','pipe'],env});
  let output='';processHandle.stdout.on('data',b=>output+=b);processHandle.stderr.on('data',b=>output+=b);
  let browser;
  try {
    await until(async()=>{assert.equal(processHandle.exitCode,null);return fetch(`http://127.0.0.1:${shellPort}/json/version`).then(r=>r.ok);},45000);
    browser=await chromium.connectOverCDP(`http://127.0.0.1:${shellPort}`);
    const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')));
    await page.waitForSelector('#root > *');
    const status=await rpc(page,'updates.status.get');assert.equal(status.distribution,'installed');assert.equal(status.channel,config.channel);
    const info=await rpc(page,'app.info');assert.ok(!info.startupError,info.startupError);
    return {process:processHandle,browser,page,data,pluginPort,executable,errors:[],output:()=>output};
  }catch(e){processHandle.kill();await browser?.close().catch(()=>{});await writeFile(join(data,'launch-error.log'),output);throw e;}
}
