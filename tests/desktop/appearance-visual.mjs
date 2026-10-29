import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {chromium} from 'playwright';
import {launch,rpc,pluginRpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve('.qa/t011/visual');await mkdir(output,{recursive:true});
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:20000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args),contrast=(...args)=>ps('tests/desktop/high-contrast.ps1',args);
const original={dpi:await dpi(),contrast:await contrast()};await writeFile(join(output,'original-system.json'),JSON.stringify(original,null,2));
const cfg=JSON.parse(await readFile('.qa/t010/linux/test-config.json','utf8'));assert.equal(cfg.port,22281);
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9812,'sodeck.terminal':9813});delete process.env.SODECK_THEME_FILE;
let app,browsers=[],win;const checks=[],measurements=[];
const pref=()=>rpc(app.page,'ui.theme.preference.get');const select=async mode=>{const p=await pref();const next=await rpc(app.page,'ui.theme.select',{mode,expectedRevision:p.revision});await until(async()=>await app.page.evaluate(()=>document.documentElement.dataset.themeRevision)===next.revision);return next;};
const invoke=(page,command,args={})=>pluginRpc(page,'plugin.invoke',{command,args});
const ime=(action,args=[])=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(win.handle),...args]);
async function connect(port){const b=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port),25000);browsers.push(b);return until(()=>b.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('sodeck-plugin')));}
async function appearance(){
 await delay(500);
 const box=await app.page.getByRole('button',{name:'应用菜单'}).boundingBox();win=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle);const scale=win.dpi/96;
 await nativeWindow(app,'click',{WindowHandle:win.handle,X:Math.round(win.clientX+(box.x+box.width/2)*scale),Y:Math.round(win.clientY+(box.y+box.height/2)*scale)});
 await delay(200);await nativeWindow(app,'menuClick',{WindowHandle:win.handle,X:Math.round(win.clientX+(box.x+20)*scale),Y:Math.round(win.clientY+(box.y+box.height+16)*scale)});
 try{await app.page.getByRole('dialog',{name:'外观',exact:true}).waitFor({timeout:5000});}catch(e){await app.page.screenshot({path:join(output,'menu-failure.png')});await nativeWindow(app,'screen',{WindowHandle:win.handle,OutputPath:join(output,'menu-native-failure.png')});await writeFile(join(output,'menu-failure.txt'),JSON.stringify({body:await app.page.locator('body').innerText(),calls:await rpc(app.page,'calls.list',{method:'ui.menu.open'}),events:await app.page.evaluate(()=>window.__menuEvents)},null,2));throw e;}
}
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9811,pluginPort:9812});win=(await nativeWindow(app,'list')).find(w=>w.visible);await nativeWindow(app,'move',{WindowHandle:win.handle,X:20,Y:20});
 await app.page.evaluate(()=>{window.__menuEvents=[];const original=window.__sodeckNotify;window.__sodeckNotify=n=>{if(n.type==='ui.menu.closed')window.__menuEvents.push(n);original(n);};});
 const s=await rpc(app.page,'servers.create',{name:'开发服务器 · 中文长名称 / Linux',host:cfg.host,port:cfg.port,username:cfg.username,authType:'key'});await rpc(app.page,'auth.ssh.set',{serverId:s.id,privateKeyPath:cfg.privateKeyPath,remember:true});try{await rpc(app.page,'connections.acquire',{serverId:s.id,workspaceId:s.id,operationId:crypto.randomUUID()});}catch(e){assert.equal(e.code,'HOST_KEY_REQUIRED');await rpc(app.page,'hosts.trust',{serverId:s.id,...e.details});}
 await rpc(app.page,'workspaces.open',{serverId:s.id});const file=await connect(9812),term=await connect(9813);await until(async()=>(await invoke(file,'files.snapshot')).path==='/home/'+cfg.username,30000);
 await term.locator('.xterm-helper-textarea').pressSequentially("printf '\\033[31mANSI red  \\033[38;5;33mIndexed blue  \\033[38;2;120;60;190mTruecolor\\033[0m\\n'\r");
 for(const percent of [100,150,200]){
  assert.equal((await dpi('-Action','set','-Percent',String(percent))).percent,percent);await delay(350);
  for(const mode of ['light','dark']){
   await select(mode);await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(1280*percent/100),Height:Math.round(820*percent/100)});await nativeWindow(app,'show',{WindowHandle:win.handle});await delay(200);
   win=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle);assert.equal(win.dpi,percent*.96);
   await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,`${mode}-${percent}.png`)});
   measurements.push({mode,percent,dpi:win.dpi,devicePixelRatio:await file.evaluate(()=>devicePixelRatio)});
   await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(760*percent/100),Height:Math.round(480*percent/100)});await appearance();await app.page.getByRole('radio',{name:mode==='light'?'浅色':'深色'}).focus();await app.page.keyboard.press('Tab');await app.page.keyboard.press('Shift+Tab');await app.page.keyboard.press('Escape');await app.page.getByRole('dialog').waitFor({state:'hidden'});
  }
 }
 checks.push('Actual Windows 100/150/200% DPI, both themes, 1280×820 and 760×480 logical windows; native menu entry, radio keyboard and Escape');
 await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:2200,Height:1440});await nativeWindow(app,'show',{WindowHandle:win.handle});
 // Exercise every form and management surface in both palettes.
 for(const mode of ['light','dark']){
  await select(mode);await appearance();await app.page.screenshot({path:join(output,`appearance-${mode}.png`)});await app.page.keyboard.press('Escape');
  for(const label of ['插件','运行资源','调用记录']){await app.page.getByRole('button',{name:label,exact:true}).click();await app.page.getByRole('dialog').waitFor();await app.page.screenshot({path:join(output,`${label}-${mode}.png`)});await app.page.keyboard.press('Escape');}
  await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await app.page.getByRole('dialog').waitFor();await app.page.screenshot({path:join(output,`picker-${mode}.png`)});await app.page.getByRole('dialog').getByRole('button',{name:'添加服务器',exact:true}).click();await app.page.getByLabel('显示名称').fill('中文服务器 · staging');await app.page.getByLabel('主机地址').fill('localhost');await app.page.screenshot({path:join(output,`editor-${mode}.png`)});await app.page.keyboard.press('Escape');
 }
 // Trusted native IME composition survives a palette commit without changing focus.
 const input=file.getByLabel('远端绝对路径');await ime('focus');const fileId=await file.evaluate(()=>window.__SODECK_CONTEXT__.viewId),surface=await app.page.locator(`[data-view-id="${fileId}"]`).boundingBox(),box=await input.boundingBox();win=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle);
 await nativeWindow(app,'click',{WindowHandle:win.handle,X:Math.round(win.clientX+(surface.x+box.x+box.width/2)*2),Y:Math.round(win.clientY+(surface.y+box.y+box.height/2)*2)});await input.fill('/tmp/');await input.focus();
 await file.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
 const imeOriginal=await ime('state');let shifted=false;
 try{await ime('language',['-Layout','0000000008040804']);for(let n=0;n<2;n++){await ime('keys',['-Keys','zhongwen']);await delay(200);if(await file.evaluate(()=>window.__imeComposing))break;await input.fill('/tmp/');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;}
  assert.equal(await file.evaluate(()=>window.__imeComposing),true);const draft=await input.inputValue();await select('light');assert.equal(await file.evaluate(()=>window.__imeComposing),true);assert.equal(await input.inputValue(),draft);await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'ime.png')});await ime('keys',['-Keys','SPACE']);await until(async()=>!await file.evaluate(()=>window.__imeComposing));const events=await file.evaluate(()=>window.__imeEvents);assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));await writeFile(join(output,'ime-events.json'),JSON.stringify(events,null,2));checks.push('Trusted Microsoft Pinyin composition, draft and focus survive a live theme commit');
 }finally{if(await file.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await ime('language',['-Layout',imeOriginal.focusLayout]);}
 await contrast('-Action','set','-Flags',String(original.contrast.flags|1),'-Scheme',original.contrast.scheme??'');await until(async()=>(await pref()).highContrast,10000);await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,'high-contrast.png')});await select('dark');assert.equal((await pref()).highContrast,true);
 await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await until(async()=>(await pref()).highContrast===Boolean(original.contrast.flags&1));assert.equal((await pref()).mode,'dark');checks.push('Real Windows high contrast overrides both palettes and restores the remembered choice');
 await app.page.emulateMedia({reducedMotion:'reduce'});await app.page.evaluate(()=>{document.documentElement.style.fontSize='20px';});await appearance();await app.page.screenshot({path:join(output,'large-text.png')});await app.page.keyboard.press('Escape');checks.push('Reduced-motion media and enlarged text checked separately from native DPI');
}catch(e){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(e),stack:e.stack},null,2));throw e;}finally{
 const cleanupErrors=[];await stop(app).catch(e=>cleanupErrors.push(String(e)));for(const b of browsers)await b.close().catch(()=>{});await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'').catch(e=>cleanupErrors.push(String(e)));await dpi('-Action','set','-Percent',String(original.dpi.percent)).catch(e=>cleanupErrors.push(String(e)));await writeFile(join(output,'results.json'),JSON.stringify({checks,measurements,original,cleanupErrors,restored:{dpi:await dpi(),contrast:await contrast()}},null,2));console.log(checks.join('\n'));assert.equal(cleanupErrors.length,0);
}
