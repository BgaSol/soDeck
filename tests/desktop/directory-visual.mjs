import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve(process.env.T010_IME_ONLY?'.qa/t010/ime':'.qa/t010/visual');await mkdir(output,{recursive:true});
const config=JSON.parse((await readFile('.qa/t010/linux/test-config.json','utf8')).replace(/^\uFEFF/,''));
assert.equal(config.port,22281);
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:15000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args);
const original=await dpi('-Action','get');await writeFile(join(output,'dpi-original.json'),JSON.stringify(original));
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9772,'sodeck.terminal':9773});
const results=[];let app;let browsers=[];
try{
 for(const percent of (process.env.T010_IME_ONLY?[200]:[200,150,100])){
  assert.equal((await dpi('-Action','set','-Percent',String(percent))).percent,percent);await delay(400);
  for(const [theme,path] of (process.env.T010_IME_ONLY?[['graphite','themes/graphite.json']]:[['graphite','themes/graphite.json'],['paper','tests/fixtures/themes/paper.json']])){
   process.env.SODECK_THEME_FILE=resolve(path);
   app=await launch({data:join(output,`${theme}-${percent}-${Date.now()}`),shellPort:9771,pluginPort:9772});
   const server=await rpc(app.page,'servers.create',{name:`目录联动 · ${theme} · ${percent}%`,host:config.host,port:config.port,username:config.username,authType:'key'});
   await rpc(app.page,'auth.set',{serverId:server.id,privateKeyPath:config.privateKeyPath,remember:true});
   try{await rpc(app.page,'connections.acquire',{serverId:server.id,workspaceId:server.id,operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:server.id,challengeId:e.details.challengeId,fingerprint:e.details.fingerprint});}
   await rpc(app.page,'workspaces.open',{serverId:server.id});
   const connect=async port=>{const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));browsers.push(b);return until(()=>b.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));};
   const file=await connect(9772),term=await connect(9773);
   const invoke=(command,args={})=>pluginRpc(file,'plugin.invoke',{command,args});
   const snapshot=()=>invoke('files.snapshot');
   await until(async()=>(await snapshot()).path==='/home/'+config.username,30000);
   let win=(await nativeWindow(app,'list')).find(w=>w.visible);assert.equal(win.dpi,percent*.96);
   await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(1100*percent/100),Height:Math.round(720*percent/100)});
   await nativeWindow(app,'show',{WindowHandle:win.handle});await delay(300);
   await term.locator('.xterm-helper-textarea').pressSequentially('cd /tmp\r');await until(async()=>(await snapshot()).path==='/tmp',30000);
   assert.equal(await term.evaluate(()=>document.hasFocus()),true,'automatic navigation must preserve terminal focus');
   assert.equal(await file.getByRole('button',{name:'跟随终端',exact:true}).getAttribute('aria-pressed'),'true');
   const colors=JSON.parse(await readFile(path,'utf8')).colors;
   assert.equal(await file.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--selected').trim()),colors.selected);
   assert.equal(await file.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
   await file.getByLabel('远端绝对路径').fill('/usr');await file.getByLabel('远端绝对路径').press('Enter');await until(async()=>(await snapshot()).path==='/usr');
   await file.getByRole('button',{name:'跟随终端',exact:true}).focus();await file.keyboard.press('Space');assert.equal((await snapshot()).follow.enabled,false);
   await file.getByRole('button',{name:'定位到终端目录',exact:true}).focus();await file.keyboard.press('Space');await until(async()=>(await snapshot()).path==='/tmp');
   if(process.env.T010_IME_ONLY){
    const ime=(action,extra=[])=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(win.handle),...extra]);
    await invoke('follow.setEnabled',{enabled:true});await until(async()=>(await snapshot()).path==='/tmp');
    await term.locator('.xterm-helper-textarea').pressSequentially('sleep 9; cd /var\r');
    const input=file.getByLabel('远端绝对路径');await ime('focus');
    const fileId=await file.evaluate(()=>window.__SODECK_CONTEXT__.viewId),surface=await app.page.locator(`[data-view-id="${fileId}"]`).boundingBox(),box=await input.boundingBox();
    win=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle);
    await nativeWindow(app,'click',{WindowHandle:win.handle,X:Math.round(win.clientX+(surface.x+box.x+box.width/2)*percent/100),Y:Math.round(win.clientY+(surface.y+box.y+box.height/2)*percent/100)});
    await input.fill('/tmp/');await input.focus();
    await file.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
    const previous=await ime('state');let shifted=false;
    try{
     await ime('language',['-Layout','0000000008040804']);let composing=false;const attempts=[];
     for(let attempt=0;attempt<2;attempt++){
      await ime('keys',['-Keys','zhongwen']);await delay(200);composing=await file.evaluate(()=>window.__imeComposing);attempts.push({composing,state:await ime('state'),events:await file.evaluate(()=>window.__imeEvents)});await writeFile(join(output,'attempts.json'),JSON.stringify(attempts,null,2));if(composing)break;
      await input.fill('/tmp/');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;
     }
     assert.equal(composing,true,'native Microsoft Pinyin must enter composition');
     const draft=await input.inputValue();await delay(10500);assert.equal(await input.inputValue(),draft);assert.equal((await snapshot()).path,'/tmp');
     await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'composition.png')});
     await ime('keys',['-Keys','SPACE']);await until(async()=>!(await file.evaluate(()=>window.__imeComposing)));
     const events=await file.evaluate(()=>window.__imeEvents);assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));
     await writeFile(join(output,'composition-events.json'),JSON.stringify(events,null,2));
     assert.equal((await snapshot()).path,'/tmp');await input.press('Escape');await until(async()=>(await snapshot()).path==='/var',30000);
     console.log('Trusted Microsoft Pinyin composition survives delayed terminal cwd changes and applies the pending directory only after editing ends');
    }finally{if(await file.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await ime('language',['-Layout',previous.focusLayout]);}
   }
   await file.emulateMedia({reducedMotion:'reduce'});
   await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,`${theme}-${percent}.png`)});
   results.push({theme,percent,nativeDpi:win.dpi,devicePixelRatio:await file.evaluate(()=>devicePixelRatio),keyboard:true,focusPreserved:true,diagnostics:await invoke('files.diagnostics')});
   console.log(`${theme} ${percent}% native DPI/keyboard/focus passed`);
   await stop(app);app=undefined;for(const b of browsers)await b.close().catch(()=>{});browsers=[];
  }
 }
}finally{
 if(app)await stop(app);for(const b of browsers)await b.close().catch(()=>{});
 const restored=await dpi('-Action','set','-Percent',String(original.percent));await writeFile(join(output,'results.json'),JSON.stringify({results,original,restored},null,2));assert.equal(restored.percent,original.percent);
}
