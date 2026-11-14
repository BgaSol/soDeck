import path from 'node:path';
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
  await page.route('**/__interactions__',route=>route.fulfill({contentType:'text/html',body:`<html><head><style>@layer base,components,themes;</style><link rel="stylesheet" href="/@fs/${path.resolve('packages/ui/shared.css').replaceAll('\\','/')}"><link rel="stylesheet" href="/@fs/${path.resolve('packages/ui/legacy.css').replaceAll('\\','/')}"><link rel="stylesheet" href="/@fs/${path.resolve('packages/ui/themes.css').replaceAll('\\','/')}"></head><body><div id="chrome"></div><div id="tree-host"></div><script type="module">import {icon} from "/ui.ts";import {updateDOM,DomRegion} from "/dom-update.ts";const region=new DomRegion(document.querySelector("#chrome"));window.render={replace:updateDOM,patchChrome:(element,html)=>region.update(element,html),icon};</script></body></html>`}));
  await page.goto(server.resolvedUrls.local[0]+'__interactions__');await page.waitForFunction(()=>window.render);
  return page;
};

const stripFixture=async()=>{
  const page=await fixture();
  await page.evaluate(async()=>{
    await import('/style.css');const {TabStrip,tabControls}=await import('/tab-strip.ts');
    const host=document.querySelector('#chrome');host.innerHTML=`<header class="component-tabs sd-tab-strip" style="width:360px"><div class="tab-navigation"><div class="component-tab-list" role="tablist">${Array.from({length:20},(_,i)=>`<div class="component-tab sd-tab" data-tab-id="${i}"><button role="tab">服务器终端 ${i}</button><button class="sd-icon">×</button></div>`).join('')}</div>${tabControls('本组页面')}</div><button id="create">＋</button></header>`;
    window.strip=new TabStrip(host.querySelector('.component-tab-list'),host.querySelector('.tab-controls'));strip.update('0');
    window.positions=()=>({left:strip.viewport.scrollLeft,max:strip.viewport.scrollWidth-strip.viewport.clientWidth});
  });
  return page;
};
test('overflow controls stay fixed, animation merges input and direct pixels interrupt it',async()=>{
  const page=await stripFixture();
  try{
    const forward=page.locator('[data-tab-action="forward"]');await forward.waitFor();
    assert.equal(await page.locator('[data-tab-action="back"]').isDisabled(),true);
    const fixed=await page.locator('#create').boundingBox();
    await page.evaluate(()=>{strip.scroll(240);window.first=positions().left;});
    assert.equal(await page.evaluate(()=>first),0);
    await page.waitForTimeout(45);const middle=await page.evaluate(()=>positions().left);assert(middle>0&&middle<240);
    await page.evaluate(()=>{strip.scroll(240);strip.update('0');});await page.waitForTimeout(320);
    assert(Math.abs(await page.evaluate(()=>positions().left)-480)<=1,'Refresh or repeated input reset the target');
    assert.deepEqual(await page.locator('#create').boundingBox(),fixed);
    await page.evaluate(()=>{strip.scroll(500);strip.scroll(-100);});await page.waitForTimeout(320);
    assert(Math.abs(await page.evaluate(()=>positions().left)-380)<=1,'Reverse must start from the visible position');
    await page.evaluate(()=>{strip.scroll(500);window.takeover=positions().left;strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:7.5,deltaMode:0,cancelable:true}));});
    await page.waitForTimeout(220);assert(Math.abs(await page.evaluate(()=>positions().left-takeover)-7.5)<=1);
    const zoom=await page.evaluate(()=>!strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:200,ctrlKey:true,cancelable:true})));assert.equal(zoom,false);
    await page.evaluate(()=>{strip.scroll(500);strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaX:5,cancelable:true}));window.native=positions().left;});await page.waitForTimeout(200);assert.equal(await page.evaluate(()=>positions().left),await page.evaluate(()=>native));
    await page.evaluate(()=>{strip.scroll(200);strip.dispose();window.stopped=positions().left;});await page.waitForTimeout(100);assert.equal(await page.evaluate(()=>positions().left),await page.evaluate(()=>stopped));
  }finally{await page.close();}
});
test('reveal, resize, manual browsing, edge dwell and reduced motion preserve selection',async()=>{
  const page=await stripFixture();
  try{
    await page.evaluate(()=>strip.update('19'));await page.waitForTimeout(350);
    assert(Math.abs((await page.evaluate(()=>positions())).left-(await page.evaluate(()=>positions())).max)<=1);
    await page.evaluate(()=>{strip.scroll(-900,false);strip.update('19');window.browsed=positions().left;});
    await page.evaluate(()=>{document.querySelector('header').style.width='280px';});await page.waitForTimeout(100);
    assert.equal(await page.evaluate(()=>positions().left),await page.evaluate(()=>browsed));
    await page.evaluate(()=>{strip.reveal('19',false);});await page.waitForTimeout(30);
    await page.evaluate(()=>{document.querySelector('header').style.width='230px';});await page.waitForTimeout(100);
    assert.equal(await page.evaluate(()=>{const r=strip.tab('19').getBoundingClientRect(),v=strip.viewport.getBoundingClientRect();return r.left>=v.left-1&&r.right<=v.right+1;}),true,'Previously visible active tab remains visible after shrinking');
    await page.evaluate(()=>{strip.scroll(-10000,false);const r=strip.viewport.getBoundingClientRect();strip.drag({x:r.right-2,y:r.top+12});});
    await page.waitForTimeout(60);assert.equal(await page.evaluate(()=>positions().left),0);
    await page.waitForTimeout(250);assert((await page.evaluate(()=>positions().left))>20);
    await page.evaluate(()=>{strip.drag();window.end=positions().left;});await page.waitForTimeout(100);assert.equal(await page.evaluate(()=>positions().left),await page.evaluate(()=>end));
    await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>{strip.scroll(10000);});assert.equal((await page.evaluate(()=>positions())).left,(await page.evaluate(()=>positions())).max);
    await page.evaluate(()=>{strip.viewport.replaceChildren();strip.update(null);});assert.equal(await page.locator('.tab-arrows').isVisible(),false);assert.equal(await page.locator('[data-tab-action="pick"]').isVisible(),false);
  }finally{await page.close();}
});
test('picker searches resource identity and preserves composition, focus and cancelled close',async()=>{
  const page=await stripFixture();
  try{
    await page.evaluate(async()=>{
      const {TabPicker}=await import('/tab-picker.ts');window.rows=[{id:'one',title:'config.yaml',detail:'/服务一/config.yaml',current:true,status:'未保存',closeLabel:'关闭'},{id:'two',title:'config.yaml',detail:'/服务二/config.yaml',closeLabel:'关闭'}];window.chosen=[];window.closeCalls=[];
      window.picker=new TabPicker(document.querySelector('[data-tab-action="pick"]'),'本组页面',()=>rows,async id=>chosen.push(id),async id=>closeCalls.push(id),()=>true,()=>{});
    });
    const input=page.locator('.tab-picker input');await input.fill('服务二');assert.equal(await page.locator('.tab-picker-row').count(),1);
    await page.evaluate(()=>{const input=document.querySelector('.tab-picker input');window.original=input;input.dispatchEvent(new CompositionEvent('compositionstart'));input.setSelectionRange(1,2);rows[1].status='错误';picker.update();});
    assert.equal(await page.evaluate(()=>document.activeElement===original&&original.selectionStart===1&&original.selectionEnd===2),true);
    await page.locator('[data-tab-picker-action="close"]').click();assert.deepEqual(await page.evaluate(()=>closeCalls),['two']);assert.equal(await page.locator('.tab-picker-row').count(),1,'Cancelled close keeps the live record');
    await input.fill('absent');assert.equal(await page.getByRole('status').textContent(),'没有匹配的标签');
    await input.fill('服务二');await input.press('ArrowDown');await page.keyboard.press('Enter');assert.deepEqual(await page.evaluate(()=>chosen),['two']);assert.equal(await page.locator('.tab-picker').count(),0);
  }finally{await page.close();}
});
test('line and page wheels normalize distance, single long labels fit without overflow controls',async()=>{
  const page=await stripFixture();
  try{
    await page.evaluate(()=>{strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:120,cancelable:true}));strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:120,cancelable:true}));window.wheelStart=positions().left;});
    assert.equal(await page.evaluate(()=>wheelStart),0,'Fast discrete wheel input still animates');await page.waitForTimeout(320);
    assert(Math.abs(await page.evaluate(()=>positions().left)-240)<=1);await page.evaluate(()=>strip.scroll(-10000,false));
    await page.evaluate(()=>strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:3,deltaMode:1,cancelable:true})));await page.waitForTimeout(320);
    assert(Math.abs(await page.evaluate(()=>positions().left)-96)<=1);
    const width=await page.evaluate(()=>strip.viewport.clientWidth);
    await page.evaluate(()=>strip.viewport.dispatchEvent(new WheelEvent('wheel',{deltaY:1,deltaMode:2,cancelable:true})));await page.waitForTimeout(320);
    assert(Math.abs(await page.evaluate(()=>positions().left)-96-width)<=1);
    await page.evaluate(()=>{strip.viewport.innerHTML='<div class="component-tab sd-tab" data-tab-id="long"><button role="tab"><span>'+('很长的文件名称'.repeat(20))+'</span></button><button class="sd-icon" aria-label="关闭">'+render.icon('close')+'</button></div>';strip.update('long');});
    await page.waitForTimeout(150);assert.equal(await page.locator('.tab-arrows').isVisible(),false);
    assert.equal(await page.locator('.component-tab').evaluate(n=>{const r=n.parentElement.getBoundingClientRect(),close=n.lastElementChild.getBoundingClientRect();return close.left>=r.left&&close.right<=r.right+1;}),true);
  }finally{await page.close();}
});
test('settling a drop stops edge scrolling, reports its token and rejects late previews',async()=>{
  const page=await stripFixture();
  try{
    await page.evaluate(async()=>{
      const {Workspaces}=await import('/workspaces.ts');window.dropHost=Object.create(Workspaces.prototype);window.reports=[];
      Object.assign(dropHost,{tabs:strip.viewport,strip,state:{window:{id:'main'}},retiredPreviews:new Set(),components:{previewDrop:()=>{},dropZones:()=>[],dockZones:()=>[]},call:async(command,args)=>reports.push({command,args})});
      const r=strip.viewport.getBoundingClientRect();window.preview={kind:'strip',windowId:'main',token:'drag-one',index:0,point:{x:r.right-2,y:r.top+10}};dropHost.previewDrop(preview);
    });
    await page.waitForTimeout(250);assert((await page.evaluate(()=>positions().left))>0);
    await page.evaluate(async()=>{await dropHost.settleDrop('drag-one');window.finished=positions().left;dropHost.previewDrop(preview);});
    await page.waitForTimeout(250);assert.equal(await page.evaluate(()=>positions().left),await page.evaluate(()=>finished));
    assert.equal(await page.evaluate(()=>reports.at(-1).args.dragToken),'drag-one');
    await page.evaluate(()=>{
      const gate=new Promise(resolve=>window.savedGate=resolve);window.flushing=false;
      Object.assign(dropHost,{moving:new Set(),finishResize:()=>{},flushLayouts:()=>{window.flushing=true;return gate;},clearGesture:()=>{},refresh:async()=>{},notify:()=>{}});dropHost.components.flush=async()=>{};
      window.finishPromise=dropHost.completeGesture({id:'server',token:'drag-two',cancelled:false});
    });
    await page.waitForFunction(()=>flushing);
    assert.equal(await page.evaluate(()=>reports.at(-1).command),'workspace_drag_release','Remote scrolling stops before waiting for a save');
    await page.evaluate(async()=>{savedGate();await finishPromise;});
    assert.equal(await page.evaluate(()=>reports.at(-1).command),'workspace_drag_finish');
  }finally{await page.close();}
});

