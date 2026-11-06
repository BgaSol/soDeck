// Real browser input and shipped file-menu code; no remote or native clipboard writes.
import assert from 'node:assert/strict';
import {before,after,test} from 'node:test';
import fs from 'node:fs';
import {createServer} from 'vite';
import {chromium} from 'playwright';

let server,browser;
before(async()=>{server=await createServer({server:{port:0,strictPort:false}});await server.listen();browser=await chromium.launch({headless:true});});
after(async()=>{await browser?.close();await server?.close();});
const fixture=async()=>{
  const page=await browser.newPage({viewport:{width:400,height:540}});
  await page.route('**/__interactions__',route=>route.fulfill({contentType:'text/html',body:'<html><head><style>@layer base,components,themes;</style><link rel="stylesheet" href="/ui.css"><link rel="stylesheet" href="/themes.css"></head><body><div id="chrome"></div><div id="tree-host"></div><script type="module">import {replace,patchChrome,icon} from "/ui.ts";window.render={replace,patchChrome,icon};</script></body></html>'}));
  await page.goto(server.resolvedUrls.local[0]+'__interactions__');await page.waitForFunction(()=>window.render);
  return page;
};
test('SVG refresh preserves the pressed button, focus and one real click',async()=>{
  const page=await fixture();
  try{
    await page.evaluate(()=>{window.clicks=0;const host=document.querySelector('#chrome');window.html='<button class="sd-icon" data-component-action="toggle" data-key="files">'+render.icon('folder')+'</button>';render.replace(host,html);window.pressed=host.firstChild;host.onclick=()=>clicks++;});
    const r=await page.locator('#chrome button').boundingBox();await page.mouse.move(r.x+r.width/2,r.y+r.height/2);await page.mouse.down();
    await page.evaluate(()=>render.replace(document.querySelector('#chrome'),html));
    assert.equal(await page.evaluate(()=>pressed.isConnected&&document.activeElement===pressed),true);
    await page.mouse.up();assert.equal(await page.evaluate(()=>clicks),1);
    await page.evaluate(()=>{document.querySelector('#chrome').firstChild.textContent='external update';render.replace(document.querySelector('#chrome'),html);});
    assert.equal(await page.locator('#chrome svg').count(),1);
  }finally{await page.close();}
});
test('chrome updates preserve unrelated controls and defer reordering until click completes',async()=>{
  const page=await fixture();
  try{
    await page.evaluate(()=>{const button=(key,label)=>'<button data-component-action="select" data-key="'+key+'">'+label+'</button>';window.beforeHTML=button('one','One')+button('two','Two');window.afterHTML=button('new','New')+button('one','Changed')+button('two','Two');const host=document.querySelector('#chrome');render.patchChrome(host,beforeHTML);window.first=host.children[0];window.second=host.children[1];window.clicks=0;host.onclick=()=>clicks++;});
    await page.locator('#chrome button').first().hover();await page.mouse.down();
    await page.evaluate(()=>render.patchChrome(document.querySelector('#chrome'),afterHTML));
    assert.equal(await page.evaluate(()=>first.isConnected&&first.textContent==='One'),true);
    await page.mouse.up();await page.waitForFunction(()=>first.textContent==='Changed');
    assert.equal(await page.evaluate(()=>clicks===1&&document.querySelector('[data-key="two"]')===second),true);
    await page.locator('[data-key="two"]').hover();await page.mouse.down();await page.mouse.move(350,160);await page.mouse.up();assert.equal(await page.evaluate(()=>clicks),1);
  }finally{await page.close();}
});

