import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {launch,until,stop} from './harness.mjs';

const output=resolve('.qa/tray/exit-'+Date.now());await mkdir(output,{recursive:true});
const executable=resolve('target/debug/sodeck-desktop.exe'),results=[];let app;
async function native(action){const {stdout}=await promisify(execFile)('powershell.exe',['-NoProfile','-File','tests/desktop/tray-windows.ps1','-AppProcessId',String(app.process.pid),'-Executable',executable,'-Action',action],{windowsHide:true,timeout:20000});return JSON.parse(stdout);}
try{
 for(const action of ['leftExit','exitBurst'])for(let index=0;index<5;index++){
  app=await launch({data:join(output,action+'-'+index),shellPort:9771+index+(action==='exitBurst'?20:0),pluginPort:9772+index+(action==='exitBurst'?20:0)});
  if(action==='exitBurst')await native('right');
  const result=await native(action);await until(()=>app.process.exitCode!==null,10000);
  assert.equal(app.process.exitCode,0);assert.equal(result.windows.filter(w=>w.cls==='Tauri Window'&&w.visible).length,0);
  let disappeared=false;for(const sample of result.raceSamples??[]){const hasMain=sample.windows.some(w=>w.cls==='Tauri Window'&&w.visible);if(!hasMain)disappeared=true;else assert.equal(disappeared,false,'A destroyed main window must not reappear');}
  results.push({action,index,pid:app.process.pid,exitCode:app.process.exitCode,...result});
  await stop(app);app=undefined;console.log(action,index+1,'passed');
 }
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,results},null,2));
 await writeFile('.qa/tray/exit-latest.json',JSON.stringify({output}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),results},null,2));throw error;}
finally{await stop(app);console.log(output);}
