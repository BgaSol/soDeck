// Real browser input and shipped file-menu code; no remote or native clipboard writes.
import assert from 'node:assert/strict';
import {before,after,test} from 'node:test';
import fs from 'node:fs';
import {packageMenuAssets} from './plugin-menu-assets.mjs';
import {createServer} from 'vite';
import {chromium} from 'playwright';

let server,browser,menuAsset;
before(async()=>{await packageMenuAssets('artifacts/menu-position/test-runtime');menuAsset=fs.readFileSync('artifacts/menu-position/test-runtime/ui/text-menu.js','utf8');server=await createServer({server:{port:0,strictPort:false}});await server.listen();browser=await chromium.launch({headless:true});});
after(async()=>{await browser?.close();await server?.close();});
const fixture=async()=>{
  const page=await browser.newPage({viewport:{width:400,height:540}});
  await page.route('**/__interactions__',route=>route.fulfill({contentType:'text/html',body:'<html><head><style>@layer base,components,themes;</style><link rel="stylesheet" href="/ui.css"><link rel="stylesheet" href="/themes.css"></head><body><div id="chrome"></div><div id="tree-host"></div><script type="module">import {replace,patchChrome,icon} from "/ui.ts";window.render={replace,patchChrome,icon};</script></body></html>'}));
  await page.goto(server.resolvedUrls.local[0]+'__interactions__');await page.waitForFunction(()=>window.render);
  return page;
};

test('nested layout reserves usable group dimensions and clamps without changing saved proportions',async()=>{
  const page=await fixture();
  try {
    const result=await page.evaluate(async()=>{
      const {centerMinimum,effectiveRatio,ratioBounds,withSplit}=await import('/center-layout.ts');
      const tree={type:'split',id:'root',axis:'x',ratio:.1,first:{type:'group',groupId:'a'},second:{type:'split',id:'lower',axis:'y',ratio:.5,first:{type:'group',groupId:'b'},second:{type:'group',groupId:'c'}}};
      return {minimum:centerMinimum(tree),bounds:ratioBounds(tree,1006,800),effective:effectiveRatio(tree,1006,800),saved:tree.ratio,extra:centerMinimum(withSplit(tree,'a','x'))};
    });
    assert.deepEqual(result,{minimum:{width:566,height:366},bounds:{min:.28,max:.72},effective:.28,saved:.1,extra:{width:852,height:366}});
  } finally {await page.close();}
});

test('workspace menu follows a right-edge anchor when the open window shrinks',async()=>{
  const page=await fixture();
  try{
    await page.setViewportSize({width:1100,height:720});
    await page.evaluate(async()=>{
      const {Workspaces}=await import('/workspaces.ts');
      const anchor=document.createElement('button');anchor.textContent='菜单';anchor.style.cssText='position:fixed;right:12px;top:56px;width:32px;height:32px';document.body.append(anchor);
      window.workspaceMenu=Object.create(Workspaces.prototype);
      Object.assign(workspaceMenu,{tabs:document.querySelector('#chrome'),state:{window:{id:'main'},workspaces:[{context:{workspaceId:'one'}}]}});
      workspaceMenu.openMenu('one',anchor);
    });
    await page.locator('.workspace-menu').waitFor();
    await page.setViewportSize({width:720,height:480});
    await page.waitForFunction(()=>{const menu=document.querySelector('.workspace-menu'),r=menu?.getBoundingClientRect();return r&&r.left>=7&&r.right<=innerWidth-7&&r.bottom<=innerHeight-7;},null,{timeout:3000});
  }finally{await page.close();}
});
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

