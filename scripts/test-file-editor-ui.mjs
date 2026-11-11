// Shipped editor bundle, real browser editing, bounded search and byte-limit regression.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createServer} from 'vite';
import {chromium} from 'playwright';
import {packageMenuAssets} from './plugin-menu-assets.mjs';
const npm=process.env.npm_execpath||path.join(path.dirname(process.execPath),'node_modules/npm/bin/npm-cli.js');
const built=spawnSync(process.execPath,[npm,'run','build','--prefix','plugins/files'],{stdio:'inherit',windowsHide:true});if(built.status!==0)throw new Error('File editor build failed');
const menuDirectory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-editor-menu-'));
await packageMenuAssets(menuDirectory);
const vite=await createServer({configFile:false,optimizeDeps:{noDiscovery:true,include:[]},server:{watch:null,host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true}),page=await browser.newPage({viewport:{width:1100,height:760}}),errors=[];
page.on('pageerror',error=>{errors.push(String(error));console.error(String(error));});page.setDefaultTimeout(10000);
await page.addInitScript({path:path.join(menuDirectory,'ui/text-menu.js')});
await page.route(url+'file-editor/**',route=>{const relative=new URL(route.request().url()).pathname.slice('/file-editor/'.length);return route.fulfill({contentType:relative.endsWith('.css')?'text/css':'text/javascript',body:fs.readFileSync(path.join('plugins/files/ui/vendor',relative))});});
await page.route(url+'editor.html',route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; worker-src 'self'"><link rel="stylesheet" href="/src/ui.css"><link rel="stylesheet" href="/plugins/files/ui/style.css"><link rel="stylesheet" href="/file-editor/editor.css"></head><body><main id="root"><header class="editor-toolbar"><span class="path">/etc/nginx/nginx.conf</span><button class="sd-control primary" id="save">保存</button></header><section id="editor-host" class="code-editor"></section><footer><span id="status" class="status"></span></footer></main><script type="module">import {FileEditor} from '/file-editor/editor.js';window.changes=0;window.messages=[];window.clipboard='';window.sodeckClipboard={readText:async()=>window.clipboard,writeText:async text=>{window.clipboard=text;return true;}};window.mount=(text,path='/etc/nginx/nginx.conf',state,bom=false,eol='\\n')=>{window.editor?.dispose();document.querySelector('#editor-host').replaceChildren();document.querySelector('footer').innerHTML='<span id="status" class="status"></span>';window.editor=new FileEditor({host:document.querySelector('#editor-host'),footer:document.querySelector('footer'),text,path,state,bom,eol,notify:message=>{window.messages.push(message);document.querySelector('#status').textContent=message;},change:()=>window.changes++,position:()=>{},save:()=>{}});window.editor.theme({panel:'#ffffff',text:'#263044',muted:'#64748b','color-scheme':'light'});window.editor.readOnly(false);};window.mount('worker_processes auto;\\n# configuration\\nserver {\\n  server_name example.com;\\n}\\n');window.ready=true;</script></body></html>`}));
try{
 await page.goto(url+'editor.html');await page.waitForFunction(()=>window.ready);
 assert.equal(await page.evaluate(()=>editor.model.getLanguageId()),'nginx');
 await page.locator('.monaco-editor').click();await page.keyboard.press('Control+End');await page.keyboard.type('# edited');
 assert((await page.evaluate(()=>editor.value())).endsWith('# edited'));
 await page.keyboard.press('Control+z');assert(!(await page.evaluate(()=>editor.value())).endsWith('# edited'));
 const changes=await page.evaluate(()=>changes);
 await page.keyboard.press('Control+f');await page.getByRole('textbox',{name:'查找当前文件'}).fill('server');await page.waitForFunction(()=>document.querySelector('.editor-find-count').textContent.endsWith('2 处'));
 await page.keyboard.press('Enter');assert.equal(await page.evaluate(()=>editor.editor.getSelection().startLineNumber),4);
 await page.keyboard.press('Enter');assert.equal(await page.evaluate(()=>editor.editor.getSelection().startLineNumber),3);
 await page.keyboard.press('Shift+Enter');assert.equal(await page.evaluate(()=>editor.editor.getSelection().startLineNumber),4);
 assert.equal(await page.evaluate(()=>changes),changes,'Search does not edit');
 await page.getByLabel('文件语言').selectOption('plaintext');assert.equal(await page.evaluate(()=>editor.model.getLanguageId()),'plaintext');assert.equal(await page.evaluate(()=>changes),changes);
 const state=await page.evaluate(()=>editor.state()),text=await page.evaluate(()=>editor.value());await page.evaluate(({text,state})=>mount(text,'/etc/nginx/nginx.conf',state),{text,state});assert.deepEqual(await page.evaluate(()=>editor.state().search),state.search);assert.equal(await page.evaluate(()=>editor.model.getLanguageId()),'plaintext');
 await page.getByRole('button',{name:'关闭搜索'}).click();assert.equal(await page.locator('.minimap canvas').count()>0,true);
 await page.evaluate(()=>mount('a'.repeat(1048576),'dense.txt'));
 await page.evaluate(()=>editor.focus());await page.keyboard.press('Control+End');await page.keyboard.type('b');
 assert.equal(await page.evaluate(()=>editor.value().length),1048576);assert((await page.evaluate(()=>messages)).some(m=>m.includes('超过 1 MiB')));
 await page.keyboard.press('Control+z');assert.equal(await page.evaluate(()=>editor.value().length),1048576,'Rejected input must not undo earlier valid text');
 await page.evaluate(()=>editor.openSearch());const started=Date.now();await page.getByRole('textbox',{name:'查找当前文件'}).fill('a');await page.waitForFunction(()=>document.querySelector('.editor-find-count').textContent.includes('1,000+'));console.log('Dense 1 MiB search incl browser scheduling:',Date.now()-started,'ms');
 await page.evaluate(()=>editor.setSelection(1048000,1048000));await page.getByRole('button',{name:'下一处',exact:true}).click();assert((await page.evaluate(()=>editor.selection().active))>1048000,'Navigation covers hits outside first 1000');
 await page.evaluate(()=>mount('old\n','compose.yaml',undefined,true,'\r\n'));assert.equal(await page.evaluate(()=>editor.model.getLanguageId()),'yaml');assert.equal(await page.evaluate(()=>editor.bytes('a'.repeat(1048573)).length),1048576);assert.equal(await page.evaluate(()=>editor.setText('a'.repeat(1048574))),false);
 await page.evaluate(()=>editor.setSelection(0,3));await page.evaluate(()=>editor.focus());await page.keyboard.press('Control+c');assert.equal(await page.evaluate(()=>clipboard),'old');await page.evaluate(()=>window.clipboard='new');await page.keyboard.press('Control+v');await page.waitForFunction(()=>editor.value()==='new\n');await page.keyboard.press('Control+z');assert.equal(await page.evaluate(()=>editor.value()),'old\n');
 for(const [path,language]of [['config.toml','toml'],['.env.local','dotenv'],['Dockerfile.dev','dockerfile'],['other.conf','plaintext'],['script','shell']]){await page.evaluate(({path})=>mount(path==='script'?'#!/usr/bin/env bash\n# test\n':'# test\n',path),{path});assert.equal(await page.evaluate(()=>editor.model.getLanguageId()),language);}
 await page.evaluate(()=>mount('server {\n  server_name example.com;\n  # example\n}\n'.repeat(120),'/etc/nginx/nginx.conf'));await page.waitForTimeout(250);
 const selection=await page.evaluate(()=>editor.selection()),mini=await page.locator('.minimap').boundingBox();
 await page.mouse.click(mini.x+mini.width/2,mini.y+mini.height*0.7);assert((await page.evaluate(()=>editor.editor.getScrollTop()))>0);assert.deepEqual(await page.evaluate(()=>editor.selection()),selection,'Minimap scroll keeps the edit selection');
 await page.evaluate(()=>editor.editor.setScrollTop(0));
 fs.mkdirSync('artifacts/files-ui',{recursive:true});await page.screenshot({path:'artifacts/files-ui/editor-light.png'});
 await page.evaluate(()=>{const tokens={panel:'#202732',text:'#e5eaf3',muted:'#a5afbd',border:'#445064',subtle:'#202732','color-scheme':'dark'};for(const [key,value]of Object.entries(tokens))document.documentElement.style.setProperty('--'+key,value);editor.theme(tokens);});await page.waitForTimeout(150);await page.screenshot({path:'artifacts/files-ui/editor-dark.png'});
 await page.setViewportSize({width:330,height:650});await page.evaluate(()=>editor.openSearch());await page.waitForTimeout(100);assert(await page.evaluate(()=>document.body.scrollWidth<=innerWidth));await page.screenshot({path:'artifacts/files-ui/editor-narrow.png'});
 for(const [path,text]of [['data.json','{"port":8080,"enabled":true}'],['compose.yaml','port: 8080\n# config\nname: "web"'],['config.toml','[server]\nport = 8080\nname = "web"\n# config'],['config.ini','[main]\nkey=value\n; config'],['.env','PORT=8080\nNAME="web"\n# config'],['Dockerfile','FROM ubuntu:24.04\n# config\nRUN echo "hello"']]){
   await page.evaluate(({path,text})=>mount(text,path),{path,text});await page.waitForFunction(()=>new Set([...document.querySelectorAll('.view-lines span[class^="mtk"]')].filter(el=>el.textContent.trim()).map(el=>getComputedStyle(el).color)).size>=2);
 }
 const tokenColor=(value,exact=true)=>page.evaluate(({value,exact})=>{
   const span=[...document.querySelectorAll('.view-lines span[class^="mtk"]')].find(el=>exact?el.textContent.replaceAll('"','')===value:el.textContent.includes(value));
   if(!span)throw new Error(`Missing colored token: ${value}`);
   return getComputedStyle(span).color;
 },{value,exact});
 const mountColored=async(path,text)=>{
   await page.evaluate(({path,text})=>mount(text,path),{path,text});
   await page.waitForFunction(()=>[...document.querySelectorAll('.view-lines span[class^="mtk"]')].some(el=>el.textContent.replaceAll('"','')==='ordinary'));
 };
 const commentColors=[];
 const dollarText='// lineMarker\n/* blockMarker\nblockContinuation */\n'+JSON.stringify({plain:'ordinary',dollar:'$HOME',braced:'${HOME}',lone:'$'},null,2);
 for(const path of ['data.json','data.jsonc']){
   await mountColored(path,dollarText);
   const ordinary=await tokenColor('ordinary');
   for(const value of ['$HOME','${HOME}','$'])assert.equal(await tokenColor(value),ordinary,`${path}: dollar signs stay string content`);
   assert.equal(await page.evaluate(()=>editor.value()),dollarText,'Coloring does not edit the document');
   commentColors.push([await tokenColor('lineMarker',false),await tokenColor('blockContinuation',false)]);
 }
 assert.notEqual(commentColors[0][0],commentColors[1][0],'Only JSONC colors line comments');
 assert.notEqual(commentColors[0][1],commentColors[1][1],'Only JSONC colors continued block comments');
 assert.equal(commentColors[1][0],commentColors[1][1],'JSONC retains its block comment state');
 for(const [path,text]of [['.env','PLAIN="ordinary"\nVALUE="$HOME"'],['/etc/nginx/nginx.conf','set $plain "ordinary";\nset $value "$HOME";'],['config.toml','plain = "ordinary"\nvalue = "$HOME"']]){
   await mountColored(path,text);
   assert.notEqual(await tokenColor('$HOME'),await tokenColor('ordinary'),`${path}: shared variable coloring is unchanged`);
 }
 await page.evaluate(()=>mount('line\n'.repeat(300),'position.txt'));await page.waitForTimeout(50);await page.evaluate(()=>editor.editor.setScrollTop(400));const position=await page.evaluate(()=>editor.state());
 await page.evaluate(state=>{document.querySelector('#editor-host').style.display='none';mount('line\n'.repeat(300),'position.txt',state);editor.preferencesChanged({fontSize:18,lineHeight:1.5,tabSize:4,wordWrap:false,minimap:true},'monospace');},position);
 assert.equal(await page.evaluate(()=>editor.state().topLine),position.topLine,'Hidden candidate retains logical scroll anchor');
 await page.evaluate(()=>{document.querySelector('#editor-host').style.display='';editor.visibility(true);});assert(Math.abs((await page.evaluate(()=>editor.state().topLine))-position.topLine)<=1);
 const preferenceText=('line '+ 'long content '.repeat(15)+'\n').repeat(300),defaults={fontSize:14,lineHeight:1.5,tabSize:4,wordWrap:false,minimap:true};
 const preferenceSnapshot=()=>page.evaluate(()=>({view:editor.editor.saveViewState(),persisted:editor.state(),text:editor.value(),changes,readOnly:editor.isReadOnly()}));
 const mountPreferences=async(width=330)=>{await page.setViewportSize({width:800,height:600});await page.evaluate(({text,width})=>{const host=document.querySelector('#editor-host');host.style.display='';Object.assign(host.style,{position:'fixed',left:'30px',top:'40px',width:width+'px',height:'220px',flex:'none'});mount(text,'preferences.txt');editor.setText(text+'edited\n');editor.editor.setSelections([{selectionStartLineNumber:20,selectionStartColumn:9,positionLineNumber:20,positionColumn:3},{selectionStartLineNumber:30,selectionStartColumn:2,positionLineNumber:30,positionColumn:5},{selectionStartLineNumber:45,selectionStartColumn:4,positionLineNumber:45,positionColumn:4}]);editor.editor.setScrollPosition({scrollTop:400,scrollLeft:80});},{text:preferenceText,width});await page.waitForTimeout(30);};
 const assertPreferenceView=(before,after)=>{assert.deepEqual(after.view.cursorState,before.view.cursorState,'Preference changes retain all selections and their directions');assert.deepEqual(after.view.viewState.firstPosition,before.view.viewState.firstPosition,'Preference changes retain the logical top text');assert(Math.abs(after.view.viewState.firstPositionDeltaTop-before.view.viewState.firstPositionDeltaTop)<1,'Preference changes retain the top text pixel offset');assert.equal(after.text,before.text);assert.equal(after.changes,before.changes);assert.equal(after.readOnly,before.readOnly);assert.deepEqual(after.persisted.search,before.persisted.search);assert.equal(after.persisted.language,before.persisted.language);};
 for(const values of [{...defaults,fontSize:18},{...defaults,lineHeight:1.8},{...defaults,tabSize:8},{...defaults,wordWrap:true},{...defaults,minimap:false}]){
   await mountPreferences(values.minimap?330:430);if(!values.minimap)assert(await page.locator('.minimap').isVisible());const before=await preferenceSnapshot();await page.evaluate(values=>editor.preferencesChanged(values,'monospace'),values);const after=await preferenceSnapshot();assertPreferenceView(before,after);if(!values.minimap)assert.equal(await page.locator('.minimap').isVisible(),false);
   if(!values.wordWrap)assert.equal(after.view.viewState.scrollLeft,before.view.viewState.scrollLeft);
   await page.evaluate(()=>editor.model.undo());assert.equal(await page.evaluate(()=>editor.value()),preferenceText,'Preferences do not add or remove undo history');
 }
 await mountPreferences();await page.evaluate(()=>{editor.openSearch();editor.setSelection(0,0);});await page.getByRole('textbox',{name:'查找当前文件'}).fill('line');await page.waitForFunction(()=>document.querySelector('.editor-find-count').textContent.includes('处'));await page.getByLabel('文件语言').selectOption('yaml');await page.getByRole('textbox',{name:'查找当前文件'}).focus();const searchBefore=await preferenceSnapshot();
 await page.evaluate(()=>editor.preferencesChanged({fontSize:18,lineHeight:1.7,tabSize:8,wordWrap:true,minimap:false},'monospace'));assertPreferenceView(searchBefore,await preferenceSnapshot());assert(await page.getByRole('textbox',{name:'查找当前文件'}).evaluate(el=>el===document.activeElement),'Preferences keep search input focus');
 await page.evaluate(()=>editor.readOnly(true));const readOnlyBefore=await preferenceSnapshot();await page.evaluate(()=>editor.preferencesChanged({fontSize:16,lineHeight:1.5,tabSize:4,wordWrap:false,minimap:true},'monospace'));assertPreferenceView(readOnlyBefore,await preferenceSnapshot());
 await mountPreferences();const hiddenBefore=await preferenceSnapshot();await page.evaluate(()=>{editor.visibility(false);document.querySelector('#editor-host').style.display='none';editor.preferencesChanged({fontSize:18,lineHeight:1.5,tabSize:8,wordWrap:true,minimap:false},'monospace');editor.preferencesChanged({fontSize:22,lineHeight:1.5,tabSize:8,wordWrap:true,minimap:true},'monospace');});
 assert.deepEqual(await page.evaluate(()=>editor.state()),hiddenBefore.persisted,'A hidden live instance retains its original serializable state');
 await page.evaluate(()=>{const host=document.querySelector('#editor-host');host.style.display='';host.style.width='0px';host.style.height='0px';editor.visibility(true);});assert.deepEqual(await page.evaluate(()=>editor.state()),hiddenBefore.persisted,'Zero-sized layout does not consume the pending state');
 await page.evaluate(()=>{const host=document.querySelector('#editor-host');host.style.width='230px';host.style.height='220px';editor.readOnly(false);});assertPreferenceView(hiddenBefore,await preferenceSnapshot());await page.evaluate(()=>editor.visibility(true));assertPreferenceView(hiddenBefore,await preferenceSnapshot());
 await page.evaluate(()=>{editor.visibility(false);document.querySelector('#editor-host').style.display='none';editor.preferencesChanged({fontSize:16,lineHeight:1.5,tabSize:4,wordWrap:false,minimap:true},'monospace');editor.dispose();document.querySelector('#editor-host').style.display='';mount('fresh\n','fresh.txt');});assert.equal(await page.evaluate(()=>editor.editor.getSelections().length),1);assert.equal(await page.evaluate(()=>editor.state().version),1);
 await page.setViewportSize({width:800,height:600});
 await page.evaluate(()=>{document.querySelector('#editor-host').style.display='';mount('old\n'+'line\n'.repeat(300),'menu.txt');});
 const menu=page.locator('.file-editor-menu'),host=page.locator('#editor-host');
 const dimensions=async(width=100,height=70,left=30,top=40)=>{await host.evaluate((el,{width,height,left,top})=>{Object.assign(el.style,{position:'fixed',width:width+'px',height:height+'px',left:left+'px',top:top+'px',flex:'none'});},{width,height,left,top});await page.waitForTimeout(50);};
 const openMenu=async()=>{await host.evaluate(el=>{const r=el.getBoundingClientRect();el.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.right-2,clientY:r.bottom-2}));});await menu.waitFor({state:'visible'});};
 const menuGeometry=()=>menu.evaluate(el=>{const r=el.getBoundingClientRect(),list=el.querySelector('.sd-menu-list')||el,style=getComputedStyle(list);return {parent:el.parentElement===document.body,position:getComputedStyle(el).position,left:r.left,top:r.top,right:r.right,bottom:r.bottom,scrolling:list.scrollHeight>list.clientHeight,horizontal:['auto','scroll'].includes(style.overflowX)&&list.scrollWidth>list.clientWidth};});
 const assertMenu=async()=>{const g=await menuGeometry(),v=page.viewportSize();assert(g.parent&&g.position==='fixed',JSON.stringify(g));assert(g.left>=0&&g.top>=0&&g.right<=v.width&&g.bottom<=v.height,JSON.stringify(g));assert.equal(g.horizontal,false);return g;};
 assert.equal(await page.evaluate(()=>typeof window.SoDeckMenuPosition?.attachMenuPosition),'function','The independently packaged menu API is loaded');
 await dimensions();await openMenu();await assertMenu();
 for(const button of await menu.getByRole('menuitem').all()){await button.focus();assert(await button.evaluate(el=>{const r=el.getBoundingClientRect();return el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}),'Each narrow-editor menu item is reachable');}
 await page.keyboard.press('Escape');assert.equal(await menu.count(),0);assert(await page.evaluate(()=>editor.editor.hasTextFocus()));
 await dimensions(330,220);await openMenu();await dimensions();await assertMenu();await page.keyboard.press('Tab');assert.equal(await menu.count(),0);
 for(const [left,top]of [[0,0],[700,0],[0,530],[700,530]]){await dimensions(100,70,left,top);await openMenu();await assertMenu();await page.keyboard.press('Escape');}
 await page.setViewportSize({width:180,height:120});await dimensions(100,70,10,20);await openMenu();assert((await assertMenu()).scrolling);
 await page.keyboard.press('End');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'全选');assert(await menu.getByRole('menuitem',{name:'全选'}).evaluate(el=>{const r=el.getBoundingClientRect();return el.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2));}));
 await page.keyboard.press('Home');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'剪切');await page.keyboard.press('ArrowDown');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'复制');await page.keyboard.press('Escape');
 await page.setViewportSize({width:800,height:600});await dimensions(330,220);await page.evaluate(()=>editor.focus());await page.keyboard.press('Shift+F10');await menu.waitFor();await page.waitForFunction(()=>document.activeElement.textContent==='剪切');await page.keyboard.press('Escape');
 await openMenu();await host.evaluate(()=>editor.editor.setScrollTop(200));await page.waitForFunction(()=>!document.querySelector('.file-editor-menu'));assert.equal(await page.evaluate(()=>editor.editor.getScrollTop()),200);
 await openMenu();await page.evaluate(()=>editor.visibility(false));assert.equal(await menu.count(),0);await page.evaluate(()=>editor.visibility(true));
 await openMenu();await page.evaluate(()=>editor.readOnly(true));assert.equal(await menu.count(),0);await openMenu();await page.waitForFunction(()=>document.activeElement.textContent==='复制');await page.keyboard.press('ArrowDown');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'全选');await page.keyboard.press('Escape');await page.evaluate(()=>editor.readOnly(false));
 await page.evaluate(()=>{editor.setSelection(0,3);editor.focus();});await openMenu();await menu.getByRole('menuitem',{name:'复制',exact:true}).click();assert.equal(await page.evaluate(()=>clipboard),'old');assert(await page.evaluate(()=>editor.editor.hasTextFocus()));
 await openMenu();await menu.getByRole('menuitem',{name:'全选'}).press('Enter');assert.equal(await page.evaluate(()=>editor.selection().active),await page.evaluate(()=>editor.value().length));
 await page.evaluate(()=>{editor.setSelection(0,3);window.sodeckClipboard.writeText=async()=>false;});await openMenu();await menu.getByRole('menuitem',{name:'剪切'}).press('Space');assert((await page.evaluate(()=>editor.value())).startsWith('old\n'),'Failed clipboard writes never delete source text');
 for(const invalid of ['focus','selection','version','readOnly','dispose']){
   await page.evaluate(()=>{mount('old\n','menu.txt');editor.setSelection(0,3);window.finishRead=undefined;window.sodeckClipboard.readText=valid=>new Promise(resolve=>{window.finishRead=()=>resolve(valid()?'late':null);});});
   await dimensions(330,220);await openMenu();await menu.getByRole('menuitem',{name:'粘贴'}).click();await page.waitForFunction(()=>typeof finishRead==='function');
   await page.evaluate(invalid=>{if(invalid==='focus')document.querySelector('#save').focus();if(invalid==='selection')editor.setSelection(4,4);if(invalid==='version')editor.setText('new\n');if(invalid==='readOnly')editor.readOnly(true);if(invalid==='dispose')mount('replacement\n','other.txt');finishRead();},invalid);
   await page.waitForTimeout(20);assert.equal(await page.evaluate(()=>editor.value()),invalid==='version'?'new\n':invalid==='dispose'?'replacement\n':'old\n',`${invalid}: late clipboard input must not edit`);
 }
 await page.evaluate(()=>{mount('old\n','menu.txt');window.originalPosition=SoDeckMenuPosition.attachMenuPosition;window.pendingReady=[];window.positionDisposals=0;SoDeckMenuPosition.attachMenuPosition=(...args)=>{const real=originalPosition(...args);return {...real,ready:new Promise(resolve=>pendingReady.push(()=>real.ready.then(resolve))),dispose(){positionDisposals++;real.dispose();}}};});
 await dimensions(330,220);await openMenu();await page.keyboard.press('Escape');await openMenu();await page.evaluate(()=>pendingReady[0]());await page.waitForTimeout(20);assert(await page.evaluate(()=>editor.editor.hasTextFocus()),'A closed menu ready callback cannot steal focus');await page.evaluate(()=>pendingReady[1]());await page.waitForFunction(()=>document.activeElement.textContent==='剪切');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>positionDisposals),2);
 await page.evaluate(()=>{SoDeckMenuPosition.attachMenuPosition=originalPosition;});await openMenu();await page.evaluate(()=>editor.dispose());assert.equal(await menu.count(),0);
 assert.deepEqual(errors,[]);console.log('File editor passed: real input/undo, byte limits, current-model search, dense full navigation, syntax, minimap, clipboard, state, narrow themes and owned menus');
}finally{await browser.close();await vite.close();const target=path.resolve(menuDirectory);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert(path.basename(target).startsWith('sodeck-editor-menu-'));fs.rmSync(target,{recursive:true,force:true});}
