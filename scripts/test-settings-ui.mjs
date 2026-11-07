// Core-backed declaration forms and committed offline fonts. No business process is launched.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import readline from 'node:readline';
import {spawn,spawnSync} from 'node:child_process';
import {createServer} from 'vite';import {chromium} from 'playwright';
import {packageSettingsAssets} from './plugin-settings-assets.mjs';
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--bin','catalog-fixture','--locked'],{stdio:'inherit',windowsHide:true});
if(build.status!==0)throw new Error('Settings core fixture build failed');
const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-settings-ui-'));
const target=spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout.match(/^host: (.+)$/m)[1];
const child=spawn(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform==='win32'?'.exe':''}`),[path.join(root,'data')],{stdio:['pipe','pipe','inherit'],windowsHide:true});
const pending=[];
readline.createInterface({input:child.stdout}).on('line',line=>{const response=JSON.parse(line),p=pending.shift();response.error?p.reject(new Error(response.error)):p.resolve(response.value);});
child.on('exit',()=>{for(const p of pending.splice(0))p.reject(new Error('Settings core exited'));});
const call=(command,args={})=>new Promise((resolve,reject)=>{pending.push({resolve,reject});child.stdin.write(JSON.stringify({command,args})+'\n');});
const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true});const errors=[],fontRequests=[];
const page=await browser.newPage({viewport:{width:1000,height:760}});page.on('pageerror',e=>errors.push(String(e)));
await page.exposeBinding('settingsInvoke',(_,command,args)=>call(command,args));
await page.route(url+'fonts/**',async route=>{
  const parts=new URL(route.request().url()).pathname.split('/').filter(Boolean),plugin=parts[1],token=parts[3],relative=parts.slice(4).join('/');
  fontRequests.push({plugin,relative});
  try{const resource=await call('settings_resource',{pluginId:plugin,definitionToken:token,relative});await route.fulfill({body:Buffer.from(resource.bytes),contentType:resource.mime});}
  catch(e){await route.fulfill({status:404,body:String(e)});}
});
const harness=`<!doctype html><html lang="zh-CN" data-theme="dopamine"><head><meta charset="utf-8"></head><body><div class="modal-backdrop"><section class="layer settings-dialog"><header class="layer-heading"><h2>设置</h2><button id="close">×</button></header><div id="body" class="layer-content settings-content"></div></section></div><script type="module">
import '/style.css';import {ComponentSettings} from '/settings.ts';
window.__TAURI_INTERNALS__={invoke:window.settingsInvoke,convertFileSrc:p=>new URL('/fonts/'+p,location.href).href};
const body=document.querySelector('#body');let hidden=false;const app=window.settingsApp=new ComponentSettings(render);
function render(){if(hidden)return;const old=body.querySelector('#component-settings-page'),active=old?.contains(document.activeElement)?document.activeElement:null;body.innerHTML='<aside class="settings-nav"><div class="nav-label">偏好设置</div><button class="nav-item">外观</button>'+app.navigation()+'<div class="nav-label settings-management-label">应用管理</div><button class="nav-item">插件管理</button><button class="nav-item">软件更新</button><button class="nav-item">应用与数据</button></aside><section class="settings-page" id="component-settings-page"></section>';if(old)body.querySelector('#component-settings-page').replaceWith(old);if(active?.isConnected)active.focus({preventScroll:true});app.mount(body.querySelector('#component-settings-page'),true);}
document.addEventListener('click',e=>{const b=e.target.closest('[data-action]');if(!b)return;if(b.dataset.action==='component-settings')app.select(b.dataset.id);else void app.action(b.dataset.action);});
document.querySelector('#close').onclick=()=>{hidden=true;app.mount(null,false);body.replaceChildren();};window.reopen=()=>{hidden=false;render();};
await app.catalog();app.select(app.entries[0].pluginId+'/'+app.entries[0].pageId);window.ready=true;
</script></body></html>`;
await page.route(url,route=>route.fulfill({body:harness,contentType:'text/html'}));
const section=()=>page.locator('#component-settings-page');
const choose=title=>page.locator('.settings-nav').getByRole('button',{name:title,exact:true}).click();
const save=()=>section().getByRole('button',{name:'保存',exact:true}).click();
try{
  for(const plugin of ['terminal','files','network-proxy']){
    const metadata=JSON.parse(fs.readFileSync(`plugins/${plugin}/release.json`,'utf8')),directory=path.join(root,metadata.id);fs.mkdirSync(path.join(directory,'ui'),{recursive:true});
    fs.writeFileSync(path.join(directory,'entry'),'never executed');fs.writeFileSync(path.join(directory,'ui/index.html'),'never mounted');
    packageSettingsAssets(metadata,directory);
    fs.writeFileSync(path.join(directory,'plugin.json'),JSON.stringify({...metadata,version:'1.0.0',target,entry:'entry'}));
    await call('add_plugin',{directory});await call('set_plugin_enabled',{id:metadata.id,enabled:false});
  }
  await page.goto(url);await page.waitForFunction(()=>window.ready);await section().getByLabel('字号',{exact:true}).waitFor();
  await page.waitForFunction(()=>document.querySelector('[data-font-status]')?.textContent.includes('字体已加载'));
  assert.equal((await call('snapshot')).plugins.filter(p=>p.pid!==null).length,0);
  assert.equal(await page.locator('.settings-nav [data-action="component-settings"]').count(),4);
  assert.equal(await section().getByLabel('字号',{exact:true}).inputValue(),'14');
  const input=section().getByLabel('字号',{exact:true});await input.fill('18');
  assert.equal(await section().locator('[data-font-sample]').evaluate(el=>getComputedStyle(el).fontSize),'18px');
  await section().getByLabel('行高',{exact:true}).fill('1.6');
  assert.equal(await section().locator('[data-font-sample]').evaluate(el=>getComputedStyle(el).lineHeight),'28.8px');
  await input.evaluate(el=>window.preservedInput=el);await page.evaluate(()=>window.settingsApp.catalog());assert(await input.evaluate(el=>el===window.preservedInput));
  await choose('文件列表');await section().getByLabel('名称字号').waitFor();await choose('终端 ●');
  assert.equal(await input.inputValue(),'18');await page.locator('#close').click();await page.evaluate(()=>window.reopen());assert.equal(await input.inputValue(),'18');
  await section().getByRole('button',{name:'保存',exact:true}).focus();await page.keyboard.press('Enter');await page.waitForFunction(()=>document.querySelector('.settings-feedback')?.textContent.includes('已保存'));
  assert.equal(await section().locator('.settings-feedback').evaluate(el=>el===document.activeElement),true,'Saving moves focus to the result when the save button becomes disabled');
  const saved=await call('settings_page',{pluginId:'sodeck.terminal',pageId:'appearance'});assert.equal(saved.values.fontSize,18);
  await section().getByLabel('字号',{exact:true}).fill('20');
  await call('save_settings',{pluginId:'sodeck.terminal',pageId:'appearance',input:{definitionToken:saved.definitionToken,baseRevision:saved.revision,requestId:'second-window',values:{...saved.values,fontSize:16}}});
  await page.evaluate(s=>window.settingsApp.changed(s),await call('settings_page',{pluginId:'sodeck.terminal',pageId:'appearance'}));
  await section().getByRole('button',{name:'载入最新值'}).waitFor();assert.equal(await input.inputValue(),'20');
  await section().getByRole('button',{name:'载入最新值'}).click();await page.waitForFunction(()=>document.querySelector('#setting-fontSize')?.value==='16');assert.equal(await input.inputValue(),'16');
  await section().getByLabel('字体',{exact:true}).selectOption('local');await section().getByLabel('本机字体名称').fill('SoDeck Definitely Missing Font 98765');
  await page.waitForFunction(()=>document.querySelector('[data-font-status]')?.textContent.includes('找不到本机字体'));
  assert(await section().getByRole('button',{name:'保存',exact:true}).isDisabled());
  const discard=section().getByRole('button',{name:'放弃更改'}),defaults=section().getByRole('button',{name:'恢复默认'});
  await discard.focus();await page.keyboard.press('Enter');assert.equal(await discard.evaluate(el=>el===document.activeElement),true,'Discarding preserves footer focus');
  await defaults.focus();await page.keyboard.press('Enter');assert.equal(await defaults.evaluate(el=>el===document.activeElement),true,'Restoring defaults preserves footer focus');
  assert.equal(await input.inputValue(),'14');await discard.focus();await page.keyboard.press('Enter');assert.equal(await input.inputValue(),'16');
  for(const id of ['jetbrains-mono','source-code-pro','ibm-plex-mono']){
    await section().getByLabel('字体',{exact:true}).selectOption(id);
    await page.waitForFunction(()=>document.querySelector('[data-font-status]')?.textContent.includes('字体已加载'));
    const family=await section().locator('[data-font-sample]').evaluate(el=>getComputedStyle(el).fontFamily);assert(family.includes('SoDeckFont'));
  }
  await choose('网络隧道');await section().getByLabel('本机端口').selectOption('1');await section().getByLabel('固定端口').fill('0');
  await save();assert.equal((await call('settings_page',{pluginId:'sodeck.network-proxy',pageId:'defaults'})).values.fixedPort,1080);
  await section().getByLabel('固定端口').fill('2080');await save();await page.waitForFunction(()=>document.querySelector('.settings-feedback')?.textContent.includes('已保存，用于新建规则'));
  for(const theme of ['light','dark','dopamine'])for(const [width,height,scale]of [[1000,760,1],[720,480,1],[900,600,1.25],[1080,720,1.5]]){
    await page.setViewportSize({width,height});await page.evaluate(({theme,scale})=>{document.documentElement.dataset.theme=theme;document.documentElement.style.zoom=String(scale);},{theme,scale});
    await choose('网络隧道');
    assert.equal(await page.locator('.settings-content').evaluate(el=>el.scrollTop),0,'Focusing the last navigation item must scroll only the navigation');
    assert(await section().locator('.section-heading').evaluate(el=>el.getBoundingClientRect().top>=document.querySelector('.settings-content').getBoundingClientRect().top),'The settings title must remain inside the content area');
    const overflow=await page.locator('.settings-dialog').evaluate(el=>({width:el.scrollWidth,client:el.clientWidth}));assert(overflow.width<=overflow.client+2,JSON.stringify({theme,width,height,scale,overflow}));
    await section().locator('.settings-fields').evaluate(el=>el.scrollTop=0);
    assert(await section().getByRole('button',{name:'保存',exact:true}).evaluate(el=>{const r=el.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight&&r.right<=innerWidth;}),`Save stays visible at ${theme}/${width}/${scale}`);
    if(width===720&&scale===1){
      await choose('终端 ●');await section().locator('.settings-fields').evaluate(el=>el.scrollTop=0);
      const visible=await section().getByLabel('字号',{exact:true}).evaluate(el=>{const r=el.getBoundingClientRect(),pane=document.querySelector('.settings-fields').getBoundingClientRect();return {visible:r.top>=pane.top&&r.bottom<=pane.bottom,available:pane.height};});
      assert(visible.visible,`Font and font size controls must both fit without scrolling: ${JSON.stringify({theme,...visible})}`);
      fs.mkdirSync('artifacts/settings-ui',{recursive:true});await page.screenshot({path:`artifacts/settings-ui/terminal-${theme}-720.png`});
    }
  }
  await page.setViewportSize({width:1000,height:760});await page.evaluate(()=>document.documentElement.style.zoom='1');await choose('终端 ●');
  fs.mkdirSync('artifacts/settings-ui',{recursive:true});await page.screenshot({path:'artifacts/settings-ui/terminal.png'});
  assert(fontRequests.some(r=>r.relative.includes('jetbrains'))&&fontRequests.some(r=>r.relative.includes('source-code'))&&fontRequests.some(r=>r.relative.includes('ibm-plex')));
  assert.deepEqual(errors,[]);console.log('Settings UI passed: 4 disabled-plugin pages, real offline fonts, drafts/close/reopen, conflict, missing local font, port validation, 12 responsive/theme layouts');
}finally{
  await browser.close();await vite.close();child.stdin.end();await new Promise(resolve=>child.once('exit',resolve));
  if(path.dirname(path.resolve(root))!==path.resolve(os.tmpdir())||!path.basename(root).startsWith('sodeck-settings-ui-'))throw new Error('Unexpected cleanup path');
  fs.rmSync(root,{recursive:true,force:true});
}
