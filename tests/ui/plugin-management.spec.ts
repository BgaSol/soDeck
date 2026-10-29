import {test,expect} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';
test.beforeEach(async({page})=>{
  await presentationFixture(page);await page.addInitScript(()=>{
    const context={generation:'settings',navigationRevision:'1',pageId:'core:plugins'};(window as any).__SODECK_SETTINGS_CONTEXT__=context;
    const rows=Array.from({length:105},(_,i)=>({pluginId:`sample.${i}`,displayName:`示例插件 ${i}`,enabled:true,bootEnabled:true,selected:{version:'1.0.0',displayName:`示例插件 ${i}`,signature:{status:i===0?'unsigned':i===1?'unverified':'developmentVerified'},digest:'a'.repeat(64),valid:true,error:null},pending:null,error:null,restartRequired:false,runtimeStatus:'running'}));
    const f:any={rows,revision:0,calls:[],failed:false,lost:false,receipts:{}};(window as any).__PLUGINS__=f;
    const preference=()=>({storageStatus:'ready',generation:'enablement',revision:String(f.revision),reason:null,diagnosticToken:null});
    (window as any).__TAURI_INTERNALS__={invoke:async(_command:string,{request:r}:any)=>{
      const presentation=(window as any).__PRESENTATION__.handle(r);if(presentation)return presentation;
      const p=r.params,m=r.method;f.calls.push({method:m,params:p});let result:any={accepted:true};
      if(m==='ui.settings.context.get')result=context;
      if(m==='settings.catalog.list')result={catalogRevision:'1',items:[{pageId:'core:plugins',title:'插件管理',owner:'core',order:20,available:true},{pageId:'core:application',title:'应用与数据',owner:'core',order:10,available:true}],nextCursor:null};
      if(m==='app.runtimeInfo.get')result={version:'test',channel:'development',mode:'development',dataPath:'C:/test'};
      if(m==='plugins.enablement.get')result=preference();
      if(m==='plugins.management.list'){const start=Number(p.cursor??0),end=Math.min(start+(p.limit??50),rows.length);result={catalogRevision:'a'.repeat(64),items:rows.slice(start,end),nextCursor:end<rows.length?String(end):null};}
      if(m==='plugins.enablement.validate'){if(f.failed)return {requestId:r.requestId,error:{code:'CONFLICT',message:'设置已改变'}};result={valid:true};}
      if(m==='plugins.enablement.commit'){for(const c of p.changes){const row=rows.find(row=>row.pluginId===c.pluginId)!;row.enabled=c.enabled;row.restartRequired=row.bootEnabled!==row.enabled;}f.revision++;result={operationId:p.operationId,generation:'enablement',persistedRevision:String(f.revision),restartRequired:true};f.receipts[p.operationId]=result;if(f.hung)return new Promise(()=>{});if(f.lost)return {requestId:r.requestId,error:{code:'RESULT_UNKNOWN',message:'响应丢失'}};}
      if(m==='plugins.enablement.operation.get'){if(f.hung)return new Promise(()=>{});result=f.receipts[p.operationId]?{state:'saved',result:f.receipts[p.operationId]}:{state:'unknown'};}
      if(m==='state.open')result={subscriptionId:crypto.randomUUID(),feedEpoch:'feed',sequence:'0',snapshot:{scope:{appGeneration:'app'},objectId:p.contract==='plugins.management.v1'?'plugins':'catalog',revision:'1',value:p.contract==='plugins.management.v1'?{catalogRevision:'a'.repeat(64),enablementRevision:String(f.revision),storageStatus:'ready'}:{catalogRevision:'1'}}};
      return {requestId:r.requestId,result:structuredClone(result)};
    }};
  });await page.goto('/?settings');await expect(page.getByLabel('启用 示例插件 104',{exact:true})).toBeVisible();
});
test('pagination, search and navigation retain drafts; grouped Apply preserves running state',async({page})=>{
  await expect(page.getByText('当前版本 1.0.0 · 未签名 · 完整性已通过',{exact:true})).toBeVisible();
  const first=page.getByLabel('启用 示例插件 0',{exact:true});await first.focus();await page.keyboard.press('Space');
  await page.getByLabel('搜索插件').fill('104');await page.getByLabel('启用 示例插件 104',{exact:true}).uncheck();await page.getByRole('button',{name:'应用与数据',exact:true}).click();await page.getByRole('button',{name:/^插件管理/}).click();await expect(first).not.toBeChecked();
  await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByRole('button',{name:'应用',exact:true})).toBeDisabled();
  const f=await page.evaluate(()=>(window as any).__PLUGINS__);expect(f.calls.filter((c:any)=>c.method==='plugins.enablement.commit')[0].params.changes).toHaveLength(2);expect(f.rows[0].runtimeStatus).toBe('running');
  await expect(page.getByText('本次运行：运行中 · 待重启').first()).toBeVisible();
});
test('validation failure retains draft and lost response reconciles one durable operation',async({page})=>{
  await page.getByLabel('启用 示例插件 0',{exact:true}).uncheck();await page.evaluate(()=>{(window as any).__PLUGINS__.failed=true;});await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByText('设置已改变',{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__PLUGINS__.calls.some((c:any)=>c.method==='plugins.enablement.commit'))).toBe(false);
  await page.evaluate(()=>{Object.assign((window as any).__PLUGINS__,{failed:false,lost:true});});await page.getByRole('button',{name:'应用',exact:true}).click();await expect(page.getByRole('button',{name:'应用',exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as any).__PLUGINS__.calls.filter((c:any)=>c.method==='plugins.enablement.commit').length)).toBe(1);
});
test('large text, reduced motion, keyboard and empty search remain usable',async({page})=>{
  await page.emulateMedia({reducedMotion:'reduce',forcedColors:'active'});await page.evaluate(()=>{document.documentElement.style.fontSize='26px';});
  await page.getByLabel('搜索插件').fill('不存在');await expect(page.getByText('没有匹配的插件',{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await page.getByLabel('搜索插件').fill('0');await page.getByLabel('启用 示例插件 0',{exact:true}).focus();await page.keyboard.press('Space');await expect(page.getByLabel('启用 示例插件 0',{exact:true})).not.toBeChecked();
});
test('ten-second uncertainty preserves the draft and reconciles without repeating the write',async({page})=>{
  await page.clock.install();await page.evaluate(()=>{(window as any).__PLUGINS__.hung=true;});
  const checkbox=page.getByLabel('启用 示例插件 0',{exact:true});await checkbox.uncheck();await page.getByRole('button',{name:'应用',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__PLUGINS__.calls.filter((c:any)=>c.method==='plugins.enablement.commit').length)).toBe(1);
  await page.clock.runFor(10_001);await expect(page.getByText('保存结果尚未确定，正在核对，请勿重复提交。',{exact:true})).toBeVisible();
  await expect(checkbox).not.toBeChecked();await expect(checkbox).toBeDisabled();
  await page.clock.runFor(10_001);await expect(page.getByRole('button',{name:'核对结果',exact:true})).toBeEnabled();
  await page.evaluate(()=>{(window as any).__PLUGINS__.hung=false;});await page.getByRole('button',{name:'核对结果',exact:true}).click();
  await expect(page.getByText('启用选择已保存，退出并重新启动 SoDeck 后生效。',{exact:true})).toBeVisible();await expect(checkbox).toBeEnabled();await expect(checkbox).not.toBeChecked();
  expect(await page.evaluate(()=>(window as any).__PLUGINS__.calls.filter((c:any)=>c.method==='plugins.enablement.commit').length)).toBe(1);
});
