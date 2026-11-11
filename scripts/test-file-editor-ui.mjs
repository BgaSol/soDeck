// Shipped editor bundle, real browser editing, bounded search and byte-limit regression.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createServer} from 'vite';
import {chromium} from 'playwright';
const npm=process.env.npm_execpath||path.join(path.dirname(process.execPath),'node_modules/npm/bin/npm-cli.js');
const built=spawnSync(process.execPath,[npm,'run','build','--prefix','plugins/files'],{stdio:'inherit',windowsHide:true});if(built.status!==0)throw new Error('File editor build failed');
const vite=await createServer({configFile:false,optimizeDeps:{noDiscovery:true,include:[]},server:{watch:null,host:'127.0.0.1',port:0}});await vite.listen();const url=vite.resolvedUrls.local[0];
const browser=await chromium.launch({headless:true}),page=await browser.newPage({viewport:{width:1100,height:760}}),errors=[];
page.on('pageerror',error=>{errors.push(String(error));console.error(String(error));});page.setDefaultTimeout(10000);
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
 assert.deepEqual(errors,[]);console.log('File editor passed: real input/undo, byte limits, current-model search, dense full navigation, syntax, minimap, clipboard, state and narrow themes');
}finally{await browser.close();await vite.close();}
