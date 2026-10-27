import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {join} from 'node:path';
import {writeFile} from 'node:fs/promises';
import {networkHarness} from './network-harness.mjs';
import {rpc,until,delay,nativeWindow} from './harness.mjs';
const h=await networkHarness('visual',9941);
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:20000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args),contrast=(...args)=>ps('tests/desktop/high-contrast.ps1',args);
const original={dpi:await dpi(),contrast:await contrast()};
try{
 const app=await h.start();await h.create();await h.trust();await h.open();const rule=await h.save({name:'浏览器内网入口 · 最终节点访问目标'});await h.run(rule);
 const page=await h.page(),win=(await nativeWindow(app,'list')).find(w=>w.visible);
 await page.locator('.network-detail-scroll').evaluate(e=>e.scrollTop=0);await page.screenshot({path:join(h.output,'ready.png')});
 await page.getByRole('button',{name:'编辑',exact:true}).click();
 const ime=(action,args=[])=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(win.handle),...args]);
 const name=page.getByLabel('名称',{exact:true});await nativeWindow(app,'show',{WindowHandle:win.handle});await name.fill('');await name.focus();
 await page.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
 await ime('focus');
 const surface=await app.page.locator(`[data-view-id="${h.view.viewId}"]`).boundingBox(),field=await name.boundingBox(),native=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle),scale=await page.evaluate(()=>devicePixelRatio);
 await nativeWindow(app,'click',{WindowHandle:win.handle,X:Math.round(native.clientX+(surface.x+field.x+field.width/2)*scale),Y:Math.round(native.clientY+(surface.y+field.y+field.height/2)*scale)});await name.fill('');await name.focus();
 const imeOriginal=await ime('state');let shifted=false;
 try{
   await ime('language',['-Layout','0000000008040804']);
   for(let n=0;n<2;n++){await ime('keys',['-Keys','zhongwen']);await delay(200);if(await page.evaluate(()=>window.__imeComposing))break;await name.fill('');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;}
   assert.equal(await page.evaluate(()=>window.__imeComposing),true);await ime('keys',['-Keys','SPACE']);await until(async()=>!await page.evaluate(()=>window.__imeComposing));
   const events=await page.evaluate(()=>window.__imeEvents);assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));await writeFile(join(h.output,'ime-events.json'),JSON.stringify(events,null,2));
   assert.equal((await h.invoke('network.snapshot')).items[0].rule.name,rule.name);h.note('Trusted Windows Pinyin composition edits the rule draft without submitting or altering saved configuration');
 }finally{if(await page.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await ime('language',['-Layout',imeOriginal.focusLayout]);await name.fill('中文规则草稿 · '+ '长名称'.repeat(30));}
 for(const percent of [100,200]){
   await dpi('-Action','set','-Percent',String(percent));await delay(300);await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(900*percent/100),Height:Math.round(600*percent/100)});await nativeWindow(app,'show',{WindowHandle:win.handle});
   for(const mode of ['dopamine','light','dark']){
     const pref=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:pref.revision});await delay(250);
     const button=page.getByRole('dialog').getByRole('button',{name:'应用并重启',exact:true}),box=await button.boundingBox(),size=await page.evaluate(()=>({w:innerWidth,h:innerHeight,dpr:devicePixelRatio}));assert.ok(box.y>=0&&box.y+box.height<=size.h,JSON.stringify({box,size}));await button.focus();await page.keyboard.press('Tab');assert.equal(await page.getByRole('button',{name:'关闭对话框',exact:true}).evaluate(e=>e===document.activeElement),true);
     await page.screenshot({path:join(h.output,`editor-${mode}-${percent}.png`)});await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(h.output,`native-${mode}-${percent}.png`)});
   }
 }
 h.note('Three themes at real Windows 100/200% DPI in a 900×600 logical window retain accessible form actions, scrolling and keyboard focus wrap');
 await contrast('-Action','set','-Flags',String(original.contrast.flags|1),'-Scheme',original.contrast.scheme??'');await until(async()=>(await rpc(app.page,'ui.theme.preference.get')).highContrast);await page.screenshot({path:join(h.output,'high-contrast.png')});
 await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await page.emulateMedia({reducedMotion:'reduce'});await page.screenshot({path:join(h.output,'reduced-motion.png')});h.note('Windows high contrast and reduced-motion rendering preserve form content and actions');
}finally{await h.close();await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await dpi('-Action','set','-Percent',String(original.dpi.percent));await writeFile(join(h.output,'environment.json'),JSON.stringify({original,restored:{dpi:await dpi(),contrast:await contrast()}},null,2));}