test('form refresh preserves supported selections and direction without transferring values or outside focus',async()=>{
  const page=await fixture();
  try{
    const results=await page.evaluate(()=>{
      const host=document.querySelector('#chrome'),cases=[];
      const markup=(kind,value='abcdef',attrs='')=>kind==='textarea'?`<textarea id="field" ${attrs}>${value}</textarea>`:`<input id="field" type="${kind}" value="${value}" ${attrs}>`;
      for(const kind of ['text','search','password','tel','url','textarea'])for(const direction of ['forward','backward','none']){
        host.innerHTML=markup(kind)+'<span>before</span>';
        const old=host.querySelector('#field');old.focus();old.setSelectionRange(1,4,direction);
        const expected=[old.selectionStart,old.selectionEnd,old.selectionDirection];
        render.replace(host,markup(kind)+'<span>after</span>');
        const current=host.querySelector('#field');
        cases.push({kind,direction,expected,focused:document.activeElement===current,selection:[current.selectionStart,current.selectionEnd,current.selectionDirection],value:current.value});
      }
      host.innerHTML=markup('password');host.firstElementChild.focus();host.firstElementChild.setSelectionRange(1,4,'backward');
      render.replace(host,markup('password','x'));
      const shortened={value:host.firstElementChild.value,selection:[host.firstElementChild.selectionStart,host.firstElementChild.selectionEnd]};
      const outside=document.createElement('input');outside.id='field';outside.value='outside';document.body.prepend(outside);
      host.innerHTML=markup('text');host.firstElementChild.focus();host.firstElementChild.setSelectionRange(1,4);
      render.replace(host,markup('text')+'<span>scoped target</span>');
      const scoped=document.activeElement===host.firstElementChild;
      outside.focus();outside.setSelectionRange(2,5,'backward');render.replace(host,markup('text','new entity'));
      const outsideKept=document.activeElement===outside&&outside.selectionStart===2&&outside.selectionEnd===5&&outside.selectionDirection==='backward';
      outside.remove();
      const changes=[];
      for(const next of [markup('number','12345'),markup('checkbox'),'<select id="field"><option>one</option></select>',markup('password','abcdef','disabled'),'<span>removed</span>','<input value="new entity">']){
        host.innerHTML=markup('password');host.firstElementChild.focus();host.firstElementChild.setSelectionRange(1,4);
        render.replace(host,next);changes.push({html:next,focused:document.activeElement===host.firstElementChild,value:host.firstElementChild.value});
      }
      host.innerHTML='<input value="abcdef">';host.firstElementChild.focus();host.firstElementChild.setSelectionRange(1,4);
      render.replace(host,'<input value="different">');
      return {cases,shortened,scoped,outsideKept,changes,noIdFocused:document.activeElement===host.firstElementChild,noIdValue:host.firstElementChild.value};
    });
    for(const row of results.cases){assert.equal(row.focused,true,row.kind);assert.deepEqual(row.selection,row.expected,row.kind);assert.equal(row.value,'abcdef');}
    assert.deepEqual(results.shortened,{value:'x',selection:[1,1]});
    assert.equal(results.scoped,true);assert.equal(results.outsideKept,true);
    assert.deepEqual(results.changes.map(row=>row.focused),[false,false,false,true,false,false]);
    assert.equal(results.noIdFocused,true);assert.equal(results.noIdValue,'different');
  }finally{await page.close();}
});

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
      Object.assign(workspaceMenu,{moving:new Set(),tabs:document.querySelector('#chrome'),state:{window:{id:'main'},workspaces:[{context:{workspaceId:'one'}}]}});
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

