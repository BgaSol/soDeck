// T-025 Windows/WebView2 checks. Display settings are captured and restored.
import assert from 'node:assert/strict';
import net from 'node:net';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,until,stop,nativeWindow,delay} from './harness.mjs';
const output=resolve('.qa/t025/visual-'+Date.now());await mkdir(output,{recursive:true});
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:20000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args),contrast=(...args)=>ps('tests/desktop/high-contrast.ps1',args);
const original={dpi:await dpi(),contrast:await contrast()};
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files','network-proxy'].map(n=>resolve(`.qa/t025/packages/${n}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.terminal':12572,'sodeck.files':12573,'sodeck.network-proxy':12574});
for(const port of [12571,12572,12573,12574]){const s=net.createServer();await new Promise((ok,no)=>s.once('error',no).listen(port,'127.0.0.1',ok));await new Promise(ok=>s.close(ok));}
let app;const checks=[],metrics=[];const note=s=>{checks.push(s);console.log(s);};
try{
 app=await launch({data:join(output,'data'),shellPort:12571,pluginPort:12572});const page=app.page;
 await page.getByRole('heading',{name:'新建 SSH 连接',exact:true}).waitFor();
 const win=(await nativeWindow(app,'list')).find(w=>w.visible&&w.title==='新建连接 — SoDeck');assert.ok(win);
 const screen=async name=>page.screenshot({path:join(output,name+'.png')});
 const geometry=async()=>{
  const box=await page.getByRole('button',{name:'保存并连接',exact:true}).boundingBox();const viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight,overflow:document.documentElement.scrollWidth>innerWidth,dpr:devicePixelRatio}));
  assert.ok(box&&box.y>=0&&box.y+box.height<=viewport.height);assert.equal(viewport.overflow,false);return {...viewport,button:box};
 };
 for(const percent of [100,125,150,200]){
  await dpi('-Action','set','-Percent',String(percent));await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:900*percent/100,Height:600*percent/100});await delay(300);
  for(const themeId of ['dopamine','soft-light','soft-dark']){const pref=await rpc(page,'ui.theme.preference.get');await rpc(page,'ui.theme.choose',{themeId,expectedRevision:pref.revision});await delay(80);metrics.push({percent,themeId,...await geometry()});await screen(`home-${themeId}-${percent}`);}
 }
 note('Three native themes at 900×600 logical pixels and Windows 100/125/150/200% DPI keep actions visible without horizontal overflow');
 await dpi('-Action','set','-Percent','100');await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:900,Height:600});
 await contrast('-Action','set','-Flags',String(original.contrast.flags|1),'-Scheme',original.contrast.scheme??'');await until(async()=>(await rpc(page,'ui.theme.preference.get')).highContrast);await geometry();await screen('home-high-contrast');
 await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await page.emulateMedia({reducedMotion:'reduce'});await geometry();await screen('home-reduced-motion');
 note('Windows high contrast and WebView reduced-motion rendering retain readable form/action boundaries');
 const ime=(action,args=[],handle=win.handle)=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-Executable',app.executable,'-WindowHandle',String(handle),...args]);
 const name=page.getByLabel('显示名称',{exact:true});await nativeWindow(app,'show',{WindowHandle:win.handle});await name.fill('');await name.focus();
 await page.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
 const imeOriginal=await ime('state');let shifted=false;
 try{
  const current=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle),box=await name.boundingBox(),ratio=await page.evaluate(()=>devicePixelRatio);
  await nativeWindow(app,'click',{WindowHandle:win.handle,X:Math.round(current.clientX+(box.x+20)*ratio),Y:Math.round(current.clientY+(box.y+box.height/2)*ratio)});await ime('focus');
  await ime('language',['-Layout','0000000008040804']);
  for(let n=0;n<2;n++){await ime('keys',['-Keys','zhongwen']);await delay(250);if(await page.evaluate(()=>window.__imeComposing))break;await name.fill('');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;}
  assert.equal(await page.evaluate(()=>window.__imeComposing),true);await screen('home-ime-composition');await ime('keys',['-Keys','SPACE']);await until(async()=>!await page.evaluate(()=>window.__imeComposing));
  const events=await page.evaluate(()=>window.__imeEvents);assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));await writeFile(join(output,'ime-events.json'),JSON.stringify(events,null,2));
  await name.press('Enter');assert.equal((await rpc(page,'servers.list')).servers.length,0);note('Trusted Microsoft Pinyin composition and ordinary Enter do not submit or open a connection');
 }finally{await ime('focus');if(await page.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await ime('language',['-Layout',imeOriginal.focusLayout]);}
 await page.getByLabel('私钥',{exact:true}).check();await page.getByLabel('私钥文件',{exact:true}).fill('C:\\fixture\\preserved-key');
 await page.getByRole('button',{name:'选择文件',exact:true}).click();
 const picker=await until(async()=>(await nativeWindow(app,'list')).find(w=>w.title==='选择 SSH 私钥'));
 await nativeWindow(app,'close',{WindowHandle:picker.handle});await until(async()=>!(await nativeWindow(app,'list')).some(w=>w.title==='选择 SSH 私钥'));
 assert.equal(await page.getByLabel('私钥文件',{exact:true}).inputValue(),'C:\\fixture\\preserved-key');note('Native private-key picker cancellation preserves the draft path');
 await page.getByLabel('主机地址',{exact:true}).fill('isolated.invalid');await page.getByLabel('用户名',{exact:true}).fill('tester');
 await page.getByRole('button',{name:'仅保存',exact:true}).click();await page.getByText(/配置已保存，后续步骤未完成/).waitFor();assert.equal((await rpc(page,'servers.list')).servers.length,1);assert.equal((await rpc(page,'workbench.snapshot')).workspaces.length,0);
 const large=join(output,'oversized-key');await writeFile(large,Buffer.alloc(1024*1024+1,120));await page.getByLabel('私钥文件',{exact:true}).fill(large);await page.getByRole('button',{name:'重试后续步骤',exact:true}).click();await page.getByText(/配置已保存，后续步骤未完成/).waitFor();
 assert.equal((await rpc(page,'servers.list')).servers.length,1);assert.equal((await rpc(page,'resources.list')).ptys.length,0);note('Missing and oversized private-key files retain the one saved server, report credential failure, and create no workspace/PTY');
 await screen('home-key-error');assert.deepEqual(app.errors,[]);
}catch(error){await app?.page.screenshot({path:join(output,'failure.png')}).catch(()=>{});await writeFile(join(output,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,checks},null,2));throw error;}
finally{
 if(app){await rpc(app.page,'app.exit').catch(()=>{});await app.page.getByRole('button',{name:'放弃修改',exact:true}).click({timeout:1500}).catch(()=>{});await until(()=>app.process.exitCode!==null,5000).catch(()=>{});await stop(app);}
 await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await dpi('-Action','set','-Percent',String(original.dpi.percent));
 const result={output,checks,metrics,original,restored:{dpi:await dpi(),contrast:await contrast()},shutdown:app?.shutdown};await writeFile(join(output,'results.json'),JSON.stringify(result,null,2));await writeFile('.qa/t025/visual-latest.json',JSON.stringify(result,null,2));
}
