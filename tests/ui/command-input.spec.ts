import { test, expect } from '@playwright/test';
import { presentationFixture,chooseMenu } from './presentation-fixture';
test.beforeEach(async({page})=>{
 await presentationFixture(page);
 await page.addInitScript(()=>{
 const view={viewId:'view-1',pluginId:'sample.plugin',viewType:'sample.page',workspaceId:'s',serverId:'s',title:'终端 1',status:'ended',placement:'center',group:0,active:true,mountGeneration:1,revision:1,stateVersion:1,actions:['sample.rename']};
 const layout={schemaVersion:3,windowId:'main',windowRole:'main',workspaceIds:['s'],activeWorkspaceId:'s',workspaces:[{workspaceId:'s',serverId:'s',ownerWindowId:'main'}],commands:[{id:'sample.rename',title:'修改显示名称',scope:'view',primary:false,input:{kind:'text',key:'title',label:'显示名称',initial:'viewTitle',minLength:1,maxLength:80}}],viewDeclarations:[],dockSlots:[],views:[view],revision:1,split:'none',splitRatio:.5,pendingOpens:[],recoveryWarnings:[]};
 const f={calls:[] as any[],fail:false};(window as any).__INPUT_TEST__=f;
 (window as any).__TAURI_INTERNALS__={invoke:async(_:string,{request}:any)=>{f.calls.push(request);const p=(window as any).__PRESENTATION__.handle(request);if(p)return p;let result:any=null;
 if(request.method==='servers.list')result={servers:[{id:'s',name:'隔离服务器',host:'test',port:22,username:'test'}]};if(request.method==='workbench.snapshot')result=JSON.parse(JSON.stringify(layout));
 if(request.method==='commands.execute'){if(f.fail){f.fail=false;return {requestId:request.requestId,error:{code:'BUSY',message:'隔离重命名失败，请重试'}};}view.title=request.params.args.title;}
 return {requestId:request.requestId,result};}};
 });await page.goto('/');await page.getByRole('tab',{name:'终端 1',exact:true}).click({button:'right'});await chooseMenu(page,'command:sample.rename');
});
test('generic command input validates, cancels and saves through declared key',async({page})=>{
 const field=page.getByRole('textbox',{name:'显示名称',exact:true});await expect(field).toHaveValue('终端 1');await field.fill('临时草稿');await page.locator('.dialog-backdrop').click({position:{x:2,y:2}});await expect(field).toHaveValue('临时草稿');await field.press('Escape');expect(await page.evaluate(()=>(window as any).__INPUT_TEST__.calls.filter((c:any)=>c.method==='commands.execute').length)).toBe(0);
 await page.getByRole('tab',{name:'终端 1',exact:true}).click({button:'right'});await chooseMenu(page,'command:sample.rename');await field.fill(' ');await expect(page.getByRole('button',{name:'保存',exact:true})).toBeDisabled();await field.fill('x'.repeat(81));await expect(page.getByRole('button',{name:'保存',exact:true})).toBeDisabled();await field.fill('  新名称🙂  ');await field.press('Enter');await expect(page.getByRole('tab',{name:'新名称🙂',exact:true})).toBeVisible();
});
test('IME Enter is ignored and failed save keeps the draft',async({page})=>{
 const field=page.getByRole('textbox',{name:'显示名称',exact:true});await field.fill('组合中的名字');await field.dispatchEvent('keydown',{key:'Enter',code:'Enter',isComposing:true,bubbles:true,cancelable:true});expect(await page.evaluate(()=>(window as any).__INPUT_TEST__.calls.filter((c:any)=>c.method==='commands.execute').length)).toBe(0);
 await page.evaluate(()=>(window as any).__INPUT_TEST__.fail=true);await field.press('Enter');await expect(page.getByRole('alert')).toContainText('隔离重命名失败');await expect(field).toHaveValue('组合中的名字');await page.getByRole('button',{name:'保存',exact:true}).click();await expect(page.getByRole('tab',{name:'组合中的名字',exact:true})).toBeVisible();
});