test('nested entity keys keep controls scoped and disposed regions cannot repaint',async()=>{
  const page=await fixture();
  try{
    const result=await page.evaluate(async()=>{
      const {updateDOM,DomRegion}=await import('/dom-update.ts');const host=document.querySelector('#tree-host');
      const group=id=>`<section data-group="${id}"><button data-component-action="create">${id}</button></section>`;
      updateDOM(host,group('a')+group('b'));const a=host.querySelector('[data-group=a]'),b=host.querySelector('[data-group=b]'),buttons=[a.firstChild,b.firstChild];
      updateDOM(host,`<div data-split="outer">${group('b')}<div data-split="inner">${group('a')}</div></div>`);
      const retained=[a,b].every(node=>node===host.querySelector(`[data-group="${node.dataset.group}"]`))&&a.firstChild===buttons[0]&&b.firstChild===buttons[1];
      const signals=[];const original=document.addEventListener.bind(document);document.addEventListener=(name,listener,options)=>{if(options?.signal)signals.push(options.signal);return original(name,listener,options);};
      const nodes=[];for(let i=0;i<20;i++){
        const root=document.createElement('div');root.innerHTML='<button>before</button>';document.body.append(root);nodes.push(root);
        const region=new DomRegion(root);root.firstChild.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,button:0,pointerId:1}));region.update(root,'<button>late</button>');document.dispatchEvent(new PointerEvent('pointerup',{pointerId:1}));region.dispose();root.remove();
      }
      document.addEventListener=original;await new Promise(resolve=>setTimeout(resolve,20));
      return {retained,disposed:signals.length===60&&signals.every(signal=>signal.aborted),unchanged:nodes.every(node=>node.textContent==='before')};
    });
    assert.deepEqual(result,{retained:true,disposed:true,unchanged:true});
  }finally{await page.close();}
});

