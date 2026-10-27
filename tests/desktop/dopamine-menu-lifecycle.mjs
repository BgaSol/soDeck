import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {launch,rpc,until,stop,nativeWindow,delay} from './harness.mjs';
const output=resolve('.qa/t012/menu-lifecycle');await mkdir(output,{recursive:true});
process.env.SODECK_PLUGIN_PACKAGES=[resolve('resources/terminal.sodeck-plugin'),resolve('resources/files.sodeck-plugin')].join(delimiter);
let app;const checks=[];
const popups=()=>app.browser.contexts().flatMap(c=>c.pages()).filter(p=>!p.isClosed()&&p.url().includes('application-menu'));
async function open(owner,key){await rpc(owner,'windows.control',{action:'show'});await delay(120);const trigger=owner.getByRole('button',{name:'应用菜单'});await until(async()=>await trigger.getAttribute('aria-expanded')==='false');if(key){await trigger.focus();await trigger.press(key);}else await trigger.click();const popup=await until(()=>popups()[0]);await popup.getByRole('menu').waitFor();return popup;}
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9861,pluginPort:9862});const main=(await nativeWindow(app,'list')).find(w=>w.title.includes('SoDeck'));
 for(const key of ['Enter','Space','ArrowDown']){const popup=await open(app.page,key);await until(async()=>await popup.getByRole('menuitem',{name:/外观/}).evaluate(el=>el===document.activeElement));await popup.keyboard.press('End');assert.match(await popup.evaluate(()=>document.activeElement.textContent),/退出/);await popup.keyboard.press('Home');await popup.keyboard.press('Escape');await until(()=>popup.isClosed());}
 checks.push('Enter/Space/Down start on first item; End/Home and Escape work');
 for(const [id,title] of [['plugins','插件'],['resources','运行资源'],['calls','调用记录']]){const popup=await open(app.page);await popup.getByRole('menuitem',{name:id==='plugins'?'管理插件':title,exact:true}).click();await app.page.getByRole('dialog').waitFor();assert.match(await app.page.getByRole('dialog').innerText(),new RegExp(title));await app.page.keyboard.press('Escape');}
 for(const label of ['重试保存 偏好与清理记录','找回所有窗口']){const popup=await open(app.page);await popup.getByRole('menuitem',{name:label,exact:true}).click();await until(()=>popup.isClosed());}
 const hidden=await open(app.page);await nativeWindow(app,'close',{WindowHandle:main.handle});await until(()=>hidden.isClosed());await until(async()=>!(await nativeWindow(app,'list')).find(w=>w.handle===main.handle).visible);await nativeWindow(app,'show',{WindowHandle:main.handle});
 const minimized=await open(app.page);await nativeWindow(app,'minimize',{WindowHandle:main.handle});await until(()=>minimized.isClosed());await nativeWindow(app,'show',{WindowHandle:main.handle});
 const moved=await open(app.page);await nativeWindow(app,'move',{WindowHandle:main.handle,X:30,Y:30});await until(()=>moved.isClosed());
 const server=await rpc(app.page,'servers.create',{name:'菜单窗口生命周期',host:'127.0.0.1',port:1,username:'fixture',authType:'password'});await rpc(app.page,'workspaces.open',{serverId:server.id,target:'newWindow'});
 const child=await until(async()=>{for(const page of app.browser.contexts().flatMap(c=>c.pages()))if(page!==app.page&&await page.evaluate(()=>window.__SODECK_CONTEXT__?.windowId))return page;});
 await until(async()=>(await rpc(child,'workbench.snapshot')).views.some(v=>v.status==='error'));
 const expired=await open(child);await rpc(app.page,'workspaces.close',{workspaceId:server.id});await until(()=>expired.isClosed());assert.equal(popups().length,0);
 checks.push('Management/recall/retry actions; owner hide/minimize/move/close release popup without blocking main');
 const exit=await open(app.page);await exit.getByRole('menuitem',{name:'退出 SoDeck',exact:true}).click();await until(()=>app.process.exitCode!==null);assert.equal(app.process.exitCode,0);
 checks.push('Application menu exit completes normal process shutdown');await writeFile(join(output,'results.json'),JSON.stringify({passed:true,checks},null,2));console.log(checks.join('\n'));
}finally{await stop(app);}
