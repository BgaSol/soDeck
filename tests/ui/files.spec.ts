import {test,expect} from '@playwright/test';
import {presentationFixture,chooseMenu} from './presentation-fixture';
import {setupNetwork} from './network-fixture';

test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const bytes=(text:string)=>({bytesBase64:btoa(unescape(encodeURIComponent(text)))});
    const root=bytes('/');
    const attributes={type:'file',size:'5368709120',mtime:null,atime:null,permissions:33188,uid:1000,gid:1000};
    const rows=Array.from({length:10001},(_,index)=>({id:index?bytes('/文件-'+index+'.dat').bytesBase64:root.bytesBase64,path:index?bytes('/文件-'+index+'.dat'):root,name:index?'文件-'+index+'.dat':'/',depth:index?1:0,type:index?'file':'directory',size:index?'5368709120':null,expanded:index===0,selected:index===0,mount:index===0,readOnly:false,loading:false}));
    rows[1].name='长文件名🙂'.repeat(100);
    rows[2].name='.隐藏文件';
    const f={requests:[] as any[],rows,selected:root.bytesBase64,hidden:true,clipboard:'',revision:1,scrollTop:0,loading:false,details:undefined as any};
    (window as any).__FILES_TEST__=f;
    (window as any).__SODECK_CONTEXT__={viewId:'files-view',viewType:'files.explorer',workspaceId:'server',workspaceGeneration:'g',serverId:'server',serverName:'浏览器文件夹具',pluginVersion:'0.1.0',mountGeneration:1};
    const layout=()=>{let unit=0;return rows.map(r=>{const units=1;const row={...r,units,unitOffset:unit,selected:r.id===f.selected,...(r.mount?{capacity:(r as any).capacity??f.details??{state:'ready',value:{source:'stat',total:'1024000',used:'256000',free:'768000',available:'716800',percent:'25.0%'}}}:{})};unit+=units;return row;});};
    const snapshot=(p:any={})=>{const all=layout(),anchor=all.find(r=>r.id===p.anchorId),viewportUnit=anchor?anchor.unitOffset+(p.anchorOffset??0):p.startUnit??0,start=Math.max(0,viewportUnit-(p.overscanUnits??0)),end=viewportUnit+(p.countUnits??100)+(p.overscanUnits??0),selected=all.find(r=>r.id===f.selected)!;
      const visible=all.filter(r=>r.unitOffset+r.units>start&&r.unitOffset<end).slice(0,12);
      return {revision:f.revision,layoutRevision:f.revision,state:'ready',showHidden:true,selected:f.selected,selectedPath:selected.path,selectedIndex:rows.findIndex(r=>r.id===f.selected),selectedUnit:selected.unitOffset,rows:visible,startUnit:visible[0]?.unitOffset??start,nextUnit:visible.at(-1)?visible.at(-1)!.unitOffset+visible.at(-1)!.units:end,viewportUnit,totalUnits:all.at(-1)!.unitOffset+all.at(-1)!.units,total:rows.length,scrollTop:f.scrollTop,loading:f.loading,path:decodeURIComponent(escape(atob(f.selected))),version:'0.6.0'};};
    (window as any).ipc={postMessage(encoded:string){const request=JSON.parse(encoded);f.requests.push(request);let result:any={accepted:true};
      const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation){queueMicrotask(()=>(window as any).__sodeckReceive(presentation));return;}
      if(request.method==='state.open'&&request.params.contract==='view.presentation.v1') {
        const scope={workspaceId:'server',workspaceGeneration:'g'};
        result={subscriptionId:'presentation',feedEpoch:'epoch',sequence:'0',snapshot:{scope,objectId:'files-view',revision:'1',value:{scope,viewId:'files-view',visible:true,focused:true,revision:1}}};
      }
      if(request.method==='view.initialize')result=snapshot();
      if(request.method==='view.ready')result={ready:true};
      if(request.method==='views.requestFocus')result={viewId:'files-view',scope:{workspaceId:'server',workspaceGeneration:'g'},visible:true,focused:true,revision:1};
      if(request.method==='clipboard.writeText'){f.clipboard=request.params.text;result={written:true};}
      if(request.method==='plugin.invoke'){const p=request.params.args;
        if(request.params.command==='files.snapshot')result=snapshot(p);
        if(request.params.command==='files.row'){const all=layout();result=p.index!==undefined?all[p.index]:all.find(r=>r.unitOffset+r.units>p.unit)??all.at(-1);}
        if(request.params.command==='files.select'){f.selected=p.id;f.revision++;}
        if(request.params.command==='files.locate'){f.loading=true;setTimeout(()=>{f.selected=p.path?.bytesBase64??bytes(p.text).bytesBase64;f.loading=false;f.revision++;},100);}
        if(request.params.command==='files.hidden'){f.hidden=p.value;f.revision++;}
        if(request.params.command==='files.scroll')f.scrollTop=p.top;
      }
      queueMicrotask(()=>(window as any).__sodeckReceive({requestId:request.requestId,result}));
    }};
  });
  await page.setViewportSize({width:320,height:800});
  await page.goto('http://127.0.0.1:1433');
  await expect(page.getByRole('tree')).toBeVisible();
});

