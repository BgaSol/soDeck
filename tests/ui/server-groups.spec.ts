import {test,expect} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';
test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const f={groups:[] as any[],items:[] as any[],calls:[] as any[],receipts:{} as any,loseGroup:false,loseSave:false,revision:1};(window as any).__GROUPS_TEST__=f;
    (window as any).__TAURI_INTERNALS__={invoke:async(_:string,{request:r}:any)=>{
      f.calls.push(r);const theme=(window as any).__PRESENTATION__.handle(r);if(theme)return theme;
      const p=r.params;let result:any=null;
      if(r.method==='capabilities.list')result={capabilities:['servers.catalog.v1','servers.routes.v2']};
      if(r.method==='workbench.snapshot')result={schemaVersion:4,windowId:'main',windowRole:'main',workspaceIds:[],workspaces:[],activeWorkspaceId:null,dockSlots:[],views:[],revision:1,split:'none',splitRatio:.5,pendingOpens:[],recoveryWarnings:[]};
      if(r.method==='workbench.contributions')result={commands:[],views:[]};
      if(r.method==='servers.catalog.list')result={items:f.items,catalogRevision:String(f.revision),nextCursor:null};
      if(r.method==='serverGroups.list')result={groups:f.groups,catalogRevision:String(f.revision),nextCursor:null};
      if(r.method==='state.open'&&p.contract==='servers.catalog.v1')result={subscriptionId:crypto.randomUUID(),feedEpoch:'catalog',sequence:'0',snapshot:{scope:{appGeneration:'test'},objectId:'catalog',revision:String(f.revision),value:{catalogRevision:String(f.revision)}}};
      if(['state.close','state.ack'].includes(r.method))result={accepted:true};
      if(r.method==='serverGroups.create'||r.method==='serverGroups.update'){
        result=f.receipts[p.operationId];if(!result){let g=f.groups.find(g=>g.id===p.groupId);if(g){g.name=p.name;g.revision=String(Number(g.revision)+1);}else{g={id:crypto.randomUUID(),name:p.name,revision:'1',serverCount:0};f.groups.push(g);}result=f.receipts[p.operationId]={group:{...g},catalogRevision:String(++f.revision)};}
        if(f.loseGroup){f.loseGroup=false;return {requestId:r.requestId,error:{code:'RESULT_UNKNOWN',message:'测试：回执丢失'}};}
      }
      if(r.method==='servers.catalog.save'){
        result=f.receipts[p.operationId];if(!result){const {connectionRoute,...config}=p.configuration;const server={...config,id:crypto.randomUUID(),revision:1,hasSecret:false};result={server,connectionRoute,groupId:p.groupId,membershipRevision:'1',postCommitPending:false};f.items.push(result);const g=f.groups.find(g=>g.id===p.groupId);if(g)g.serverCount++;f.receipts[p.operationId]=result;f.revision++;}
        if(f.loseSave){f.loseSave=false;return {requestId:r.requestId,error:{code:'RESULT_UNKNOWN',message:'测试：回执丢失'}};}
      }
      if(r.method==='servers.catalog.operation')result=f.receipts[p.operationId]?{status:'committed',result:f.receipts[p.operationId]}:{status:'unknown'};
      if(r.method==='serverGroups.delete'){const g=f.groups.find(g=>g.id===p.groupId);f.groups=f.groups.filter(g=>g!==g);for(const item of f.items)if(item.groupId===g.id)item.groupId=null;result={deleted:true,movedCount:g.serverCount,catalogRevision:String(++f.revision)};}
      if(r.method==='workspaces.open')result={workspaceId:p.serverId,created:true,ownerWindowId:'main',committed:true,persisted:true};
      if(r.method==='auth.set')result={remembered:true};
      return {requestId:r.requestId,result};
    }};
  });
  await page.goto('/');await page.getByRole('button',{name:'添加服务器',exact:true}).click();
});
test('creating a group is independent, then assigning, filtering, renaming and deleting preserves servers',async({page})=>{
  await page.getByRole('button',{name:'新建分组',exact:true}).click();await page.getByLabel('分组名称').fill('生产');await page.getByRole('button',{name:'保存分组'}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__GROUPS_TEST__.groups.length)).toBe(1);
  await expect(page.getByRole('combobox',{name:'分组',exact:true})).toHaveValue(await page.evaluate(()=>(window as any).__GROUPS_TEST__.groups[0].id));
  await page.getByRole('button',{name:'取消',exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__GROUPS_TEST__.groups.length)).toBe(1);
  await page.getByRole('button',{name:'添加服务器',exact:true}).click();
  await page.getByLabel('显示名称',{exact:true}).fill('生产一号');await page.getByLabel('主机地址',{exact:true}).fill('test.invalid');await page.getByLabel('用户名',{exact:true}).fill('tester');
  await page.getByRole('combobox',{name:'分组',exact:true}).selectOption({label:'生产'});await page.evaluate(()=>(window as any).__GROUPS_TEST__.loseSave=true);
  await page.getByRole('button',{name:'保存并打开'}).click();await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByLabel('按分组筛选').selectOption('');await expect(page.locator('.picker-row')).toHaveCount(0);
  await page.getByLabel('按分组筛选').selectOption({label:'生产 · 1'});await expect(page.locator('.picker-row')).toHaveCount(1);
  await page.getByRole('button',{name:'管理分组'}).click();await page.getByRole('button',{name:'改名',exact:true}).click();await page.getByLabel('分组名称').fill('生产环境');await page.getByRole('button',{name:'保存分组'}).click();
  await expect(page.getByRole('button',{name:'生产环境 · 1 台'})).toBeVisible();await page.getByRole('button',{name:'删除',exact:true}).click();await page.getByRole('button',{name:'删除分组',exact:true}).click();
  await expect(page.getByText('还没有分组，服务器默认显示在未分组中。')).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__GROUPS_TEST__.items.map((i:any)=>i.groupId))).toEqual([null]);
});
test('an ambiguous group save locks the draft and retries the same durable operation',async({page})=>{
  await page.getByRole('button',{name:'新建分组',exact:true}).click();await page.getByLabel('分组名称').fill('测试');await page.evaluate(()=>(window as any).__GROUPS_TEST__.loseGroup=true);
  await page.getByRole('button',{name:'保存分组'}).click();await expect(page.getByText(/保存结果待核实/)).toBeVisible();await expect(page.getByLabel('分组名称')).toBeDisabled();
  await page.getByRole('button',{name:'保存分组'}).click();
  const state=await page.evaluate(()=>{const f=(window as any).__GROUPS_TEST__;return {groups:f.groups.length,ids:f.calls.filter((r:any)=>r.method==='serverGroups.create').map((r:any)=>r.params.operationId)};});
  expect(state.groups).toBe(1);expect(new Set(state.ids).size).toBe(1);
});
test('inputs keep their focus inside, credentials share a row and window controls keep equal theme geometry',async({page})=>{
  const measurements=[];
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(t=>(window as any).__PRESENTATION__.change(t),theme);
    const username=page.getByLabel('用户名',{exact:true});await username.focus();const before=await username.boundingBox();await page.getByLabel('主机地址',{exact:true}).focus();expect(await username.boundingBox()).toEqual(before);
    const password=page.getByLabel(/登录密码/);const ub=await username.boundingBox(),pb=await page.locator(".password-field").boundingBox();expect(Math.abs(ub!.y-pb!.y)).toBeLessThan(1);expect(pb!.x).toBeGreaterThan(ub!.x+ub!.width);
    await username.focus();expect(await username.evaluate(el=>getComputedStyle(el).outlineOffset)).toBe('-2px');
    measurements.push(await page.locator('.window-controls button').evaluateAll(elements=>elements.map(el=>{const r=el.getBoundingClientRect();return {width:r.width,height:r.height};})));
    await page.screenshot({path:`.local/test-results/t015/server-form-${theme}.png`});
  }
  expect(measurements[0]).toEqual(measurements[1]);expect(measurements[1]).toEqual(measurements[2]);
  expect(new Set(measurements[0].map(b=>`${b.width}:${b.height}`)).size).toBe(1);
  await page.setViewportSize({width:420,height:650});
  const username=await page.getByLabel('用户名',{exact:true}).boundingBox(),password=await page.getByLabel(/登录密码/).boundingBox();expect(password!.y).toBeGreaterThan(username!.y+username!.height);
});
