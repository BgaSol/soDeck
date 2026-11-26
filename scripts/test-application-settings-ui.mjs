import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createServer} from 'vite';
import {chromium} from 'playwright';

const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:720,height:480}});
const url=server.resolvedUrls.local[0],errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.route(url,route=>route.fulfill({contentType:'text/html; charset=utf-8',body:`<html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><div id="app"></div><script type="module">
import {GeneralSettings} from '/general-settings.ts';import {settings as settingsView,settingsNavigation} from '/settings-views.ts';import {updateDOM} from '/dom-update.ts';import {Shell} from '/shell.ts';
window.calls=[];window.state={enabled:false,state:'disabled',reason:null};window.os='windows';window.pending=null;window.fail=false;window.holdReads=false;window.reads=[];window.readFail=false;window.holdTheme=false;window.paints=0;
window.snapshot={os,preferences:{theme:'system',closeBehavior:'background'},version:'test',target:'test',dataDirectory:'data',logDirectory:'logs'};
window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
 calls.push({command,args});
 if(command==='login_startup_state'){const value=structuredClone(state);if(holdReads)await new Promise(resolve=>reads.push(resolve));if(readFail)throw Error('无法读取登录项');return value;}
 if(command==='set_close_behavior'){await new Promise((resolve,reject)=>window.closePending={resolve,reject});snapshot.preferences.closeBehavior=args.behavior;return;}
 if(command==='set_theme'){if(holdTheme)await new Promise((resolve,reject)=>window.themePending={resolve,reject});snapshot.preferences.theme=args.theme;return;}
 if(command==='set_login_startup'){await new Promise(resolve=>pending=resolve);if(fail){fail=false;throw Error('系统拒绝修改');}state={enabled:args.enabled,state:args.enabled?'enabled':'disabled',reason:null};return structuredClone(state);}
 throw Error(command);
}};
window.shell=new Shell(document.querySelector('#app'),true);shell.attach({setHostVisible(){},setHostCallbacks(changed){changed({window:{activeWorkspace:'w'},workspaces:[]});}},{setNavigation(){}});
window.section='general';window.paint=()=>{paints++;shell.updateSettingsNavigation(settingsNavigation(section,''));updateDOM(document.querySelector('#layer-content'),settingsView({...snapshot,os},section,settings.state));document.documentElement.dataset.theme=snapshot.preferences.theme==='system'?'light':snapshot.preferences.theme;};
window.settings=new GeneralSettings(paint,async()=>paint());
shell.onPage=()=>{document.querySelector('#layer-content').className='layer-content settings-content';paint();};shell.onConsoleVisibility=page=>settings.mount(page==='settings'&&section==='general');
window.showSection=next=>{section=next;shell.navigate('settings');settings.mount(shell.visibleConsolePage==='settings'&&section==='general');};
document.addEventListener('change',event=>{if(event.target.id==='login-startup')void settings.setLoginStartup(event.target.checked);if(event.target.id==='close-behavior')void settings.setCloseBehavior(event.target.value);});
document.addEventListener('click',event=>{const button=event.target.closest('[data-action]');if(!button)return;const action=button.dataset.action;if(action==='login-startup-retry')void settings.retry();if(action==='login-startup-update')void settings.updateLoginStartup();if(action==='theme')void settings.setTheme(button.dataset.id);if(action==='section')showSection(button.dataset.id);});
showSection('general');window.ready=true;
</script></body></html>`}));
const count=command=>page.evaluate(command=>calls.filter(x=>x.command===command).length,command);
const release=()=>page.evaluate(()=>{pending();pending=null;});
try{
 await page.goto(url);await page.waitForFunction(()=>window.ready);const toggle=page.getByRole('switch',{name:'开机启动并驻留托盘'}),close=page.locator('#close-behavior');
 await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);assert.equal(await toggle.isChecked(),false);
 await toggle.check();assert.equal(await toggle.isDisabled(),true);await release();await page.getByText('已开启，下次登录系统时生效。').waitFor();assert.equal(await toggle.isChecked(),true);
 await page.evaluate(()=>fail=true);await toggle.uncheck();await release();await page.getByText(/系统拒绝修改/).waitFor();assert.equal(await toggle.isChecked(),true,'Failed write reads back OS state');
 await page.evaluate(()=>{state={enabled:false,state:'blocked',reason:'已被系统禁用'};window.dispatchEvent(new Event('focus'));});await page.waitForFunction(()=>!document.querySelector('#login-startup').checked);
 assert.equal(await toggle.isChecked(),false);await page.evaluate(()=>settings.retry());await page.getByText('已被系统禁用').waitFor();
 await page.evaluate(()=>{window.os='macos';state={enabled:true,state:'stale',reason:'旧版登录项仍开启'};window.dispatchEvent(new Event('focus'));});
 const update=page.getByRole('button',{name:'更新登录项'});await update.waitFor();const before=await count('set_login_startup');
 await page.evaluate(()=>window.dispatchEvent(new Event('focus')));await update.waitFor();assert.equal(await count('set_login_startup'),before,'Refresh never migrates or enables login items');
 await update.click();assert.equal(await toggle.isDisabled(),true);await release();await page.getByText('已开启，下次登录系统时生效。').waitFor();assert.equal(await update.count(),0);
 await close.selectOption('exit');await page.evaluate(()=>paint());assert.equal(await close.isDisabled(),true,'Background rendering preserves pending close preference');assert.equal(await close.inputValue(),'exit');
 const writes=await count('set_close_behavior');await page.evaluate(()=>settings.setCloseBehavior('background'));assert.equal(await count('set_close_behavior'),writes);
 await page.evaluate(()=>closePending.reject(Error('关闭行为保存失败')));await page.locator('#close-behavior-feedback').filter({hasText:'关闭行为保存失败'}).waitFor();assert.equal(await close.inputValue(),'background');
 assert(!await page.locator('#login-startup-feedback').innerText().then(text=>text.includes('关闭行为保存失败')));
 await page.evaluate(()=>settings.refresh());assert.match(await page.locator('#close-behavior-feedback').innerText(),/关闭行为保存失败/,'Login reads cannot clear close failures');
 await close.selectOption('exit');await page.evaluate(()=>closePending.resolve());await page.waitForFunction(()=>!document.querySelector('#close-behavior').disabled);assert.equal(await close.inputValue(),'exit');
 await page.evaluate(()=>{holdTheme=true;});await page.locator('#theme-dark').click();await page.evaluate(()=>paint());assert.equal(await page.locator('#theme-light').isDisabled(),true);assert.equal(await close.isEnabled(),true);
 await page.evaluate(()=>themePending.reject(Error('主题保存失败')));await page.locator('#theme-feedback').filter({hasText:'主题保存失败'}).waitFor();assert.equal(await page.locator('#theme-system').getAttribute('aria-pressed'),'true');
 await page.evaluate(()=>{holdTheme=false;readFail=true;});await page.evaluate(()=>settings.refresh());assert.equal(await toggle.isDisabled(),true);assert.equal(await page.locator('#theme-dark').isEnabled(),true);assert.equal(await close.isEnabled(),true);
 await page.evaluate(()=>{readFail=false;});await page.getByRole('button',{name:'重新读取'}).click();await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);
 for(const state of ['unknown','unavailable']){
  await page.evaluate(state=>{window.state={enabled:false,state,reason:'登记不可确认'};return settings.refresh();},state);
  assert.equal(await toggle.isDisabled(),true);assert.equal(await close.isEnabled(),true);assert.equal(await page.locator('#theme-light').isEnabled(),true);
 }
 await page.evaluate(()=>{state={enabled:false,state:'disabled',reason:null};return settings.refresh();});
 for(const destination of ['connections','about','workbench']){
  await page.evaluate(destination=>{if(destination==='about')showSection('about');else if(destination==='workbench')shell.showWorkbench();else shell.navigate(destination);},destination);
  const reads=await count('login_startup_state');await page.evaluate(()=>{window.dispatchEvent(new Event('focus'));void settings.refresh();});assert.equal(await count('login_startup_state'),reads,destination+' does not query login registration');
  await page.evaluate(()=>showSection('general'));await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);
 }
 await page.evaluate(()=>{holdReads=true;state={enabled:true,state:'enabled',reason:null};void settings.refresh();});await page.waitForFunction(()=>reads.length===1);
 await page.evaluate(()=>{shell.navigate('connections');state={enabled:false,state:'disabled',reason:null};showSection('general');reads.shift()();});await page.waitForFunction(()=>reads.length===1);
 assert.equal(await toggle.isDisabled(),true,'Reentry waits for the current read, not the obsolete reply');await page.evaluate(()=>{holdReads=false;reads.shift()();});await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);assert.equal(await toggle.isChecked(),false);
 await toggle.check();await page.evaluate(()=>{shell.setWindowVisible(false);window.beforeReads=calls.filter(x=>x.command==='login_startup_state').length;pending();pending=null;});
 await page.waitForFunction(()=>!settings.state.startup.saving);assert.equal(await count('login_startup_state'),await page.evaluate(()=>beforeReads),'Hidden writes settle without a new OS query');
 await page.evaluate(()=>shell.setWindowVisible(true));await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);assert.equal(await toggle.isChecked(),true);
 fs.mkdirSync('artifacts/application-settings',{recursive:true});
 for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[1100,720],[720,480]]){
  await page.setViewportSize({width,height});await page.locator('#theme-'+theme).click();await page.waitForFunction(theme=>document.documentElement.dataset.theme===theme,theme);
  await page.waitForFunction(()=>!document.querySelector('#theme-light').disabled);
  await close.scrollIntoViewIfNeeded();await close.focus();assert.equal(await close.evaluate(n=>n===document.activeElement),true);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
  await page.screenshot({path:'artifacts/application-settings/'+theme+'-'+width+'.png'});
  await page.locator('.settings-page').evaluate(node=>node.scrollTop=0);await page.screenshot({path:'artifacts/application-settings/'+theme+'-'+width+'-top.png'});
 }
 await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>showSection('about'));assert.equal(await page.locator('#login-startup,#close-behavior,[data-action="theme"]').count(),0);await page.evaluate(()=>showSection('general'));
 await page.waitForFunction(()=>!document.querySelector('#login-startup').disabled);await page.evaluate(()=>{holdReads=true;void settings.refresh();});await page.waitForFunction(()=>reads.length===1);
 await page.evaluate(()=>{settings.dispose();window.beforePaints=paints;window.beforeCalls=calls.length;reads.shift()();window.dispatchEvent(new Event('focus'));});
 await page.evaluate(()=>Promise.resolve());assert.equal(await page.evaluate(()=>paints),await page.evaluate(()=>beforePaints));assert.equal(await page.evaluate(()=>calls.length),await page.evaluate(()=>beforeCalls));assert.deepEqual(errors,[]);
 console.log('General settings: independent pending/errors, real DOM refresh, OS readback, stale migration, route/window lifetime, queued reads, dispose, three themes and two sizes passed.');
}catch(error){console.error(await page.evaluate(()=>({calls,state,settings:settings.state,html:document.querySelector('#layer-content').innerHTML})));throw error;}finally{await browser.close();await server.close();}
