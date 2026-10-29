import {test,expect} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';
test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const context={generation:'settings-test',navigationRevision:'1',pageId:'plugin:sample:general'};
    (window as any).__SODECK_SETTINGS_CONTEXT__=context;
    const definitions=[{id:'general',title:'常规设置',order:1,schemaVersion:1,sections:[{id:'main',title:'测试字段',fields:[
      {key:'enabled',label:'显示名称',type:'boolean',default:true,effect:'immediate'},
      {key:'count',label:'数量',type:'integer',default:14,minimum:10,maximum:24,effect:'immediate',keywords:['count']},
      {key:'name',label:'名称',description:'仅用于控件测试',type:'text',default:'初始',maxLength:80,effect:'immediate',visibleWhen:{key:'enabled',equals:true}},
      {key:'mode',label:'模式',type:'enum',default:'a',options:[{value:'a',label:'模式 A'},{value:'b',label:'模式 B'}],effect:'newSession'}
    ]}]},{id:'extra',title:'其他设置',order:2,schemaVersion:1,sections:[{id:'extra',title:'另一个组',fields:[{key:'detail',label:'说明',type:'text',default:'默认',effect:'immediate'}]}]}];
    const pages=Object.fromEntries(definitions.map(definition=>{const pageId='plugin:sample:'+definition.id;return [pageId,{entry:{pageId,title:definition.title,owner:'plugin',pluginId:'sample',pluginName:'示例插件',order:definition.order,available:true,definition},definition,snapshot:{pageId,providerEpoch:'epoch',storageGeneration:'data',schemaVersion:1,settingsRevision:'0',values:definition.sections.flatMap(s=>s.fields.map(f=>({key:f.key,value:f.default}))),syncState:'current',warnings:[]}}];}));
    const f:any={pages,calls:[],failCommit:'',failRead:false};(window as any).__SETTINGS__=f;
    (window as any).__TAURI_INTERNALS__={invoke:async(_command:string,{request}:any)=>{
      const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation)return presentation;
      const {method,params:p}=request;f.calls.push({method,params:p});let result:any={accepted:true};
      if(method==='ui.settings.context.get')result=context;
      if(method==='settings.catalog.list')result={catalogRevision:'1',items:[{pageId:'core:application',title:'应用与数据',owner:'core',order:20,available:true},...Object.values(pages).map((v:any)=>v.entry)],nextCursor:null};
      if(method==='app.runtimeInfo.get')result={version:'2.0.0-test',channel:'development',mode:'development',dataPath:'C:\\test\\isolated-data'};
      if(method===f.failDirectoryMethod)return {requestId:request.requestId,error:{code:'NATIVE_ERROR',message:'目录操作失败，请重试'}};
      if(method==='settings.page.get'){if(f.failRead)return {requestId:request.requestId,error:{code:'PLUGIN_UNAVAILABLE',message:'夹具暂不可读'}};result=pages[p.pageId];}
      if(method==='settings.validate')result={errors:[]};
      if(method==='settings.commit'){
        if(f.failCommit===p.pageId)return {requestId:request.requestId,error:{code:'STORAGE_ERROR',message:'夹具模拟保存失败'}};
        const snapshot=pages[p.pageId].snapshot;snapshot.values=snapshot.values.map((v:any)=>p.changes.find((c:any)=>c.key===v.key)??v);snapshot.settingsRevision=String(Number(snapshot.settingsRevision)+1);
        result={operationId:p.operationId,persistedRevision:snapshot.settingsRevision,persistence:'saved',synchronization:'current'};
        if(f.failAfterSave)f.failRead=true;
      }
      if(method==='state.open')result={subscriptionId:crypto.randomUUID(),feedEpoch:'feed',sequence:'0',snapshot:{scope:{appGeneration:'app'},objectId:p.target.pageId??'catalog',revision:'1',value:p.contract==='settings.catalog.v1'?{catalogRevision:'1'}:pages[p.target.pageId].snapshot}};
      return {requestId:request.requestId,result:structuredClone(result)};
    }};
  });
  await page.goto('/?settings');await expect(page.getByRole('heading',{name:'常规设置'})).toBeVisible();
});
test('four controls, conditions, plugin page navigation and metadata-only search',async({page})=>{
  await expect(page.getByRole('button',{name:/示例插件/})).toHaveCount(1);
  await page.getByLabel('名称',{exact:true}).fill('私有草稿');await page.getByLabel('显示名称',{exact:false}).uncheck();
  await expect(page.getByLabel('名称',{exact:true})).toHaveCount(0);await page.getByLabel('显示名称',{exact:false}).check();await expect(page.getByLabel('名称',{exact:true})).toHaveValue('私有草稿');
  await page.getByLabel('模式',{exact:true}).selectOption('b');await page.getByLabel('数量',{exact:true}).fill('18');
  await page.getByLabel('插件设置页面').selectOption('plugin:sample:extra');await page.getByLabel('插件设置页面').selectOption('plugin:sample:general');await expect(page.getByLabel('数量',{exact:true})).toHaveValue('18');
  await page.getByLabel('搜索设置').fill('私有草稿');await expect(page.getByText('没有匹配的设置')).toBeVisible();
  await page.getByLabel('搜索设置').fill('数量');await expect(page.locator('mark')).toHaveText(['数','量']);
  await page.getByLabel('搜索设置').fill('');await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByText('更改已应用',{exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'应用',exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as any).__SETTINGS__.pages['plugin:sample:general'].snapshot.values)).toEqual(expect.arrayContaining([{key:'name',value:'私有草稿'},{key:'mode',value:'b'},{key:'count',value:18}]));
});
test('invalid integer and composition keep raw input and focus the error',async({page})=>{
  const count=page.getByLabel('数量',{exact:true});await count.fill('');await count.dispatchEvent('compositionstart');await count.fill('十');await page.keyboard.press('Escape');await expect(count).toHaveValue('十');await count.dispatchEvent('compositionend');
  await page.getByRole('button',{name:'应用',exact:true}).click();await expect(count).toBeFocused();await expect(count).toHaveAttribute('aria-invalid','true');
  expect(await page.evaluate(()=>(window as any).__SETTINGS__.calls.filter((c:any)=>c.method==='settings.commit').length)).toBe(0);
});
test('partial save retains only failed group and a durable receipt survives reread failure',async({page})=>{
  await page.getByLabel('数量',{exact:true}).fill('19');await page.getByLabel('插件设置页面').selectOption('plugin:sample:extra');await page.getByLabel('说明',{exact:true}).fill('保留');
  await page.evaluate(()=>{(window as any).__SETTINGS__.failCommit='plugin:sample:extra';});await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByText(/已保存 1 组/)).toBeVisible();await expect(page.getByLabel('说明',{exact:true})).toHaveValue('保留');
  await page.evaluate(()=>{const f=(window as any).__SETTINGS__;f.failCommit='';f.failAfterSave=true;});await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByText(/设置提供者暂不可用/)).toBeVisible();await expect(page.getByRole('button',{name:'应用',exact:true})).toBeDisabled();await expect(page.getByRole('button',{name:'核对结果'})).toHaveCount(0);
  await page.evaluate(()=>{(window as any).__SETTINGS__.failRead=false;});await page.getByRole('button',{name:'重新载入',exact:true}).click();await expect(page.getByLabel('说明',{exact:true})).toBeEnabled();
});