const fileFixture=async(inFrame=false)=>{
  const page=await fixture();
  let surface=page;
  if(inFrame){await page.setViewportSize({width:900,height:700});await page.evaluate(()=>{document.body.innerHTML='<iframe id="files" src="/__interactions__" style="position:fixed;left:40px;top:30px;width:600px;height:540px;border:0"></iframe>';});surface=await page.frameLocator('#files').owner().elementHandle().then(handle=>handle.contentFrame());await surface.waitForFunction(()=>window.render);page.fileSurface=surface;}
  await surface.addStyleTag({content:fs.readFileSync('plugins/files/ui/style.css','utf8')});
  await surface.addScriptTag({content:menuAsset});
  await surface.addScriptTag({content:fs.readFileSync('plugins/files/ui/tree.js','utf8')});
  await surface.addScriptTag({content:fs.readFileSync('plugins/files/ui/transfer.js','utf8')});
  const taskGroups=await surface.evaluate(async()=>{
    const host=document.createElement('div');document.body.append(host);
    const tasks=['completed','failed','partial','needsReview','awaitingConflict','running'].map((state,index)=>({id:String(index),state,kind:'upload',completed:1,failed:state==='failed'?1:0,skipped:0,bytes:0}));
    const panel=new FileTransfers({host,event:async()=>({tasks}),refresh:async()=>{},message:()=>{}});await panel.poll();
    const first={attention:panel.groups.attention.list.childElementCount,active:panel.groups.active.list.childElementCount,completed:panel.groups.completed.list.childElementCount,collapsed:!panel.groups.completed.group.open};
    panel.groups.completed.group.open=true;await panel.poll();first.expanded=panel.groups.completed.group.open;
    panel.dispose();host.remove();return first;
  });
  assert.deepEqual(taskGroups,{attention:4,active:1,completed:1,collapsed:true,expanded:true},'Only successful tasks collapse; review/failure/conflict stay visible');
  await surface.evaluate(()=>{
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
  if(inFrame)await surface.addStyleTag({content:'html,body{height:100%;overflow:hidden}#tree-host{height:100%!important}'});
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
      await page.getByRole('menu',{name:'文件操作',exact:true}).waitFor();
      const geometry=await page.locator('.tree-menu').evaluate(menu=>{const r=menu.getBoundingClientRect(),s=getComputedStyle(menu),list=menu.querySelector('.tree-menu-list');return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,x:s.overflowX,y:s.overflowY,stroke:getComputedStyle(menu,'::before').borderTopWidth,horizontal:list.scrollWidth>list.clientWidth,vertical:list.scrollHeight>list.clientHeight};});
      assert.equal(geometry.x,'visible');assert.equal(geometry.y,'visible');assert.equal(geometry.horizontal,false);assert.equal(geometry.stroke,theme==='dopamine'?'2px':'1px');assert.ok(geometry.left>=7&&geometry.top>=7&&geometry.right<=width-7&&geometry.bottom<=height-7);assert.equal(geometry.vertical,height<300);
      await page.getByRole('menuitem',{name:'新建',exact:true}).focus();await page.keyboard.press('ArrowRight');
      await page.getByRole('menu',{name:'新建',exact:true}).waitFor({timeout:3000}).catch(async error=>{console.error({theme,width,height,errors,state:await page.evaluate(()=>({active:document.activeElement.outerHTML,menu:tree.menu?.outerHTML}))});throw error;});
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

test('file iframe submenus flip in four directions and allow real mouse crossing and actions',async()=>{
  const page=await fileFixture(true),frame=page.fileSurface,placements=new Set();
  try{
    for(const [width,height,x,y,desiredTop]of [[600,540,12,30,null],[600,540,580,30,null],[320,200,150,5,20],[320,200,150,180,100],[180,160,90,5,10]]){
      await page.locator('#files').evaluate((node,{width,height})=>Object.assign(node.style,{width:width+'px',height:height+'px'}),{width,height});
      await page.mouse.move(850,650);
      await frame.evaluate(({x,y})=>tree.showMenu(tree.nodes.get('/home/dw'),x,y),{x,y});
      await frame.getByRole('menu',{name:'文件操作',exact:true}).waitFor();
      const trigger=frame.getByRole('menuitem',{name:'新建',exact:true});
      if(desiredTop!==null)await trigger.evaluate((node,top)=>{const list=node.parentElement;list.scrollTop=node.offsetTop-top;},desiredTop);
      await trigger.hover();
      const sub=frame.getByRole('menu',{name:'新建',exact:true});await sub.waitFor();
      placements.add((await sub.getAttribute('data-placement')).split('-')[0]);
      const parent=await trigger.boundingBox(),popup=await sub.boundingBox(),item=await sub.getByRole('menuitem',{name:'文件…',exact:true}).boundingBox();
      assert.ok(popup.x>=47&&popup.y>=37&&popup.x+popup.width<=40+width-7&&popup.y+popup.height<=30+height-7,JSON.stringify({width,height,popup}));
      const horizontal=popup.x>=parent.x+parent.width||popup.x+popup.width<=parent.x;
      const start=horizontal?{x:popup.x>parent.x?parent.x+parent.width-1:parent.x+1,y:Math.max(parent.y+1,Math.min(parent.y+parent.height-1,item.y+item.height/2))}:{x:Math.max(parent.x+1,Math.min(parent.x+parent.width-1,item.x+item.width/2)),y:popup.y>parent.y?parent.y+parent.height-1:parent.y+1};
      await page.mouse.move(start.x,start.y);
      await page.mouse.move(item.x+item.width/2,item.y+item.height/2,{steps:12});
      await page.waitForTimeout(220);assert.equal(await sub.isVisible(),true,'Crossing the gap keeps the submenu open');
      await page.mouse.click(item.x+item.width/2,item.y+item.height/2);
      await frame.waitForFunction(()=>operations.some(op=>op[0]==='createEmpty'));
      assert.equal(await frame.evaluate(()=>operations.at(-1)[2]),'/home/dw');
      await frame.evaluate(()=>operations=[]);
    }
    assert.deepEqual([...placements].sort(),['bottom','left','right','top']);
  }finally{await page.close();}
});

test('element menus follow scrolling, pointer menus close on source scrolling, and disposed measurements stay inert',async()=>{
  const page=await fixture();
  try{
    await page.evaluate(async()=>{
      const {attachMenuPosition}=await import('/menu-position.js');window.attach=attachMenuPosition;
      const scroller=document.createElement('div');scroller.id='scroller';scroller.style.cssText='height:180px;overflow:auto;position:fixed;left:20px;top:20px;width:250px';scroller.innerHTML='<div style="height:500px"><button id="anchor" style="margin-top:70px">anchor</button></div>';document.body.append(scroller);
      window.openPosition=(point=false)=>{window.position?.dispose();document.querySelector('#position-menu')?.remove();const menu=document.createElement('div');menu.id='position-menu';menu.className='sd-menu';menu.innerHTML='<div class="sd-menu-list"><button>action</button></div>';menu.style.width='180px';document.body.append(menu);const anchor=document.querySelector('#anchor');window.position=attach(menu,point?{x:130,y:120,contextElement:anchor}:anchor,{onInvalid:()=>menu.remove()});};openPosition();
    });
    await page.locator('#position-menu').waitFor();
    const before=await page.locator('#position-menu').boundingBox();
    await page.locator('#scroller').evaluate(node=>node.scrollTop=20);
    await page.waitForFunction(top=>Math.abs(document.querySelector('#position-menu').getBoundingClientRect().top-top+20)<1,before.y);
    await page.evaluate(()=>openPosition(true));await page.locator('#position-menu').waitFor();
    await page.locator('#scroller').evaluate(node=>node.scrollTop=30);await page.locator('#position-menu').waitFor({state:'detached'});
    await page.evaluate(()=>{openPosition();document.querySelector('#anchor').style.visibility='hidden';});await page.locator('#position-menu').waitFor({state:'detached'});
    assert.equal(await page.evaluate(async()=>{document.querySelector('#anchor').style.visibility='visible';openPosition();const menu=document.querySelector('#position-menu');position.update();position.dispose();const before=menu.getAttribute('style');await new Promise(resolve=>setTimeout(resolve,50));return before===menu.getAttribute('style')&&await position.ready===false;}),true);
    await page.evaluate(()=>{openPosition();document.querySelector('#anchor').remove();});await page.locator('#position-menu').waitFor({state:'detached'});
  }finally{await page.close();}
});

test('component filtering resizes one menu and workspace context replacement closes its original target',async()=>{
  const page=await fixture();
  try{
    await page.addStyleTag({content:fs.readFileSync('src/shell.css','utf8')});
    await page.addStyleTag({content:fs.readFileSync('src/components.css','utf8')});
    await page.evaluate(async()=>{
      const {Components}=await import('/components.ts'),{Workspaces}=await import('/workspaces.ts');
      const anchor=document.createElement('button');anchor.id='anchor';anchor.style.cssText='position:fixed;right:12px;bottom:12px';anchor.textContent='菜单';document.body.append(anchor);
      window.component=Object.create(Components.prototype);component.content=document.querySelector('#chrome');component.showMenu(anchor,Array.from({length:30},(_,i)=>({label:'组件 '+i+' very long label '.repeat(8),action:async()=>{}})),true);
      window.Workspaces=Workspaces;
    });
    const menu=page.locator('.component-menu');await menu.waitFor();const height=(await menu.boundingBox()).height;
    await menu.locator('input').fill('组件 29');await page.waitForFunction(()=>document.querySelector('.component-menu').offsetHeight<150);
    assert.ok((await menu.boundingBox()).height<height);assert.equal(await menu.locator('.component-menu-row:visible').count(),1);
    await page.setViewportSize({width:180,height:160});
    await page.waitForFunction(()=>{const r=document.querySelector('.component-menu').getBoundingClientRect();return r.left>=7&&r.top>=7&&r.right<=innerWidth-7&&r.bottom<=innerHeight-7;});
    await page.evaluate(()=>document.querySelector('#anchor').remove());await menu.waitFor({state:'detached'});
    await page.evaluate(()=>{const anchor=document.createElement('button');document.body.append(anchor);window.workspace=Object.create(Workspaces.prototype);Object.assign(workspace,{state:{window:{id:'main',activeWorkspace:'one'},workspaces:[{context:{workspaceId:'one'}},{context:{workspaceId:'two'}}]}});workspace.openMenu('one',anchor);workspace.state.window.activeWorkspace='two';workspace.menuPosition.update();});
    await page.locator('.workspace-menu').waitFor({state:'detached'});
  }finally{await page.close();}
});
