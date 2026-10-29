import {test,expect} from '@playwright/test';
import {connectionHomeFixture} from './connection-home-fixture';
test.beforeEach(async({page})=>{await connectionHomeFixture(page);await page.goto('/');await expect(page.getByRole('heading',{name:'新建 SSH 连接',exact:true})).toBeVisible();});
const fill=async(page:import('@playwright/test').Page,host='target.invalid')=>{await page.getByLabel('主机地址',{exact:true}).fill(host);await page.getByLabel('用户名',{exact:true}).fill('tester');};
test('startup presents defaults, only save creates no workspace, list opens directly, plus preserves an independent page',async({page})=>{
  await expect(page.getByRole('dialog')).toHaveCount(0);await expect(page.getByLabel('SSH 端口',{exact:true})).toHaveValue('22');await expect(page.getByLabel('用户名',{exact:true})).toHaveValue('');
  await fill(page);await page.getByRole('button',{name:'仅保存',exact:true}).click();await expect(page.getByText('连接已保存',{exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.layout.workspaceIds)).toHaveLength(0);
  await page.getByRole('listitem',{name:'打开服务器 target.invalid'}).click();await expect(page.getByRole('tab',{name:'target.invalid',exact:true})).toHaveAttribute('aria-selected','true');
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await expect(page.getByRole('tab',{name:'新建连接',exact:true})).toHaveAttribute('aria-selected','true');await expect(page.locator('[data-workspace-id]')).toHaveCount(1);
});
test('inline proxy is durable and reusable, unknown response reconciles once, canceling parent keeps proxy',async({page})=>{
  await fill(page);await page.getByLabel('经由中转',{exact:true}).check();await page.getByRole('button',{name:'添加 SOCKS5',exact:true}).click();await page.getByRole('button',{name:'新建代理',exact:true}).last().click();
  const inline=page.getByRole('region',{name:'就地新建代理'});await inline.getByLabel('代理地址',{exact:true}).fill('proxy.invalid');
  await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.loseProxy=true);await inline.getByRole('button',{name:'保存并选用'}).click();await expect(inline.getByText(/配置|回执/).first()).toBeVisible();
  await inline.getByRole('button',{name:'核对保存结果'}).click();await expect(inline).toHaveCount(0);await expect(page.getByRole('combobox',{name:'第 1 站 SOCKS5 代理',exact:true})).toHaveValue('proxy.invalid:1080');
  await page.getByRole('button',{name:'取消',exact:true}).last().click();await page.getByRole('button',{name:'放弃修改',exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.proxies.length)).toBe(1);expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.items.length)).toBe(0);
});
test('jump nodes must already exist, drafts retain fields and secrets across navigation',async({page})=>{
  await fill(page,'jump.invalid');await page.getByRole('button',{name:'仅保存'}).click();await page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await fill(page);
  await page.getByLabel('登录密码',{exact:true}).fill('isolated-test');await page.getByLabel('经由中转',{exact:true}).check();await page.getByRole('button',{name:'添加 SSH',exact:true}).click();await page.getByRole('combobox',{name:'第 1 站 SSH 服务器'}).fill('jump');await page.getByRole('option',{name:/jump.invalid/}).click();
  await expect(page.getByRole('button',{name:/新建跳板/})).toHaveCount(0);await page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await fill(page,'another.invalid');
  await page.getByLabel(/未完成配置/).selectOption({label:'target.invalid · 未完成'});await expect(page.getByLabel('登录密码',{exact:true})).toHaveValue('isolated-test');await expect(page.getByText('最终目标 · target.invalid · 由SSH 主机 jump.invalid访问')).toBeVisible();
  expect(await page.locator('form form').count()).toBe(0);
});
test('credential failure retries the saved object and delayed save cannot navigate over a new draft',async({page})=>{
  await fill(page);await page.getByLabel('登录密码',{exact:true}).fill('isolated-test');await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.failCredential=true);await page.getByRole('button',{name:'保存并连接'}).click();await expect(page.getByText(/配置已保存，后续步骤未完成/)).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.items.length)).toBe(1);expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.layout.workspaceIds.length)).toBe(0);
  await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.failCredential=false);await page.getByRole('button',{name:'重试后续步骤'}).click();await expect(page.getByRole('tab',{name:'target.invalid',exact:true})).toHaveAttribute('aria-selected','true');
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await fill(page,'slow.invalid');await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.holdSave=true);await page.getByRole('button',{name:'保存并连接'}).click();
  await page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await fill(page,'keep.invalid');await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.releaseSave());await expect(page.getByLabel('主机地址',{exact:true})).toHaveValue('keep.invalid');
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.layout.workspaceIds.length)).toBe(1);
});
test('close cancellation retains latest input and confirmation disposes it',async({page})=>{
  await fill(page);await page.getByRole('button',{name:'关闭新建连接页'}).click();await page.getByRole('button',{name:'继续编辑'}).click();await expect(page.getByLabel('主机地址',{exact:true})).toHaveValue('target.invalid');
  await page.getByRole('button',{name:'关闭新建连接页'}).click();await page.getByRole('button',{name:'放弃修改',exact:true}).click();await expect(page.getByRole('tab',{name:'新建连接',exact:true})).toHaveCount(0);
});