test('application data is read-only and directory failures can recover without losing drafts',async({page})=>{
  await page.getByLabel('数量',{exact:true}).fill('18');await page.getByRole('button',{name:'应用与数据',exact:true}).click();
  await expect(page.getByText('C:\\test\\isolated-data',{exact:true})).toBeVisible();await expect(page.locator('.settings-data-path input')).toHaveCount(0);
  for(const [method,label] of [['app.dataDirectory.open','打开目录'],['app.dataDirectory.copyPath','复制路径']]){
    await page.evaluate(m=>{(window as any).__SETTINGS__.failDirectoryMethod=m;},method);await page.getByRole('button',{name:label,exact:true}).click();await expect(page.getByText('目录操作失败，请重试')).toBeVisible();
    await page.evaluate(()=>{(window as any).__SETTINGS__.failDirectoryMethod='';});await page.getByRole('button',{name:label,exact:true}).click();await expect(page.getByText('目录操作失败，请重试')).toHaveCount(0);
  }
  await page.getByRole('button',{name:/示例插件/}).click();await expect(page.getByLabel('数量',{exact:true})).toHaveValue('18');
  expect(await page.evaluate(()=>(window as any).__SETTINGS__.calls.filter((c:any)=>c.method==='settings.commit').length)).toBe(0);
});