const fileFixture=async()=>{
  const page=await fixture();
  await page.addStyleTag({content:fs.readFileSync('plugins/files/ui/style.css','utf8')});
  await page.addScriptTag({content:fs.readFileSync('plugins/files/ui/tree.js','utf8')});
  await page.addScriptTag({content:fs.readFileSync('plugins/files/ui/transfer.js','utf8')});
  await page.evaluate(()=>{
    window.operations=[];window.clipboard={revision:'1',hasFiles:false};window.remote={remote:false,reason:'没有可粘贴的文件'};
    window.sodeckClipboard={writeText:async text=>{clipboard={revision:String(Number(clipboard.revision)+1),hasFiles:false};operations.push(['path',text]);}};
    const transfers=Object.create(FileTransfers.prototype);
    Object.assign(transfers,{contextEpoch:0,disposed:false,target:()=>tree.targetDirectory(),message:()=>{},data:async(action,args)=>{if(action==='input.revision')return {...clipboard};if(action==='stat')return {attrs:{kind:'directory'}};if(action==='input.clipboard')return {inputId:'local',revision:clipboard.revision};return {};},event:async args=>{if(args.action==='clipboardStatus')return {...remote};if(args.action==='pasteEntries')return {selectionId:args.selectionId,kind:'copy'};return {};},start:async params=>operations.push(['start',params]),download:async roots=>operations.push(['download',roots]),copy:async roots=>operations.push(['copy',roots]),mutate:async(kind,roots,target)=>operations.push([kind,roots,target])});
    window.transfers=transfers;
    window.tree=new FileTree(document.querySelector('#tree-host'),{data:transfers.data,event:async()=>{},request:async()=>{},download:()=>{},message:()=>{},transfers});
    const root=tree.node('/home'),dir=tree.node('/home/dw'),file=tree.node('/home/config.txt',{kind:'file',size:12});
    root.children=[dir.path,file.path];root.loaded=dir.loaded=true;tree.rootPath='/home';tree.expanded.add('/home');tree.selected=dir.path;tree.selection=new Set([dir.path]);tree.render();
    window.openMenu=(kind='directory')=>{const n=kind==='blank'?null:tree.nodes.get(kind==='file'?'/home/config.txt':'/home/dw');if(n){tree.selected=n.path;tree.selection=new Set([n.path]);}tree.showMenu(n,180,130);};
    window.sodeck={};
    document.querySelector('#tree-host').style.height='430px';document.querySelector('#tree-host').style.display='flex';
  });
  return page;
};
test('file menu has real paste states, stable target, short labels and cancellable late results',async()=>{
  const page=await fileFixture();
  try{
    await page.evaluate(()=>openMenu());const paste=page.getByRole('menuitem',{name:'粘贴',exact:true});
    await page.waitForFunction(()=>document.querySelector('.tree-menu button[title="没有可粘贴的文件"]'));
    assert.equal(await paste.isDisabled(),true);
    assert.equal(await page.getByRole('menuitem',{name:'展开 / 折叠',exact:true}).count(),0);
    assert.equal(await page.getByRole('menuitem',{name:'重命名…'}).count(),1);
    await page.evaluate(()=>{window.originalMenu=tree.menu;clipboard={revision:'2',hasFiles:true};tree.readMenuState();});
    await page.waitForFunction(()=>!document.querySelector('.tree-menu button[title="正在读取剪贴板"]')&&[...document.querySelectorAll('.tree-menu button')].some(b=>b.textContent==='粘贴'&&!b.disabled));
    assert.equal(await page.evaluate(()=>tree.menu===originalMenu),true);
    await paste.click();await page.waitForFunction(()=>operations.some(op=>op[0]==='start'));
    assert.equal(await page.evaluate(()=>operations.find(op=>op[0]==='start')[1].targetPath),'/home/dw');
    await page.evaluate(()=>{clipboard={revision:'3',hasFiles:false};remote={remote:true,clipboardId:'remote-one'};openMenu();});await paste.waitFor();await page.waitForFunction(()=>[...document.querySelectorAll('.tree-menu button')].some(b=>b.textContent==='粘贴'&&!b.disabled));
    await paste.click();await page.waitForFunction(()=>operations.filter(op=>op[0]==='start').length===2);
    await page.evaluate(()=>{remote={remote:false,reason:'复制来源属于其他连接'};openMenu();});await page.waitForFunction(()=>document.querySelector('button[title="复制来源属于其他连接"]'));assert.equal(await paste.isDisabled(),true);
    await page.evaluate(()=>{tree.closeMenu();transfers.pasteState=()=>new Promise(resolve=>window.resolveOld=resolve);openMenu();tree.closeMenu();openMenu('file');resolveOld({canPaste:true});});
    assert.equal(await page.getByRole('menuitem',{name:'粘贴',exact:true}).count(),0);
    assert.equal(await page.getByRole('menuitem',{name:'新建',exact:true}).count(),0);
    await page.evaluate(()=>{tree.selection=new Set(['/home/dw','/home/config.txt']);tree.showMenu(tree.nodes.get('/home/dw'),180,130);});
    assert.equal(await page.getByRole('menuitem',{name:'重命名…'}).count(),0);assert.equal(await page.getByRole('menuitem',{name:'复制',exact:true}).count(),1);
    await page.getByRole('menuitem',{name:'复制',exact:true}).click();assert.deepEqual(await page.evaluate(()=>operations.find(op=>op[0]==='copy')[1]),['/home/dw','/home/config.txt']);
    await page.evaluate(()=>{openMenu('file');tree.nodes.get('/home').children=['/home/dw'];tree.render();});
    assert.equal(await page.getByRole('menu',{name:'文件操作',exact:true}).count(),0);
  }finally{await page.close();}
});
test('file menu frame stays visible, submenus support keyboard and small viewports scroll only vertically',async()=>{
  const page=await fileFixture();const errors=[];page.on('pageerror',error=>errors.push(error.message));
  try{
    for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[400,540],[240,280]]){
      await page.setViewportSize({width,height});await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;openMenu();},theme);
      const geometry=await page.locator('.tree-menu').evaluate(menu=>{const r=menu.getBoundingClientRect(),s=getComputedStyle(menu),list=menu.querySelector('.tree-menu-list');return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,x:s.overflowX,y:s.overflowY,stroke:getComputedStyle(menu,'::before').borderTopWidth,horizontal:list.scrollWidth>list.clientWidth,vertical:list.scrollHeight>list.clientHeight};});
      assert.equal(geometry.x,'visible');assert.equal(geometry.y,'visible');assert.equal(geometry.horizontal,false);assert.equal(geometry.stroke,theme==='dopamine'?'2px':'1px');assert.ok(geometry.left>=7&&geometry.top>=7&&geometry.right<=width-7&&geometry.bottom<=height-7);assert.equal(geometry.vertical,height<300);
      await page.getByRole('menuitem',{name:'新建',exact:true}).focus();await page.keyboard.press('ArrowRight');await page.getByRole('menu',{name:'新建',exact:true}).waitFor();
      const sub=await page.getByRole('menu',{name:'新建',exact:true}).boundingBox();assert.ok(sub.x>=7&&sub.x+sub.width<=width-7);
      await page.keyboard.press('Escape');assert.equal(await page.getByRole('menu',{name:'新建',exact:true}).isVisible(),false);assert.equal(await page.getByRole('menu',{name:'文件操作',exact:true}).isVisible(),true);
      const upload=page.getByRole('menuitem',{name:'上传',exact:true});await upload.click();await page.getByRole('menu',{name:'上传',exact:true}).waitFor();
      const parent=await upload.boundingBox(),popup=await page.getByRole('menu',{name:'上传',exact:true}).boundingBox();
      assert.ok(popup.y+popup.height<=parent.y||popup.y>=parent.y+parent.height||popup.x+popup.width<=parent.x||popup.x>=parent.x+parent.width);
      await page.keyboard.press('Escape');
      await page.keyboard.press('Escape');assert.equal(await page.getByRole('menu',{name:'文件操作',exact:true}).isVisible(),false);
    }
    assert.deepEqual(errors,[]);
  }finally{await page.close();}
});