test('windowed tree assembles bounded pages, preserves exact sizes, keyboard and context actions',async({page})=>{
  await expect(page.getByRole('treeitem')).toHaveCount(await page.evaluate(()=>Math.min(300,Math.ceil(document.querySelector('.file-tree')!.clientHeight/28)+20)));
  await expect(page.getByRole('region',{name:'节点详情'})).toHaveCount(0);
  await expect(page.getByText('.隐藏文件',{exact:true})).toBeVisible();
  await expect(page.getByRole('meter')).toHaveAttribute('aria-valuenow','25');
  const tree=page.getByRole('tree');await tree.focus();await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('treeitem',{selected:true})).toContainText('长文件名');
  await page.keyboard.press('End');await expect(page.getByRole('treeitem',{selected:true})).toContainText('文件-10000.dat');
  await page.keyboard.press('Shift+F10');await chooseMenu(page,'copy');
  await expect.poll(()=>page.evaluate(()=>(window as any).__FILES_TEST__.clipboard)).toBe('/文件-10000.dat');
  await page.keyboard.press('Home');await expect(page.getByRole('treeitem',{selected:true})).toContainText('/');
  await expect(page.getByRole('button',{name:'显示隐藏文件',exact:true})).toHaveCount(0);
  await tree.focus();await page.keyboard.press('F5');
  await expect.poll(()=>page.evaluate(()=>(window as any).__FILES_TEST__.requests.some((r:any)=>r.params?.command==='files.refresh'))).toBe(true);
  expect(await page.getByRole('treeitem').count()).toBeLessThanOrEqual(300);
  await page.screenshot({path:'.qa/t008/ui/files-keyboard.png'});
});

test('long names, narrow layout, reduced motion and enlarged text keep controls reachable',async({page})=>{
  await page.emulateMedia({reducedMotion:'reduce'});
  await page.setViewportSize({width:260,height:600});
  await page.evaluate(()=>{document.documentElement.style.fontSize='18px';});
  await expect(page.getByRole('button',{name:'定位路径',exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'刷新目录与挂载',exact:true})).toBeVisible();
  const measurements=await page.evaluate(()=>({width:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth,rows:document.querySelectorAll('[role=treeitem]').length,animation:getComputedStyle(document.querySelector('.file-row')!).animationName}));
  expect(measurements.scroll).toBe(measurements.width);expect(measurements.rows).toBeLessThanOrEqual(300);expect(measurements.animation).toBe('none');
  await page.screenshot({path:'.qa/t008/ui/files-large-text.png'});
});

test('live palette leaves file draft and controller state intact; text density drives actual virtual row size',async({page})=>{
 const input=page.getByLabel('远端绝对路径');await input.fill('/未提交/草稿');
 const before=await page.evaluate(()=>(window as any).__FILES_TEST__.requests.filter((r:any)=>r.method==='view.initialize').length);
 await page.evaluate(()=>(window as any).__PRESENTATION__.change('light'));await expect(page.locator('html')).toHaveAttribute('data-theme','soft-light');await expect(input).toHaveValue('/未提交/草稿');await expect(input).toBeFocused();
 expect(await page.evaluate(()=>(window as any).__FILES_TEST__.requests.filter((r:any)=>r.method==='view.initialize').length)).toBe(before);
 await page.evaluate(()=>document.documentElement.style.fontSize='20px');await expect.poll(()=>page.locator('.file-row').first().evaluate(el=>el.getBoundingClientRect().height)).toBeGreaterThan(42);
 const tree=page.getByRole('tree');await tree.focus();await page.keyboard.press('End');await expect(page.getByRole('treeitem',{selected:true})).toContainText('文件-10000.dat');
});