test('host draft close waits for final composition and reports only changed values',async()=>{
  const page=await fixture();
  try{
    const result=await page.evaluate(async()=>{
      const {HostDrafts}=await import('/host-drafts.ts'),{core}=await import('/core.ts');
      const snapshot={preferences:{updateNetwork:{revision:0,fallbackEnabled:true,disabledBuiltins:[],customMirrors:[]}},plugins:[{manifest:{id:'one',name:'One',updateSource:{repository:'owner/repo',tagPrefix:'v'}}}]};
      const state={busy:new Set(),sourceDrafts:{}};const drafts=new HostDrafts(()=>snapshot,state,()=>{});
      const input=document.createElement('input');document.body.append(input);input.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));let flushed=false;
      const pending=drafts.flushInputs().then(()=>flushed=true);await Promise.resolve();const waited=!flushed;
      input.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true}));drafts.input('one','repository','owner/edited');await pending;
      const collected=await drafts.closeInputs('collect');let writes=0;
      core.updateSource=async()=>{writes++;throw new Error('save failed');};
      let failed=false;try{await drafts.closeInputs('save');}catch{failed=true;}
      const retained=state.sourceDrafts.one.repository==='owner/edited';
      await drafts.closeInputs('discard');const clean=(await drafts.closeInputs('collect')).length===0;
      drafts.input('one','repository','owner/repo');const unchanged=(await drafts.closeInputs('collect')).length===0;
      drafts.dispose();return {waited,flushed,count:collected.length,failed,retained,writes,clean,unchanged};
    });
    assert.deepEqual(result,{waited:true,flushed:true,count:1,failed:true,retained:true,writes:1,clean:true,unchanged:true});
  }finally{await page.close();}
});

