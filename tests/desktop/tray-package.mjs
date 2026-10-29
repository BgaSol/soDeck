import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {cp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {launch,rpc,stop,until} from './harness.mjs';

const source=resolve(process.argv[2]??'.local/dev/SoDeck-v2-Tray');
const output=resolve('.qa/tray/package-'+Date.now());await mkdir(output,{recursive:true});
const candidate=join(output,'便携托盘'),executable=join(candidate,'sodeck-desktop.exe');
await cp(source,candidate,{recursive:true,filter:src=>!/^data(?:-|$)/.test(src.slice(source.length+1).split(/[\\/]/)[0])});
const expected=JSON.parse(await readFile(join(candidate,'build.json'),'utf8')).hashes['sodeck-desktop.exe'];
assert.equal(createHash('sha256').update(await readFile(executable)).digest('hex'),expected);
const results=[],states=[];let app;
async function native(action,params={}){
 const args=['-NoProfile','-File','tests/desktop/tray-windows.ps1','-AppProcessId',String(app.process.pid),'-Executable',executable,'-Action',action];
 for(const [key,value]of Object.entries(params))args.push('-'+key,String(value));
 const {stdout}=await promisify(execFile)('powershell.exe',args,{windowsHide:true,timeout:20000});const state=JSON.parse(stdout);states.push({action,state,time:Date.now()});return state;
}
try{
 for(let run=0;run<2;run++){
  app=await launch({executable,data:join(candidate,'data'),ordinary:true,bundledResources:true,shellPort:9801+run,pluginPort:9811+run,cwd:output});
  const info=await rpc(app.page,'app.info');assert.equal(info.startupError,null);
  const sources=(await rpc(app.page,'observability.describe')).sources;assert.equal(sources.length,3);
  const first=await native('inspect'),main=first.windows.find(w=>w.cls==='Tauri Window'&&w.visible);assert.ok(main);
  await rpc(app.page,'windows.close',{windowId:'main'});assert.equal((await native('inspect')).windows.find(w=>w.handle===main.handle).visible,false);
  let shown=await native('left');await until(async()=>{if(shown.foreground===main.handle&&shown.windows.find(w=>w.handle===main.handle)?.visible)return true;shown=await native('inspect');return false;},5000);
  assert.equal(shown.menus.length,0);
  await rpc(app.page,'app.hide');const menu=await native('right');assert.equal(menu.windows.find(w=>w.handle===main.handle).visible,false);
  assert.deepEqual(menu.menus[0].items.map(i=>i.text),['显示主窗口','退出 SoDeck']);
  await native('capture',{OutputPath:join(output,`menu-${run}.png`)});
  await native('menu',{MenuIndex:1});await until(()=>app.process.exitCode!==null);assert.equal(app.process.exitCode,0);
  results.push({run,info,sourceCount:sources.length,mainHandle:main.handle,menu,normalExit:true});
  await stop(app);app=undefined;console.log(`release ordinary run ${run+1}: real tray left/right/Exit passed`);
 }
 await writeFile(join(output,'results.json'),JSON.stringify({passed:true,source,sha256:expected,results},null,2));
 await writeFile('.qa/tray/package-latest.json',JSON.stringify({output}));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),results,states},null,2));throw error;}
finally{await stop(app);console.log(output);}