test('delayed location follows the requested path after completion and paints its viewport',async({page})=>{
  const input=page.getByRole('textbox',{name:'远端绝对路径'});await input.fill('/文件-9999.dat');await page.getByRole('button',{name:'定位路径',exact:true}).click();
  await expect(input).toHaveValue('/文件-9999.dat');await expect(page.getByRole('treeitem',{selected:true})).toContainText('文件-9999.dat');
  await expect.poll(()=>page.getByRole('tree').evaluate(el=>el.scrollTop)).toBeGreaterThan(200000);
});

test('new mount recognition above the bottom viewport preserves the visible anchor after layout commits',async({page})=>{
  const tree=page.getByRole('tree');await tree.focus();await page.keyboard.press('End');await expect(page.getByRole('treeitem',{selected:true})).toContainText('文件-10000.dat');
  await tree.evaluate(e=>e.scrollTop=e.scrollHeight);
  const before=await page.getByRole('treeitem',{selected:true}).evaluate(e=>e.getBoundingClientRect().top);
  await page.evaluate(()=>{const f=(window as any).__FILES_TEST__;f.rows[2].mount=true;f.revision++;});
  await expect.poll(()=>page.getByRole('treeitem',{selected:true}).evaluate(e=>e.getBoundingClientRect().top)).toBeCloseTo(before,0);
  await page.waitForTimeout(400);
  expect(await page.getByRole('treeitem',{selected:true}).evaluate(e=>e.getBoundingClientRect().top)).toBeCloseTo(before,0);
  expect(await tree.evaluate(e=>e.scrollHeight-e.scrollTop-e.clientHeight)).toBeLessThan(2);
});

test('partial df capacity stays readable and zero/failed statistics do not hide mounts',async({page})=>{
  await page.setViewportSize({width:260,height:600});
  await page.evaluate(()=>{
    document.documentElement.style.fontSize='18px';
    const f=(window as any).__FILES_TEST__;
    f.details={state:'ready',value:{source:'df',total:'1024000',used:'256000',available:'716800',free:null,percent:'25.0%'}};
    f.revision++;
  });
  const details=page.getByRole('treeitem').first();
  await expect(page.getByText(/不符合工作台契约/)).toHaveCount(0);
  for(const mode of ['light','dark','dopamine']) {
    await page.evaluate(mode=>(window as any).__PRESENTATION__.change(mode),mode);
    await expect(details.getByRole('meter')).toHaveAttribute('aria-valuenow','25');
    await expect(details.getByText('250 KiB / 1000 KiB',{exact:true})).toBeVisible();
    expect(await page.evaluate(()=>document.documentElement.scrollWidth)).toBe(260);
    await page.screenshot({path:`.qa/openwrt-mounts/ui/df-${mode}.png`});
  }
  const height=await details.evaluate(el=>el.getBoundingClientRect().height);
  await page.evaluate(()=>{const f=(window as any).__FILES_TEST__;f.details={state:'ready',value:{source:'df',total:'0',used:'0',free:null,available:null,percent:null}};f.revision++;});
  await expect(details.getByRole('meter')).toHaveCount(0);
  await expect(details.getByText('0 B / 0 B',{exact:true})).toBeVisible();
  await page.evaluate(()=>{const f=(window as any).__FILES_TEST__;f.details={state:'unavailable',message:'服务器未提供容量统计'};f.revision++;});
  await expect(details.getByText('容量不可用',{exact:true})).toBeVisible();
  expect(await details.evaluate(el=>el.getBoundingClientRect().height)).toBe(height);
  await page.evaluate(()=>{const f=(window as any).__FILES_TEST__;f.details={state:'ready',value:{source:'stat',total:'409600',used:'245760',free:'163840',available:'122880',percent:'60.0%'}};f.revision++;});
  await expect(details.getByRole('meter')).toHaveAttribute('aria-valuenow','60');
});

