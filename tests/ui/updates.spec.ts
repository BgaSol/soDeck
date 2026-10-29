import {test,expect} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';

test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const context={generation:'settings',navigationRevision:'1',pageId:'core:updates'};(window as any).__SODECK_SETTINGS_CONTEXT__=context;
    const rows=['core','sodeck.terminal','sodeck.files','sodeck.network-proxy','sodeck.monitor'].map(id=>({componentId:id,currentVersion:'1.0.0',availableVersion:'1.1.0',selected:false,stage:'available',received:0,total:1048576,blockedReasons:[],requiredComponentIds:[],origin:id==='core'?'core':'official',followOfficial:true,error:null}));
    const f:any={rows,calls:[],operations:{},policy:{generation:'policy',revision:'0',autoCheck:true},lost:false,conflict:false};(window as any).__UPDATES__=f;
    const status=()=>({revision:'status',channel:'preview',appVersion:'2.0.0-preview.1',buildId:'build',distribution:'installed',configured:true,checking:false,lastCheck:1,lastSuccess:1,retryAfter:0,error:null,warnings:[],policy:f.policy,restartRequired:rows.some(r=>r.stage==='ready')});
    (window as any).__TAURI_INTERNALS__={invoke:async(_command:string,{request:r}:any)=>{
      const presentation=(window as any).__PRESENTATION__.handle(r);if(presentation)return presentation;
      const p=r.params,m=r.method;f.calls.push({method:m,params:p});let result:any={accepted:true};
      if(m==='ui.settings.context.get')result=context;
      if(m==='settings.catalog.list')result={catalogRevision:'1',items:[{pageId:'core:plugins',title:'插件管理',owner:'core',order:20,available:true},{pageId:'core:updates',title:'更新',owner:'core',order:30,available:true}],nextCursor:null};
      if(m==='plugins.enablement.get')result={generation:'enablement',revision:'0',storageStatus:'ready',reason:null,diagnosticToken:null};
      if(m==='plugins.management.list')result={catalogRevision:'a'.repeat(64),items:rows.filter(r=>r.componentId!=='core').map(r=>({pluginId:r.componentId,displayName:r.componentId,enabled:true,bootEnabled:true,selected:{version:'1.0.0',displayName:r.componentId,signature:{status:'verified'},digest:'a'.repeat(64),valid:true,error:null},pending:null,error:null,restartRequired:false,runtimeStatus:'running'})),nextCursor:null};
      if(m==='updates.status.get')result=status();
      if(m==='updates.components.list')result={revision:'list',items:rows,nextCursor:null};
      if(m==='updates.plan.get')result={planRevision:'plan',catalogRevision:'catalog',components:rows.map(row=>({...row,selected:p.componentIds.includes(row.componentId)})),componentIds:p.componentIds,downloadBytes:p.componentIds.length*1048576,blockedReasons:[]};
      if(m==='updates.policy.validate'){if(f.conflict)return {requestId:r.requestId,error:{code:'CONFLICT',message:'更新设置已改变'}};result={valid:true};}
      if(m==='updates.policy.commit'){f.policy={...f.policy,revision:String(Number(f.policy.revision)+1),autoCheck:p.autoCheck};f.policyReceipt=p.operationId;result=f.policy;if(f.lost)return {requestId:r.requestId,error:{code:'RESULT_UNKNOWN',message:'响应丢失'}};}
      if(m==='updates.policy.operation.get')result={state:'saved',result:f.policy};
      if(['updates.download','updates.install','updates.check','app.restartForUpdates'].includes(m)){
        const selected=rows.filter(row=>(p.componentIds??[]).includes(row.componentId));for(const row of selected)row.stage=m==='updates.download'?'downloaded':'ready';
        result={operationId:p.operationId,kind:m,stage:m==='updates.download'?'downloaded':m==='updates.check'?'installed':'ready',running:false,components:selected.map(row=>({componentId:row.componentId,stage:row.stage,received:row.total,total:row.total,error:null})),error:null,updatedAt:1};f.operations[p.operationId]=result;
      }
      if(m==='updates.operation.get')result=f.operations[p.operationId];
      if(m==='state.open')result={subscriptionId:crypto.randomUUID(),feedEpoch:'feed',sequence:'0',snapshot:{scope:{appGeneration:'app'},objectId:p.contract==='plugins.management.v1'?'plugins':p.contract==='updates.state.v1'?'updates':'catalog',revision:'1',value:{revision:'1'}}};
      return {requestId:r.requestId,result:structuredClone(result)};
    }};
  });
  await page.goto('/?settings');await expect(page.getByRole('heading',{name:'更新',exact:true})).toBeVisible();
});

test('checking does not download; a plugin updates independently with two clicks',async({page})=>{
  await page.getByRole('button',{name:'检查更新',exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__UPDATES__.calls.filter((c:any)=>c.method==='updates.download'))).toHaveLength(0);
  await page.getByLabel('选择 终端',{exact:true}).check();
  await page.getByRole('button',{name:/下载所选更新/}).click();
  await expect(page.getByRole('button',{name:'安装所选插件',exact:true})).toBeEnabled();
  expect(await page.evaluate(()=>(window as any).__UPDATES__.calls.filter((c:any)=>c.method==='updates.install'))).toHaveLength(0);
  await page.getByRole('button',{name:'安装所选插件',exact:true}).click();
  await expect(page.getByRole('button',{name:'重启应用',exact:true})).toBeVisible();
  const actions=await page.evaluate(()=>(window as any).__UPDATES__.calls.filter((c:any)=>['updates.download','updates.install'].includes(c.method)));
  expect(actions.map((c:any)=>c.params.componentIds)).toEqual([['sodeck.terminal'],['sodeck.terminal']]);
});

test('auto-check is a shared unsaved draft, and unknown save replies reconcile once',async({page})=>{
  await page.getByLabel('自动检查并提示更新',{exact:true}).uncheck();
  await page.getByRole('button',{name:/^插件管理/}).click();await page.getByRole('button',{name:/^更新/}).click();
  await expect(page.getByLabel('自动检查并提示更新',{exact:true})).not.toBeChecked();
  expect(await page.evaluate(()=>(window as any).__UPDATES__.policy.autoCheck)).toBe(true);
  await page.evaluate(()=>{(window as any).__UPDATES__.lost=true;});await page.getByRole('button',{name:'应用',exact:true}).click();
  await expect(page.getByText('更改已应用',{exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'应用',exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as any).__UPDATES__.calls.filter((c:any)=>c.method==='updates.policy.commit').length)).toBe(1);
  expect(await page.evaluate(()=>(window as any).__UPDATES__.policy.autoCheck)).toBe(false);
});

test('narrow windows, 200 percent text, keyboard and reduced motion retain all actions',async({page})=>{
  await page.setViewportSize({width:760,height:700});await page.emulateMedia({reducedMotion:'reduce'});
  await page.evaluate(()=>{document.documentElement.style.fontSize='28px';});
  await page.getByLabel('选择 文件',{exact:true}).focus();await page.keyboard.press('Space');await expect(page.getByLabel('选择 文件',{exact:true})).toBeChecked();
  await expect(page.getByRole('button',{name:/下载所选更新/})).toBeVisible();expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});
