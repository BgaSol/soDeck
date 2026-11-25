import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createServer} from 'vite';
import {chromium} from 'playwright';

const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:720,height:480}});
const url=server.resolvedUrls.local[0];const errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.route(url,route=>route.fulfill({contentType:'text/html; charset=utf-8',body:`<html><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><section class="settings-page" id="settings"></section><script type="module">
import {ApplicationSettings} from '/application-settings.ts';
window.calls=[];window.state={enabled:false,state:'disabled',reason:null};window.os='windows';window.pending=null;window.fail=false;
window.__TAURI_INTERNALS__={invoke:async(command,args)=>{calls.push({command,args});if(command==='login_startup_state')return structuredClone(state);if(command==='set_close_behavior')return;if(command==='set_login_startup'){await new Promise(resolve=>pending=resolve);if(fail){fail=false;throw Error('系统拒绝修改');}state={enabled:args.enabled,state:args.enabled?'enabled':'disabled',reason:null};return structuredClone(state);}throw Error(command);}};
window.settings=new ApplicationSettings(()=>document.querySelector('#settings').innerHTML='<h2>应用与数据</h2>'+settings.render(os),async()=>{});
document.addEventListener('change',event=>settings.input(event.target));document.addEventListener('click',event=>{if(event.target.closest('[data-action="login-startup-retry"]'))void settings.refresh();});
settings.mount(true);window.ready=true;
</script></body></html>`}));
try{
  await page.goto(url);await page.waitForFunction(()=>window.ready);const toggle=page.getByRole('switch',{name:'开机启动并驻留托盘'});
  await toggle.waitFor();await toggle.isEnabled();assert.equal(await toggle.isChecked(),false);
  await toggle.check();assert.equal(await toggle.isDisabled(),true);
  await page.evaluate(()=>{pending();pending=null;});await page.getByText('已开启，下次登录系统时生效。').waitFor();assert.equal(await toggle.isChecked(),true);
  await page.evaluate(()=>fail=true);await toggle.uncheck();await page.evaluate(()=>{pending();pending=null;});await page.getByText(/系统拒绝修改/).waitFor();assert.equal(await toggle.isChecked(),true,'Failed write reads back actual OS state');
  await page.evaluate(()=>{state={enabled:false,state:'blocked',reason:'已被系统禁用'};window.dispatchEvent(new Event('focus'));});await page.getByText('已被系统禁用').waitFor();assert.equal(await toggle.isChecked(),false);
  for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);await toggle.focus();assert.equal(await toggle.evaluate(n=>n===document.activeElement),true);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);fs.mkdirSync('artifacts/application-settings',{recursive:true});await page.screenshot({path:`artifacts/application-settings/${theme}.png`});}
  await page.evaluate(()=>{settings.mount(false);window.before=calls.length;window.dispatchEvent(new Event('focus'));});assert.equal(await page.evaluate(()=>calls.length),await page.evaluate(()=>before),'Leaving settings stops OS queries on focus');
  await page.evaluate(()=>settings.dispose());assert.deepEqual(errors,[]);
  console.log('Application settings: system read, pending toggle, write failure/readback, external disable, keyboard focus, three themes and page lifetime passed.');
}catch(error){console.error(await page.evaluate(()=>({calls,state,settings:{...settings},html:document.body.innerHTML})));throw error;}finally{await browser.close();await server.close();}
