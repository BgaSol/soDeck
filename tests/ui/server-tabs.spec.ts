import { presentationFixture, chooseMenu } from "./presentation-fixture";
import {test,expect} from '@playwright/test';
import {readFileSync} from 'node:fs';

test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.setViewportSize({width:800,height:600});
  await page.addInitScript(()=>{
    const servers=Array.from({length:22},(_,i)=>({id:`server-${i}`,name:`服务器 ${i} · 很长的显示名称`,host:`host-${i}.test`,port:22,username:`user-${i}`,authType:'password'}));
    const workspaces=servers.slice(0,21).map((s,i)=>({workspaceId:s.id,serverId:s.id,ownerWindowId:i===20?'workspace-other':'main'}));
    const layout={schemaVersion:3,windowId:'main',windowRole:'main',workspaceIds:servers.slice(0,20).map(s=>s.id),activeWorkspaceId:'server-0',workspaces,commands:[],viewDeclarations:[],dockSlots:[],views:[],revision:1,split:'none',splitRatio:.5,pendingOpens:[],recoveryWarnings:[]};
    const fixture={calls:[] as any[],layout};(window as any).__TABS_TEST__=fixture;
    (window as any).__TAURI_INTERNALS__={invoke:async(_:string,{request}:any)=>{
      fixture.calls.push(request);const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation)return presentation;let result:any=null;
      if(request.method==='servers.list')result={servers};
      if(request.method==='workbench.snapshot')result=JSON.parse(JSON.stringify(layout));
      if(request.method==='workspaces.activate'){layout.activeWorkspaceId=request.params.workspaceId;result={workspaceId:request.params.workspaceId,ownerWindowId:'main',committed:true,persisted:true};}
      if(request.method==='workspaces.open')result={workspaceId:request.params.serverId,created:false,ownerWindowId:workspaces.find(w=>w.serverId===request.params.serverId)?.ownerWindowId??'main',committed:true,persisted:true};
      if(request.method==='workspaces.move')result={operationId:request.params.operationId,phase:'committed',committed:true,persisted:true,targetWindowId:request.params.targetWindowId};
      return {requestId:request.requestId,result};
    }};
  });
  await page.goto('/');
  await expect(page.getByRole('tablist',{name:'服务器工作区'}).getByRole('tab')).toHaveCount(20);
});

test('twenty server tabs stay on one row and keyboard selection scrolls the active tab into view',async({page})=>{
  const tabs=page.getByRole('tablist',{name:'服务器工作区'}).getByRole('tab');
  await tabs.first().focus();await tabs.first().press('End');await expect(tabs.last()).toBeFocused();await expect(tabs.last()).toHaveAttribute('aria-selected','true');
  await expect.poll(()=>page.locator('.server-tabs').evaluate(el=>el.scrollLeft)).toBeGreaterThan(0);
  const rows=await tabs.evaluateAll(items=>items.map(el=>el.getBoundingClientRect().top));expect(new Set(rows).size).toBe(1);
  for(const label of ['打开服务器','最小化','最大化或还原','关闭窗口'])await expect(page.getByRole('button',{name:label,exact:true}).first()).toBeInViewport();
  expect(await page.locator('.window-drag-space').evaluate(el=>el.getBoundingClientRect().width)).toBeGreaterThanOrEqual(48);
  await tabs.last().press('Home');await expect(tabs.first()).toBeFocused();await expect(tabs.first()).toBeInViewport();
  await page.screenshot({path:'.qa/t-004/twenty-tabs-browser.png'});
});

test('picker searches account and address, locates existing workspace and only opens explicitly selected new window',async({page})=>{
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  await page.getByLabel('搜索服务器').fill('user-20');
  await expect(page.locator('.picker-row')).toHaveCount(1);await expect(page.locator('.picker-row')).toContainText('其他窗口');
  await page.locator('.picker-open').click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.filter((c:any)=>c.method==='workspaces.open').at(-1)?.params)).toMatchObject({serverId:'server-20',target:'currentWindow'});
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByLabel('搜索服务器').fill('host-21.test');
  await page.getByRole('button',{name:/在新窗口打开 服务器 21/}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.filter((c:any)=>c.method==='workspaces.open').at(-1)?.params)).toMatchObject({serverId:'server-21',target:'newWindow'});
  expect(await page.evaluate(()=>(window as any).__TABS_TEST__.calls.some((c:any)=>['connections.acquire','commands.execute','pty.open'].includes(c.method)))).toBe(false);
});

test('keyboard workspace menu selects another window through the generic move contract',async({page})=>{
  const tab=page.getByRole('tablist',{name:'服务器工作区'}).getByRole('tab').first();await tab.focus();await tab.press('Shift+F10');
  await chooseMenu(page,'window:workspace-other');
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.find((c:any)=>c.method==='workspaces.move')?.params)).toMatchObject({workspaceId:'server-0',targetWindowId:'workspace-other'});
});

test('server picker keeps search on one row and uses compact, scrollable themed content',async({page})=>{
  for(const [name,path] of [['graphite','themes/graphite.json'],['paper','tests/fixtures/themes/paper.json']]){
    const theme=JSON.parse(readFileSync(path,'utf8'));
    await page.evaluate(theme=>{for(const [key,value] of Object.entries(theme.colors))document.documentElement.style.setProperty('--'+key,String(value));document.documentElement.style.colorScheme=theme.mode;},theme);
    await page.setViewportSize({width:900,height:600});
    await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
    const search=page.getByLabel('搜索服务器');await expect(search).toBeFocused();
    const geometry=await page.locator('.server-search').evaluate(el=>{
      const icon=el.querySelector('svg')!.getBoundingClientRect(),input=el.querySelector('input')!.getBoundingClientRect(),bounds=el.getBoundingClientRect();
      return {iconCenter:icon.y+icon.height/2,inputCenter:input.y+input.height/2,iconRight:icon.right,inputLeft:input.left,inputRight:input.right,right:bounds.right,border:getComputedStyle(el.querySelector('input')!).borderTopWidth};
    });
    expect(Math.abs(geometry.iconCenter-geometry.inputCenter)).toBeLessThan(1);
    expect(geometry.inputLeft).toBeGreaterThan(geometry.iconRight);expect(geometry.inputRight).toBeLessThan(geometry.right);expect(geometry.border).toBe('0px');
    const list=await page.locator('.picker-list').evaluate(el=>({height:el.clientHeight,content:el.scrollHeight}));expect(list.content).toBeGreaterThan(list.height);
    await expect(page.getByRole('button',{name:'添加服务器',exact:true})).toBeInViewport();
    await search.fill('host-0.test');await expect(page.locator('.picker-row')).toHaveCount(1);
    expect((await page.getByRole('dialog').boundingBox())!.height).toBeLessThan(380);
    await page.screenshot({path:`.qa/t007/${name}-picker.png`});
    await search.fill('not-a-saved-server');await expect(page.getByText('没有匹配的服务器',{exact:true})).toBeVisible();
    await expect(page.getByRole('status')).toContainText('0 个匹配');
    await page.keyboard.press('Escape');await expect(page.getByRole('dialog')).toHaveCount(0);
  }
});
