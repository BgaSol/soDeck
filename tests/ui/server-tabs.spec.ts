import { presentationFixture, chooseMenu } from "./presentation-fixture";
import {test,expect} from '@playwright/test';
import {readFileSync} from 'node:fs';

test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.setViewportSize({width:800,height:600});
  await page.addInitScript(()=>{
    const servers=Array.from({length:22},(_,i)=>({id:`server-${i}`,name:`服务器 ${i} · 很长的显示名称`,host:`host-${i}.test`,port:22,username:`user-${i}`,authType:'password'}));
    const workspaces=servers.slice(0,21).map((s,i)=>({workspaceId:s.id,serverId:s.id,generation:"test",initializers:[],initializationError:null,ownerWindowId:i===20?'workspace-other':'main'}));
    const layout={schemaVersion:4,windowId:'main',windowRole:'main',workspaceIds:servers.slice(0,20).map(s=>s.id),activeWorkspaceId:'server-0',workspaces,commands:[],viewDeclarations:[],dockSlots:[],views:[],revision:1,split:'none',splitRatio:.5,pendingOpens:[],recoveryWarnings:[]};
    const fixture={calls:[] as any[],layout};(window as any).__TABS_TEST__=fixture;
    (window as any).__TAURI_INTERNALS__={invoke:async(_:string,{request}:any)=>{
      fixture.calls.push(request);const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation)return presentation;let result:any=null;
      if(request.method==='servers.list')result={servers};
      if(request.method==='workbench.snapshot')result=JSON.parse(JSON.stringify(layout));
      if(request.method==='workbench.contributions')result={commands:layout.commands.map(c=>({...c,pluginId:'sodeck.terminal'})),views:[]};
      if(request.method==='workspaces.activate'){layout.activeWorkspaceId=request.params.workspaceId;result={workspaceId:request.params.workspaceId,ownerWindowId:'main',committed:true,persisted:true};}
      if(request.method==='workspaces.open')result={workspaceId:request.params.serverId,created:false,ownerWindowId:workspaces.find(w=>w.serverId===request.params.serverId)?.ownerWindowId??'main',committed:true,persisted:true};
      if(request.method==='workspaces.move')result={operationId:request.params.operationId,phase:'committed',committed:true,persisted:true,targetWindowId:request.params.targetWindowId};
      return {requestId:request.requestId,result};
    }};
  });
  await page.goto('/');
  await expect(page.getByRole('tablist',{name:'工作台页面'}).getByRole('tab')).toHaveCount(20);
});

test('twenty server tabs stay on one row and keyboard selection scrolls the active tab into view',async({page})=>{
  const tabs=page.getByRole('tablist',{name:'工作台页面'}).getByRole('tab');
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
  await expect(page.locator('[data-connection-row]')).toHaveCount(1);await expect(page.locator('[data-connection-row]')).toContainText('已打开');
  await page.getByRole('listitem',{name:/打开服务器/}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.filter((c:any)=>c.method==='workspaces.open').at(-1)?.params)).toMatchObject({serverId:'server-20',target:'currentWindow'});
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByLabel('搜索服务器').fill('host-21.test');
  await page.getByRole('listitem',{name:/打开服务器 服务器 21/}).click({button:'right'});await chooseMenu(page,'open');
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.filter((c:any)=>c.method==='workspaces.open').at(-1)?.params)).toMatchObject({serverId:'server-21',target:'newWindow'});
  expect(await page.evaluate(()=>(window as any).__TABS_TEST__.calls.some((c:any)=>['connections.acquire','commands.execute','pty.open'].includes(c.method)))).toBe(false);
});

test('keyboard workspace menu selects another window through the generic move contract',async({page})=>{
  const tab=page.getByRole('tablist',{name:'工作台页面'}).getByRole('tab').first();await tab.focus();await tab.press('Shift+F10');
  await chooseMenu(page,'window:workspace-other');
  await expect.poll(()=>page.evaluate(()=>(window as any).__TABS_TEST__.calls.find((c:any)=>c.method==='workspaces.move')?.params)).toMatchObject({workspaceId:'server-0',targetWindowId:'workspace-other'});
});

test('connection directory scrolls independently and keeps creation actions reachable in three themes',async({page})=>{
  await page.setViewportSize({width:900,height:600});
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  for(const mode of ['light','dark','dopamine']){
    await page.evaluate(mode=>(window as any).__PRESENTATION__.change(mode),mode);
    const search=page.getByLabel('搜索服务器');await search.fill('');
    const list=await page.locator('.connection-server-list').evaluate(el=>({height:el.clientHeight,content:el.scrollHeight}));expect(list.content).toBeGreaterThan(list.height);
    await expect(page.getByRole('button',{name:'新建 SSH 连接',exact:true}).first()).toBeInViewport();
    await search.fill('host-0.test');await expect(page.locator('[data-connection-row]')).toHaveCount(1);
    await page.screenshot({path:`.qa/t025/directory-${mode}.png`});
    await search.fill('not-a-saved-server');await expect(page.getByText('没有匹配的服务器',{exact:true})).toBeVisible();
    await expect(page.getByText('共 0 台',{exact:true})).toBeVisible();await expect(page.getByRole('dialog')).toHaveCount(0);
  }
});
