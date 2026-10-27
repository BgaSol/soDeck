import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,delimiter} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {launch,rpc,until,stop,delay,nativeWindow} from './harness.mjs';
const output=resolve('.qa/t013-multihop/visual');await mkdir(output,{recursive:true});
const config=JSON.parse(await readFile('.qa/t013-multihop/linux/config.json','utf8'));
process.env.SODECK_PLUGIN_PACKAGES=['terminal','files'].map(p=>resolve(`resources/${p}.sodeck-plugin`)).join(delimiter);
process.env.SODECK_PLUGIN_DEBUG_PORTS=JSON.stringify({'sodeck.files':9872,'sodeck.terminal':9873});
const ps=async(file,args=[])=>JSON.parse((await promisify(execFile)('powershell.exe',['-NoProfile','-File',file,...args],{windowsHide:true,timeout:20000})).stdout);
const dpi=(...args)=>ps('tests/desktop/dpi-windows.ps1',args),contrast=(...args)=>ps('tests/desktop/high-contrast.ps1',args);
const original={dpi:await dpi(),contrast:await contrast()};let app,win;const checks=[];
async function screenshot(name){await app.page.screenshot({path:join(output,name+'.png')});}
async function editor(){await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await app.page.getByRole('dialog').getByRole('button',{name:'添加服务器',exact:true}).click();}
async function fill(name,host,user){await app.page.getByRole('textbox',{name:'显示名称',exact:true}).fill(name);await app.page.getByRole('textbox',{name:'主机地址',exact:true}).fill(host);await app.page.getByRole('textbox',{name:'用户名',exact:true}).fill(user);}
try{
 app=await launch({data:join(output,'data-'+Date.now()),shellPort:9871,pluginPort:9872});win=(await nativeWindow(app,'list')).find(w=>w.visible);
 await editor();
 const password=()=>app.page.getByLabel(/^登录密码/).filter({visible:true});
 for(let index=3;index>=0;index--){
   const node=config.nodes[index];await fill(index===3?'内网目标 D · 中文草稿':`跳板 ${node.name}`,node.host,node.username);
   await app.page.getByRole('textbox',{name:'端口',exact:true}).fill(String(node.port));
   await password().fill(`multihop-${node.name.toLowerCase()}-fixture`);
   if(index>0){await app.page.getByLabel('连接方式').filter({visible:true}).selectOption('sshJump');await app.page.getByRole('button',{name:'添加跳板服务器',exact:true}).click();}
 }
 for(let index=0;index<3;index++){
   await app.page.getByRole('button',{name:'保存并选用'}).click();
   const expected=index===2?'内网目标 D · 中文草稿':`跳板 ${config.nodes[index+1].name}`;
   await until(async()=>await app.page.getByRole('textbox',{name:'显示名称',exact:true}).inputValue()===expected);
   assert.equal(await password().inputValue(),`multihop-${config.nodes[index+1].name.toLowerCase()}-fixture`);
   assert.equal(await app.page.getByRole('button',{name:'添加跳板服务器',exact:true}).evaluate(e=>e===document.activeElement),true);
 }
 assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,0);
 assert.equal((await rpc(app.page,'servers.routes.v2.list')).servers.length,3);
 checks.push('MH-01: four nested drafts preserve independent passwords/names; each save returns focus to parent; no gateway workspace opens');
 const ime=(action,args=[])=>ps('tests/desktop/ime-windows.ps1',['-Action',action,'-AppProcessId',String(app.process.pid),'-WindowHandle',String(win.handle),...args]);
 const name=app.page.getByRole('textbox',{name:'显示名称',exact:true});await nativeWindow(app,'show',{WindowHandle:win.handle});await name.fill('');await name.focus();
 await app.page.evaluate(()=>{window.__imeEvents=[];window.__imeComposing=false;for(const type of ['compositionstart','compositionupdate','compositionend','input'])document.addEventListener(type,e=>{if(type==='compositionstart')window.__imeComposing=true;if(type==='compositionend')window.__imeComposing=false;window.__imeEvents.push({type,data:e.data,isTrusted:e.isTrusted});},true);});
 await ime('focus');await name.focus();
 const imeOriginal=await ime('state');let shifted=false;
 try{
   await ime('language',['-Layout','0000000008040804']);
   for(let n=0;n<2;n++){await ime('keys',['-Keys','zhongwen']);await delay(200);if(await app.page.evaluate(()=>window.__imeComposing))break;await name.fill('');await ime('keys',['-Keys','SHIFT']);shifted=!shifted;}
   assert.equal(await app.page.evaluate(()=>window.__imeComposing),true);await screenshot('ime-composition');await ime('keys',['-Keys','SPACE']);
   await until(async()=>!await app.page.evaluate(()=>window.__imeComposing));const events=await app.page.evaluate(()=>window.__imeEvents);
   assert.ok(events.some(e=>e.type==='compositionstart'&&e.isTrusted));assert.ok(events.some(e=>e.type==='input'&&e.isTrusted&&/[\u3400-\u9fff]/.test(e.data??'')));
   assert.equal((await rpc(app.page,'servers.routes.v2.list')).servers.length,3);await writeFile(join(output,'ime-events.json'),JSON.stringify(events,null,2));
   checks.push('MH-10: trusted Microsoft Pinyin composition in target draft commits Chinese text without submitting the parent form');
 }finally{if(await app.page.evaluate(()=>window.__imeComposing))await ime('keys',['-Keys','ESC']);if(shifted)await ime('keys',['-Keys','SHIFT']);await ime('language',['-Layout',imeOriginal.focusLayout]);await name.fill('内网目标 D · 中文草稿');}
 for(const percent of [100,200]){
   await dpi('-Action','set','-Percent',String(percent));await delay(300);
   await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(900*percent/100),Height:Math.round(600*percent/100)});
   await nativeWindow(app,'show',{WindowHandle:win.handle});
   for(const mode of ['dopamine','light','dark']){
     const p=await rpc(app.page,'ui.theme.preference.get');await rpc(app.page,'ui.theme.choose',{themeId:mode==='dopamine'?mode:'soft-'+mode,expectedRevision:p.revision});await delay(250);
     const button=app.page.getByRole('button',{name:'保存并打开'}),box=await button.boundingBox();
     const size=await app.page.evaluate(()=>({width:innerWidth,height:innerHeight}));assert.ok(box.y>=0&&box.y+box.height<=size.height);
     await button.focus();await app.page.keyboard.press('Tab');assert.equal(await app.page.getByRole('button',{name:'关闭对话框'}).evaluate(e=>e===document.activeElement),true);
     await screenshot(`editor-${mode}-${percent}`);await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(output,`native-${mode}-${percent}.png`)});
   }
 }
 checks.push('MH-10: Windows 100/200% DPI, 900×600 logical window, three themes, fixed footer and focus wrap');
 await contrast('-Action','set','-Flags',String(original.contrast.flags|1),'-Scheme',original.contrast.scheme??'');
 await until(async()=>(await rpc(app.page,'ui.theme.preference.get')).highContrast);await screenshot('high-contrast');
 await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');
 await app.page.emulateMedia({reducedMotion:'reduce'});await screenshot('reduced-motion');
 await app.page.getByRole('button',{name:'保存并打开'}).click();
 for(let i=0;i<4;i++){
   await until(async()=>{const d=app.page.getByRole('dialog',{name:'确认服务器身份',exact:true});return await d.count()&&(await d.innerText()).includes(`${i===3?'目标':'跳板'} ${i+1}/4`);},30000);
   await screenshot(`trust-${i+1}-of-4`);await app.page.getByRole('button',{name:'信任并连接',exact:true}).click();
 }
 await until(async()=>(await rpc(app.page,'resources.list')).ptys.some(p=>p.state==='active'),30000);
 const servers=(await rpc(app.page,'servers.routes.v2.list')).servers,target=servers.find(s=>s.name==='内网目标 D · 中文草稿');
 assert.equal((await rpc(app.page,'workbench.snapshot')).workspaces.length,1);
 checks.push('MH-03: all four native trust dialogs identify exact k/N and final target; saved per-node passwords authenticate to OpenSSH');
 await app.page.locator('.connection-status').click();await app.page.getByRole('dialog',{name:'连接路径'}).waitFor();await app.page.getByRole('button',{name:'查看完整路径',exact:true}).first().click();await app.page.getByText('4/4 · 目标 · 内网目标 D · 中文草稿',{exact:true}).waitFor();await screenshot('path-details');
 assert.equal(await app.page.evaluate(()=>document.querySelector('.connection-status')?.closest('.statusbar')!==null),true);
 await app.page.keyboard.press('Escape');
 await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
 // Use the picker delete action for the saved gateway; refusal is reviewable before any stop.
 const gateway=servers.find(s=>s.connectionRoute.kind==='direct');
 const row=app.page.locator('.picker-row').filter({has:app.page.getByRole('button',{name:`删除 ${gateway.name}`,exact:true})});
 assert.equal(await row.count(),1);{await row.getByRole('button',{name:/删除/}).click();await until(async()=>/内网目标 D/.test(await app.page.getByRole('dialog').innerText()));await screenshot('delete-dependency');await app.page.keyboard.press('Escape');}
 await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
 await app.page.getByRole('button',{name:`编辑 ${gateway.name}`,exact:true}).click();
 await app.page.getByLabel('连接方式').selectOption('sshJump');
 assert.equal(await app.page.getByLabel('跳板服务器').count(),0);
 await app.page.keyboard.press('Escape');
 checks.push('MH-02: deleting A lists transitive target D; editing A excludes all descendants as cyclic candidates');
 assert.deepEqual(app.errors,[]);
}finally{
 await stop(app);await contrast('-Action','set','-Flags',String(original.contrast.flags),'-Scheme',original.contrast.scheme??'');await dpi('-Action','set','-Percent',String(original.dpi.percent));
 await writeFile(join(output,'results.json'),JSON.stringify({checks,original,restored:{dpi:await dpi(),contrast:await contrast()}},null,2));console.log(checks.join('\n'));
}
