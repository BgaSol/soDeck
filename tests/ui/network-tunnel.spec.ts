import {test,expect,type Page} from '@playwright/test';
import {setupNetwork as setup} from './network-fixture';

const name=(page:Page)=>page.getByLabel('连接名称（可选）',{exact:true});
const requests=(page:Page)=>page.evaluate(()=>(window as any).__NETWORK_TEST__.requests);

test('two mode drafts survive theme changes; dirty inactive draft is protected',async({page})=>{
  await setup(page);await expect(page.getByLabel('自动分配端口',{exact:true})).toBeChecked();await expect(page.getByLabel('本地端口',{exact:true})).toHaveCount(0);
  await name(page).fill('中文代理草稿');await page.getByRole('radio',{name:'端口转发',exact:true}).check();
  await page.keyboard.press('Escape');await expect(page.getByRole('dialog',{name:'放弃未保存的修改？'})).toBeVisible();await page.getByRole('button',{name:'继续编辑'}).click();
  await name(page).fill('固定草稿');await page.getByLabel('目标主机',{exact:true}).fill('service.internal');
  await page.getByRole('radio',{name:'内网代理',exact:true}).check();await expect(name(page)).toHaveValue('中文代理草稿');
  await page.evaluate(()=>(window as any).__PRESENTATION__.change('dopamine'));await expect(name(page)).toHaveValue('中文代理草稿');
  await page.getByRole('radio',{name:'端口转发',exact:true}).check();await expect(page.getByLabel('目标主机',{exact:true})).toHaveValue('service.internal');
});
test('blank optional name saves an automatic port without starting; IME Enter does not submit',async({page})=>{
  await setup(page);await name(page).dispatchEvent('compositionstart');await name(page).press('Enter');
  expect((await requests(page)).filter((r:any)=>r.params?.command==='network.editor.save')).toHaveLength(0);
  await name(page).dispatchEvent('compositionend');await page.getByRole('button',{name:'保存',exact:true}).click();
  await expect.poll(async()=>(await requests(page)).filter((r:any)=>r.method==='ui.dialog.complete').length).toBe(1);
  const rs=await requests(page),save=rs.find((r:any)=>r.params?.command==='network.editor.save');
  expect(save.params.args).toMatchObject({action:'save',draft:{name:'',auto:true,port:''}});expect(rs.some((r:any)=>r.method==='commands.execute')).toBe(false);
});
for(const code of ['RESULT_UNKNOWN','TIMEOUT'])test(`${code} freezes the draft and reconciles the same save`,async({page})=>{
  await setup(page);await name(page).fill('只保存一次');await page.evaluate(code=>(window as any).__NETWORK_TEST__.saveError=code,code);await page.getByRole('button',{name:'保存',exact:true}).click();
  await expect(name(page)).toBeDisabled();await page.getByRole('button',{name:'核对保存结果',exact:true}).click();await expect(page.getByText('保存结果仍待核实，请稍后重试')).toBeVisible();
  await page.evaluate(()=>(window as any).__NETWORK_TEST__.committed=true);await page.getByRole('button',{name:'核对保存结果',exact:true}).click();
  await expect.poll(async()=>(await requests(page)).filter((r:any)=>r.method==='ui.dialog.complete').length).toBe(1);
  expect((await requests(page)).filter((r:any)=>r.params?.command==='network.editor.save')).toHaveLength(1);
});
for(const mode of ['light','dark','dopamine'])test(`600px high and enlarged text: ${mode} dialog footer`,async({page})=>{
  await page.setViewportSize({width:900,height:600});await setup(page);await page.evaluate(mode=>(window as any).__PRESENTATION__.change(mode),mode);
  await page.getByRole('radio',{name:'端口转发',exact:true}).check();await name(page).fill('长规则名称'.repeat(15));
  await page.evaluate(()=>document.documentElement.style.fontSize='20px');await page.emulateMedia({reducedMotion:'reduce'});
  const button=page.getByRole('button',{name:'保存并启动',exact:true}),box=await button.boundingBox();expect(box!.y+box!.height).toBeLessThanOrEqual(600);
  await button.focus();await page.keyboard.press('Tab');await expect(page.getByRole('button',{name:'关闭对话框',exact:true})).toBeFocused();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth)).toBe(900);
  await page.screenshot({path:`.local/test-results/t015/network-editor-${mode}.png`});
});
test('sidebar keeps header fixed; details require selecting a rule',async({page})=>{
  await page.setViewportSize({width:360,height:600});await setup(page,false);
  await page.evaluate(()=>{(window as any).__NETWORK_TEST__.items=Array.from({length:20},(_,i)=>({rule:{id:String(i),revision:1,name:`代理 ${i}`,serverId:'server',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}}));});
  const list=page.getByRole('region',{name:'规则列表'});await expect(list).toBeVisible();const header=await page.getByRole('button',{name:'创建',exact:true}).boundingBox();
  await list.evaluate(e=>e.scrollTop=e.scrollHeight);expect(await page.getByRole('button',{name:'创建',exact:true}).boundingBox()).toEqual(header);
  await expect(page.getByRole('button',{name:'返回列表'})).toHaveCount(0);await page.getByRole('button',{name:'代理 19',exact:true}).click();
  await expect(page.getByRole('button',{name:'返回列表'})).toBeVisible();await page.getByRole('button',{name:'返回列表'}).click();await expect(list).toBeVisible();
  expect(await list.evaluate(e=>e.scrollTop)).toBeGreaterThan(0);
});