for(const mode of ['dopamine','light','dark'])test(`capacity spans the whole row with shared levels and independent selection: ${mode}`,async({page})=>{
  await page.setViewportSize({width:400,height:540});
  await page.evaluate(mode=>{
    (window as any).__PRESENTATION__.change(mode);
    const f=(window as any).__FILES_TEST__;f.rows.splice(8);
    for(const [i,name,used,total] of [[0,'/',1840,10000],[1,'data',8000,10000],[2,'archive',9400,10000],[3,'empty',0,10000],[4,'proc',0,0]] as const){
      Object.assign(f.rows[i],{name,type:'directory',mount:true,depth:i?2:0,capacity:{state:'ready',value:{used:String(used),total:String(total),source:'stat'}}});
    }
    f.rows[5].name='configuration.json';f.rows[6].name='backup.tar.gz';f.rows[7].name='readme.md';f.revision++;
  },mode);
  const rows=page.getByRole('treeitem');await expect(rows).toHaveCount(8);
  await expect(page.getByRole('meter')).toHaveCount(4);
  await expect(rows.first()).not.toContainText('挂载');
  await expect(rows.nth(1).locator('.sd-capacity-fill')).toHaveAttribute('data-level','warning');
  await expect(rows.nth(2).locator('.sd-capacity-fill')).toHaveAttribute('data-level','critical');
  await expect(rows.nth(1).getByRole('meter')).toHaveAttribute('aria-valuetext','80.0% · 偏高');
  const metrics=await rows.evaluateAll(elements=>elements.map(el=>{
    const row=el.getBoundingClientRect(),fill=el.querySelector('.sd-capacity-fill')?.getBoundingClientRect(),tail=el.querySelector('.mount-capacity,.file-size')!.getBoundingClientRect();
    return {height:row.height,width:row.width,left:row.left,top:row.top,tailRight:tail.right,fillLeft:fill?.left,fillWidth:fill?.width};
  }));
  for(const [i,m] of metrics.entries()){
    expect(m.height).toBeCloseTo(28,1);expect(m.tailRight).toBeCloseTo(metrics[0].tailRight,1);
    if(i)expect(m.top-metrics[i-1].top).toBeCloseTo(28,1);
  }
  for(const [i,percent] of [18.4,80,94,0].entries()){
    expect(metrics[i].fillLeft).toBeCloseTo(metrics[i].left,1);
    expect(metrics[i].fillWidth).toBeCloseTo(metrics[i].width*percent/100,1);
  }
  await rows.nth(1).click();await expect(rows.nth(1)).toHaveAttribute('aria-selected','true');
  expect(await rows.nth(1).evaluate(e=>getComputedStyle(e).outlineStyle)).toBe('solid');
  await expect(rows.nth(1).getByRole('meter')).toHaveAttribute('aria-valuenow','80');
  await page.screenshot({path:`.local/test-results/t018/capacity-${mode}.png`});
});