test('update progress preserves wheel scrolling and expanded notes',async()=>{
  const page=await fixture();
  try{
    await page.setViewportSize({width:1100,height:720});
    await page.evaluate(async()=>{
      await import('/style.css');
      const {settings}=await import('/views.ts');const {updateDOM:renderUpdatesSettings}=await import('/dom-update.ts');
      const host=document.querySelector('#chrome');host.className='settings-content';host.style.cssText='width:940px;height:560px';
      window.updateSnapshot={version:'0.1.0',target:'x86_64-pc-windows-msvc',preferences:{},plugins:Array.from({length:12},(_,i)=>({manifest:{id:'test.'+i,name:'插件 '+i,version:'0.1.0',target:'x86_64-pc-windows-msvc',updateSource:{repository:'test/plugins'}},enabled:true}))};
      window.updateView={section:'updates',updates:Object.fromEntries(updateSnapshot.plugins.map(p=>[p.manifest.id,{id:p.manifest.id,phase:'downloading',version:'0.2.0',notes:'# 更新说明\n\n'+Array.from({length:20},(_,i)=>'- 改进 '+i).join('\n'),downloadedBytes:1048576,totalBytes:104857600,error:null}])),excludedUpdates:new Set(),busy:new Set(),updateOverview:{busy:true,checking:false}};
      window.refreshUpdates=()=>renderUpdatesSettings(host,settings(updateSnapshot,updateView));refreshUpdates();
    });
    const scroller=page.locator('.settings-page');
    await scroller.evaluate(node=>node.scrollTop=350);
    await scroller.hover();await page.mouse.wheel(0,300);
    await page.evaluate(()=>{updateView.updates['test.0'].downloadedBytes+=1048576;refreshUpdates();});
    await page.waitForTimeout(150);
    assert((await scroller.evaluate(node=>node.scrollTop))>=649,'Download progress interrupted a wheel scroll');
    await scroller.locator('.update-release-notes summary').first().click();
    await scroller.evaluate(node=>node.scrollTop=600);
    await page.evaluate(()=>{updateView.updates['test.0'].downloadedBytes+=1048576;refreshUpdates();});
    assert.equal(await scroller.evaluate(node=>node.scrollTop),600);
    assert.equal(await scroller.locator('.update-release-notes').first().evaluate(node=>node.open),true);
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
    assert.equal(await page.getByRole('menuitem',{name:'重命名（当前后端不支持）',exact:true}).isDisabled(),true);
    assert.equal(await page.getByRole('menuitem',{name:'剪切（当前后端不支持）',exact:true}).isDisabled(),true);
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
    assert.equal(await page.getByRole('menuitem',{name:'重命名（当前后端不支持）',exact:true}).count(),0);assert.equal(await page.getByRole('menuitem',{name:'复制',exact:true}).count(),1);
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
    await page.evaluate(()=>{const anchor=document.createElement('button');document.body.append(anchor);window.workspace=Object.create(Workspaces.prototype);Object.assign(workspace,{moving:new Set(),state:{window:{id:'main',activeWorkspace:'one'},workspaces:[{context:{workspaceId:'one'}},{context:{workspaceId:'two'}}]}});workspace.openMenu('one',anchor);workspace.state.window.activeWorkspace='two';workspace.menuPosition.update();});
    await page.locator('.workspace-menu').waitFor({state:'detached'});
  }finally{await page.close();}
});