test('closing waits for an unresolved save, then explicit abandonment cannot cause a late navigation',async({page})=>{
  await fill(page,'pending.invalid');await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.holdSave=true);await page.getByRole('button',{name:'保存并连接',exact:true}).click();
  await page.getByRole('button',{name:'关闭新建连接页'}).click();const abandon=page.getByRole('button',{name:'放弃等待并关闭',exact:true});await expect(abandon).toBeDisabled();
  await page.clock.install();await page.clock.fastForward(11000);await expect(abandon).toBeEnabled();await abandon.click();await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.releaseSave());
  await expect(page.getByRole('tab',{name:'新建连接',exact:true})).toHaveCount(0);expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.items.length)).toBe(1);expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.layout.workspaceIds.length)).toBe(0);
});
test('a hidden home still reviews drafts, and stale completion cannot discard them',async({page})=>{
  await fill(page,'saved.invalid');await page.getByRole('button',{name:'仅保存'}).click();await page.getByRole('button',{name:'新建 SSH 连接',exact:true}).click();await fill(page,'keep.invalid');
  await page.getByRole('listitem',{name:'打开服务器 saved.invalid'}).click();
  await page.evaluate(()=>{const w=window as any;w.__sodeckNotify({type:'connectionHome.closeRequested',payload:{ticket:'current',pageSession:w.__CONNECTION_HOME_TEST__.session}});});
  await expect(page.getByRole('dialog',{name:'关闭前处理未完成配置'})).toBeVisible();
  await page.evaluate(()=>(window as any).__sodeckNotify({type:'connectionHome.closeFinished',payload:{ticket:'obsolete',committed:true}}));
  await expect(page.getByRole('dialog',{name:'关闭前处理未完成配置'})).toBeVisible();await page.getByRole('button',{name:'继续编辑'}).click();
  await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await expect(page.getByLabel('主机地址',{exact:true})).toHaveValue('keep.invalid');
});
test('all themes keep 900×600 actions reachable, narrow window has no horizontal overflow, Enter does not submit',async({page})=>{
  await page.setViewportSize({width:900,height:600});
  for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>(window as any).__PRESENTATION__.change(theme),theme);await expect(page.getByRole('button',{name:'保存并连接'})).toBeInViewport();await page.screenshot({path:`.qa/t027/home-${theme}.png`});}
  await fill(page);await page.getByLabel('用户名',{exact:true}).press('Enter');expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.items.length)).toBe(0);
  await page.setViewportSize({width:520,height:650});await expect(page.getByRole('button',{name:'保存并连接'})).toBeInViewport();expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await page.screenshot({path:'.qa/t027/home-narrow.png'});
});

test('large paginated directory and selectors have bounded DOM; 100 page switches keep one catalog subscription',async({page})=>{
  test.setTimeout(120000);
  await page.evaluate(()=>{const f=(window as any).__CONNECTION_HOME_TEST__;
    f.items=Array.from({length:1000},(_,i)=>({server:{id:`server-${i}`,name:`Server ${String(i).padStart(4,'0')}`,host:`host-${i}.invalid`,port:22,username:'tester',authType:'password',revision:1,hasSecret:false},accessPath:{steps:[]},groupId:null,membershipRevision:'1',postCommitPending:false}));
    f.proxies=Array.from({length:1000},(_,i)=>({id:`proxy-${i}`,name:`Proxy ${String(i).padStart(4,'0')}`,host:`proxy-${i}.invalid`,port:1080,authType:'none',username:'',revision:1,hasPreset:false}));f.changed();
  });
  await expect(page.getByText('共 1000 台',{exact:true})).toBeVisible();expect(await page.locator('[data-connection-row]').count()).toBeLessThan(40);
  await page.getByLabel('经由中转',{exact:true}).check();await page.getByRole('button',{name:'添加 SOCKS5',exact:true}).click();await page.getByRole('combobox',{name:'第 1 站 SOCKS5 代理',exact:true}).fill('Proxy');expect(await page.getByRole('option').count()).toBeLessThan(100);
  await page.getByRole('combobox',{name:'第 1 站 SOCKS5 代理',exact:true}).fill('0999');await page.getByRole('option',{name:/Proxy 0999/}).click();
  await page.getByLabel('搜索服务器').fill('host-999.invalid');await page.getByRole('listitem',{name:'打开服务器 Server 0999'}).click();
  const before=await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.feeds.size);
  for(let n=0;n<100;n++){
    await page.getByRole('button',{name:'打开服务器',exact:true}).first().click();await expect(page.getByLabel('搜索服务器')).toHaveValue('host-999.invalid');await page.getByRole('tab',{name:'Server 0999',exact:true}).click();
  }
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.feeds.size)).toBe(before);
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.layout.workspaceIds.length)).toBe(1);
  expect(await page.evaluate(()=>(window as any).__CONNECTION_HOME_TEST__.calls.filter((c:any)=>c.method==='state.open'&&c.params.contract==='servers.catalog.v2').length)).toBe(1);
});