test('capacity states and enlarged narrow rows keep numbers reachable, with accessible high contrast',async({page})=>{
  await page.setViewportSize({width:260,height:600});
  await page.emulateMedia({reducedMotion:'reduce'});
  await page.evaluate(()=>{document.documentElement.style.fontSize='26px';const f=(window as any).__FILES_TEST__;f.details={state:'loading'};f.revision++;});
  const root=page.getByRole('treeitem').first();
  await expect(root.getByText('读取中',{exact:true})).toBeVisible();
  await expect(root.getByRole('meter')).toHaveCount(0);
  await page.evaluate(()=>{const f=(window as any).__FILES_TEST__;f.details={state:'ready',value:{used:'999000000000',total:'1000000000000',source:'stat'}};f.revision++;});
  await expect(root.getByRole('meter')).toHaveAttribute('aria-valuenow','99.9');
  const geometry=await root.evaluate(el=>{
    const tail=el.querySelector('.mount-capacity')!,name=el.querySelector('.filename')!,row=el.getBoundingClientRect(),t=tail.getBoundingClientRect();
    return {rowHeight:row.height,tailFits:tail.scrollWidth<=tail.clientWidth,nameRight:name.getBoundingClientRect().right,tailLeft:t.left,tailRight:t.right,treeWidth:document.querySelector('.file-tree')!.scrollWidth,outerWidth:document.documentElement.scrollWidth};
  });
  expect(geometry.rowHeight).toBeCloseTo(56,1);expect(geometry.tailFits).toBe(true);expect(geometry.nameRight).toBeLessThanOrEqual(geometry.tailLeft);expect(geometry.outerWidth).toBe(260);
  expect(geometry.tailRight).toBeLessThanOrEqual(geometry.treeWidth);
  const ends=await page.locator('.file-row .mount-capacity,.file-row .file-size').evaluateAll(es=>es.map(e=>e.getBoundingClientRect().right));
  for(const end of ends)expect(end).toBeCloseTo(ends[0],1);
  await page.getByRole('tree').evaluate(el=>el.scrollLeft=el.scrollWidth);
  await page.screenshot({path:'.local/test-results/t018/capacity-large-text.png'});
  await page.emulateMedia({forcedColors:'active'});
  await expect(root.getByRole('meter')).toHaveAttribute('aria-valuetext','99.9% · 紧张');
  expect(await root.evaluate(el=>getComputedStyle(el).outlineStyle)).toBe('solid');
  expect(await root.locator('.sd-capacity-fill').evaluate(el=>getComputedStyle(el).borderInlineEndWidth)).toBe('1px');
  await page.screenshot({path:'.local/test-results/t018/capacity-high-contrast.png'});
});

for(const mode of ['dopamine','light','dark'])test(`network matches files panel surface, selection and footer: ${mode}`,async({page,context})=>{
  await page.setViewportSize({width:360,height:680});
  await page.evaluate(mode=>{(window as any).__PRESENTATION__.change(mode);const f=(window as any).__FILES_TEST__;f.rows.splice(9);f.rows[1].name='workspace';f.rows[1].type='directory';f.rows[2].name='.config';f.selected=f.rows[1].id;f.revision++;},mode);
  const network=await context.newPage();await network.setViewportSize({width:360,height:680});await setupNetwork(network,false);
  await network.evaluate(mode=>{(window as any).__PRESENTATION__.change(mode);const rules=[{id:'proxy',name:'开发环境代理',type:'network-proxy',bindMode:'auto',port:0},{id:'db',name:'数据库连接',type:'port-forward',targetHost:'db.internal',targetPort:5432,bindMode:'fixed',port:15432},{id:'test',name:'测试环境',type:'network-proxy',bindMode:'auto',port:0}];(window as any).__NETWORK_TEST__.items=rules.map((rule,i)=>({rule:{...rule,revision:1,serverId:'server',autoReconnect:false},...(i===0?{run:{rule,status:'ready',address:{host:'127.0.0.1',port:32180},connections:0,bytesRead:0,bytesWritten:0}}:{})}));},mode);
  await network.getByRole('button',{name:'开发环境代理',exact:true}).click();await network.getByRole('button',{name:'返回列表'}).click();
  const metrics=async(p:typeof page,root:string,selected:string)=>p.evaluate(({root,selected})=>{
    const body=getComputedStyle(document.querySelector(root)!);const footer=document.querySelector('.sd-statusbar')!,f=getComputedStyle(footer),row=getComputedStyle(document.querySelector(selected)!);
    return {background:body.backgroundColor,footer:{background:f.backgroundColor,border:f.borderTop,padding:f.padding,font:f.fontSize,height:footer.getBoundingClientRect().height},selection:{background:row.backgroundColor,outline:row.boxShadow,radius:row.borderRadius,weight:row.fontWeight}};
  },{root,selected});
  await expect.poll(async()=>metrics(network,'.network-view','.sd-resource-item[data-selected=true]')).toEqual(await metrics(page,'html','.sd-tree-row[data-selected=true]'));
  expect(await network.locator('.sd-resource-item').first().evaluate(e=>e.getBoundingClientRect().height)).toBeLessThanOrEqual(60);
  await page.screenshot({path:`.local/test-results/t017-r2/files-reference-${mode}.png`});
  await network.screenshot({path:`.local/test-results/t017-r2/network-matched-${mode}.png`});await network.getByRole('button',{name:'开发环境代理',exact:true}).click();await network.screenshot({path:`.local/test-results/t017-r2/detail-matched-${mode}.png`});await network.close();
});