for(const mode of ['dopamine','light','dark'])test(`redesigned sidebar ${mode}: one creation entry, pinned footer, compact states`,async({page})=>{
  await page.setViewportSize({width:360,height:680});await setup(page,false);await page.evaluate(mode=>(window as any).__PRESENTATION__.change(mode),mode);
  await expect(page.getByRole('button',{name:'创建',exact:true})).toHaveCount(1);await expect(page.getByRole('button',{name:'创建隧道',exact:true})).toHaveCount(0);
  expect(await page.getByText('网络隧道',{exact:true}).count()).toBe(0);
  const footer=await page.locator('.sd-statusbar').boundingBox();expect(footer!.y+footer!.height).toBeCloseTo(680,0);
  await page.screenshot({path:`.local/test-results/t017-r2/tunnels-empty-${mode}.png`});
  await page.evaluate(()=>{const rules=[{id:'ready',revision:1,name:'开发环境代理',serverId:'server',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false},{id:'fixed',revision:1,name:'数据库连接',serverId:'server',type:'port-forward',targetHost:'database.internal',targetPort:5432,bindMode:'fixed',port:15432,autoReconnect:false},{id:'failed',revision:1,name:'测试环境',serverId:'server',type:'network-proxy',bindMode:'auto',port:0,autoReconnect:false}];(window as any).__NETWORK_TEST__.items=rules.map((rule,i)=>({rule,...(i===0?{run:{status:'ready',rule,address:{host:'127.0.0.1',port:32180},connections:2,bytesRead:512,bytesWritten:4096,started:Date.now()}}:i===2?{run:{status:'failed',rule,error:{code:'UNREACHABLE',message:'无法连接测试服务器'}}}:{})}));});
  await expect(page.getByRole('button',{name:'开发环境代理',exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'上一页'})).toHaveCount(0);
  await page.getByRole('button',{name:'复制入口 开发环境代理',exact:true}).click();await expect.poll(()=>page.evaluate(()=>(window as any).__NETWORK_TEST__.clipboard)).toBe('127.0.0.1:32180');
  await expect(page.getByRole('status',{exact:true})).toHaveText('入口已复制');
  expect(await page.locator('.sd-statusbar').boundingBox()).toEqual(footer);
  await expect(page.getByRole('status',{exact:true})).toHaveText('3 条 · 1 运行中',{timeout:6000});
  await page.screenshot({path:`.local/test-results/t017-r2/tunnels-list-${mode}.png`});
  await page.getByRole('button',{name:'开发环境代理',exact:true}).click();await expect(page.getByRole('button',{name:'复制地址'})).toBeEnabled();
  await expect(page.getByLabel('测试目标主机')).toHaveCount(0);await page.getByText('连接检查',{exact:true}).click();await expect(page.getByLabel('测试目标主机')).toBeVisible();await page.getByText('连接检查',{exact:true}).click();
  await page.screenshot({path:`.local/test-results/t017-r2/tunnels-detail-${mode}.png`});
  await page.getByRole('button',{name:'返回列表'}).click();
  await page.evaluate(()=>{const f=(window as any).__NETWORK_TEST__;f.items[0].run.status='stopped';document.documentElement.style.fontSize='26px';});
  await expect(page.getByRole('button',{name:'复制入口 开发环境代理'})).toHaveCount(0);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth)).toBe(360);
  await page.setViewportSize({width:320,height:680});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth)).toBe(320);
  const narrowFooter=await page.locator('.sd-statusbar').boundingBox();expect(narrowFooter!.y+narrowFooter!.height).toBeCloseTo(680,0);
});
