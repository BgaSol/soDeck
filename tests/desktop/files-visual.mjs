import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {filesHarness,snapshot,locate,invoke} from './files-harness.mjs';
import {rpc,until,nativeWindow,pluginRpc,delay} from './harness.mjs';
const h=await filesHarness('visual',9711),checks=[];
const original=process.env.SODECK_THEME_FILE;
try{
  for(const [name,themeFile]of [['graphite','themes/graphite.json'],['paper','tests/fixtures/themes/paper.json']]){
    process.env.SODECK_THEME_FILE=resolve(themeFile);const app=await h.start(),server=await h.create('文件视觉 · '+name);await rpc(app.page,'workspaces.open',{serverId:server.id});
    const file=await h.view('sodeck.files',server.id),term=await h.view('sodeck.terminal',server.id);await until(async()=>(await snapshot(file)).state==='ready',25000);
    const win=(await nativeWindow(app,'list')).find(w=>w.visible);await nativeWindow(app,'show',{WindowHandle:win.handle});const sizes=[await resizeLogical(app,win,1280,800)];
    const data=await locate(file,'/srv/sodeck-t008/ordinary');await invoke(file,'files.toggle',{id:data.selected});await until(async()=>(await snapshot(file)).rows.some(r=>r.name==='sparse-5GiB.bin'));
    const theme=JSON.parse(await readFile(themeFile,'utf8'));for(const key of ['canvas','surface','text','selected'])assert.equal(await file.evaluate(k=>getComputedStyle(document.documentElement).getPropertyValue('--'+k).trim(),key),theme.colors[key]);
    await locate(file,'/srv/sodeck-t008/ordinary/sparse-5GiB.bin');await until(async()=>await file.getByRole('treeitem').filter({hasText:'sparse-5GiB.bin'}).count()>0);await delay(200);
    await nativeWindow(app,'capture',{WindowHandle:win.handle,OutputPath:join(h.output,name+'.png')});
    await locate(file,'/srv/sodeck-t008/ordinary/link-file');assert.equal((await snapshot(file)).details.targetAttributes.size,'1024');await file.getByRole('button',{name:'定位目标',exact:true}).click();await until(async()=>(await snapshot(file)).path==='/srv/sodeck-t008/ordinary/1024.bin');
    const malicious="/srv/sodeck-t008/ordinary/space quote' $(do-not-run)\n";await invoke(file,'files.locate',{text:malicious});await until(async()=>(await snapshot(file)).details?.attributes.size==='55');await file.getByRole('button',{name:'刷新目录与挂载'}).click();await file.getByRole('button',{name:'复制完整路径',exact:true}).click();assert.equal((await pluginRpc(file,'clipboard.readText')).text,malicious);
    await locate(file,'/srv/sodeck-t008/ordinary');await invoke(file,'files.toggle',{id:(await snapshot(file)).selected});let s=await snapshot(file);if(!s.rows.some(r=>r.name==='invalid-\\xff')){await invoke(file,'files.toggle',{id:s.selected});s=await snapshot(file);}const invalid=s.rows.find(r=>r.name==='invalid-\\xff');assert.ok(invalid);await invoke(file,'files.select',{id:invalid.id});await until(async()=>(await snapshot(file)).details?.attributes.size==='44');await file.getByRole('button',{name:'刷新目录与挂载'}).click();await file.getByRole('button',{name:'复制转义路径',exact:true}).click();
    assert.match((await pluginRpc(file,'clipboard.readText')).text,/invalid-\\xff$/);
    const tree=file.getByRole('tree');await tree.focus();await file.keyboard.press('Home');await file.keyboard.press('ArrowDown');await expectSelected(file);
    await file.emulateMedia({reducedMotion:'reduce',forcedColors:'active'});await file.evaluate(()=>document.documentElement.style.fontSize='19.5px');sizes.push(await resizeLogical(app,win,900,600));await delay(200);await file.screenshot({path:join(h.output,name==='graphite'?'large-text.png':'paper-large-text.png')});
    await file.emulateMedia({forcedColors:'none'});await file.evaluate(()=>document.documentElement.style.fontSize='13px');
    const view=(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.pluginId==='sodeck.files');await rpc(app.page,'views.move',{viewId:view.viewId,placement:'right'});await until(async()=>(await rpc(app.page,'workbench.snapshot')).views.find(v=>v.viewId===view.viewId).placement==='right');await rpc(app.page,'views.move',{viewId:view.viewId,placement:'left'});
    await tree.click();await until(async()=>await file.evaluate(()=>document.hasFocus()));await invoke(file,'files.refresh');await until(async()=>!(await snapshot(file)).loading);assert.equal(await file.evaluate(()=>document.hasFocus()),true);
    await term.locator('.xterm-helper-textarea').click();await until(async()=>await term.evaluate(()=>document.hasFocus()));await invoke(file,'files.refresh');await until(async()=>!(await snapshot(file)).loading);assert.equal(await term.evaluate(()=>document.hasFocus()),true);
    await nativeWindow(app,'close',{WindowHandle:win.handle});await rpc(app.page,'app.show');assert.equal((await rpc(app.page,'workbench.snapshot')).views.length,2);
    checks.push({theme:name,nativeWindow:win,sizes,passed:['two theme token sets','1280x800 and 900x600 logical client sizes','150% text enlargement','reduced motion and forced colors','keyboard tree and exact/escaped clipboard','symlink target size','left/right docking','file refresh preserves user focus in either surface','tray restore preserves workspace']});await h.stop();
  }
}finally{await writeFile(join(h.output,'results.json'),JSON.stringify(checks,null,2));await h.stop();if(original===undefined)delete process.env.SODECK_THEME_FILE;else process.env.SODECK_THEME_FILE=original;}
async function expectSelected(page){await page.waitForFunction(()=>Boolean(document.querySelector('[role=treeitem][aria-selected=true]')));}
async function resizeLogical(app,win,width,height){
  const before=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle),scale=before.dpi/96;
  await nativeWindow(app,'resize',{WindowHandle:win.handle,Width:Math.round(width*scale)+before.width-before.clientWidth,Height:Math.round(height*scale)+before.height-before.clientHeight});
  await until(async()=>await app.page.evaluate(({width,height})=>Math.abs(innerWidth-width)<=1&&Math.abs(innerHeight-height)<=1,{width,height}));
  const actual=(await nativeWindow(app,'list')).find(w=>w.handle===win.handle);
  assert.equal(await app.page.locator('.sessions-panel').evaluate(element=>element.getBoundingClientRect().width),320,'responsive rules must preserve the workbench sidebar width');
  return{requestedLogical:{width,height},actual,webview:await app.page.evaluate(()=>({width:innerWidth,height:innerHeight,devicePixelRatio}))};
}
