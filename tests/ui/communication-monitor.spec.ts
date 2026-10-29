import {test,expect} from '@playwright/test';
import {mkdir} from 'node:fs/promises';
import {presentationFixture} from './presentation-fixture';

test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const now=Date.now(),feeds=new Map<string,any>(),calls:any[]=[];
    const target={kind:'all',configEpoch:'epoch'};
    let context={generation:'monitor-1',navigationRevision:'1',target:{target,layer:'channel'}};
    let policy={revision:'0',saveHistory:true,rateUnit:'bytes',notifications:true};
    let qualityPolicy={revision:'0',enabled:false,slowMs:500,recoveryMs:350,notifications:true};
    (window as any).__SODECK_MONITOR_CONTEXT__=context;
    const summary=(request:any={})=>({target:request.target??target,layer:request.layer??'channel',bootId:'boot',configEpoch:'epoch',revision:'3',serverId:request.target?.serverId??null,observedAt:now,startedAt:now-300000,sentBytes:'9007199254740993',receivedBytes:'2048000',sendRate:1536,receiveRate:8192,active:2,invalid:false,coverage:'complete',limited:'0',unattributedSentBytes:'0',unattributedReceivedBytes:'0',state:'active'});
    const row=(id:string)=>({id,serverId:id,pluginId:null,resource:null,sentBytes:'1024',receivedBytes:'2048',sendRate:1024,receiveRate:2048,active:1,invalid:false});
    const f:any={calls,feeds,summary,hold:false,held:[],release(){for(const r of this.held)r();this.held=[];},emit(){for(const [id,feed] of feeds)if(feed.contract==='communication.summary.v1'){
      (window as any).__sodeckNotify({type:'state.update',payload:{subscriptionId:id,feedEpoch:'epoch',sequence:String(++feed.sequence),snapshot:{scope:{appGeneration:'app'},objectId:feed.objectId,revision:String(feed.sequence+1),value:{...summary(feed.request),observedAt:now+feed.sequence*1000,revision:String(3+feed.sequence)}}}});
    }}};(window as any).__MONITOR__=f;
    (window as any).__TAURI_INTERNALS__={invoke:async(_command:string,{request}:any)=>{
      calls.push(request);const presentation=(window as any).__PRESENTATION__.handle(request);if(presentation)return presentation;
      const p=request.params;let result:any=null;
      const event={eventId:'event-1',sequence:'10',time:now,kind:'domain',type:'connections.route.stage',source:{source:'core'},bootId:'boot',configEpoch:'epoch',serverId:'server-a',callId:null,traceId:null,outcome:'succeeded',attributes:{stage:'active'}};
      if(request.method==='events.query')result={storeEpoch:'store',highWatermark:'10',retainedFromUtc:now-1000,retainedToUtc:now,coverage:'complete',items:[event],nextCursor:null};
      if(request.method==='events.get')result=event;
      if(request.method==='ui.communicationMonitor.get')result=context;
      if(request.method==='communication.policy.get')result=policy;
      const outcome={observedAt:now,outcome:'succeeded',sent:true,durationMs:24.5,queueMs:2,openMs:3};
      if(request.method==='quality.policy.get')result=qualityPolicy;
      if(request.method==='quality.policy.set'){
        if(f.failQuality)return {requestId:request.requestId,error:{code:'STORE_UNAVAILABLE',message:'测试：探测设置暂不可写'}};
        if((p.patch.recoveryMs??qualityPolicy.recoveryMs)>=(p.patch.slowMs??qualityPolicy.slowMs))return {requestId:request.requestId,error:{code:'INVALID_PARAMS',message:'恢复阈值必须小于提醒阈值'}};
        qualityPolicy={...qualityPolicy,...p.patch,revision:String(Number(qualityPolicy.revision)+1)};result=qualityPolicy;
      }
      if(request.method==='quality.summary')result={method:'ssh-channel-response-v1',revision:'1',observedAt:now,activeProbes:0,coverage:'complete',segments:f.qualityChecked?[{segmentId:'1',serverId:'server-a',routeRevision:'route',connectionId:'connection',generation:'1',state:'ready',enabled:qualityPolicy.enabled,latest:outcome,slow:false,unresponsive:false,statistics:{successes:1,sent:1,timeouts:0,skipped:0,pairs:0,medianMs:null,p95Ms:null,variationMs:null,timeoutRate:0}}]:[]};
      if(request.method==='quality.check.start'){
        f.qualityChecked=true;result=f.qualityTask={taskId:'check-1',operationId:p.operationId,serverId:p.serverId,routeRevision:'route',connectionId:'connection',generation:'1',segmentId:'1',method:'ssh-channel-response-v1',state:'checking',result:null,cleanupPending:true};
        if(f.holdCheck)await new Promise<void>(r=>f.held.push(r));
      }
      if(request.method==='quality.check.get')result={...f.qualityTask,state:'finished',result:outcome,cleanupPending:false};
      if(request.method==='quality.check.cancel')result={...f.qualityTask,state:'finished',result:{...outcome,outcome:'CANCELED',sent:false,durationMs:null},cleanupPending:false};
      if(request.method==='communication.policy.set'){
        if(f.failSettings)return {requestId:request.requestId,error:{code:'STORE_UNAVAILABLE',message:'测试：设置暂不可写'}};
        policy={...policy,...p.patch,revision:String(Number(policy.revision)+1)};result=policy;
      }
      if(request.method==='ui.communicationMonitor.open'){context={...context,target:p.target,navigationRevision:String(Number(context.navigationRevision)+1)};result=context;(window as any).__sodeckNotify({type:'communicationMonitor.navigate',payload:context});}
      if(request.method==='servers.routes.list')result={servers:[{id:'server-a',name:'测试服务器 A',host:'127.0.0.1',port:22331,username:'tester',authType:'key',revision:1,hasSecret:true,accessPath:{kind:'direct'}}],nextCursor:null};
      if(request.method==='state.open'){
        const id=crypto.randomUUID(),t=p.target.target;
        const tuple=[p.target.layer??'channel',t?.configEpoch??'current',t?.kind??'all',t?.serverId??'',t?.bootId??'',t?.resourceId??'',t?.generation??''];
        const objectId=p.contract==='servers.catalog.v1'?'catalog':Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(p.contract==='communication.quality.v1'?[p.target.configEpoch,p.target.serverId??'']:tuple)))),b=>b.toString(16).padStart(2,'0')).join('');
        feeds.set(id,{objectId,request:p.target,contract:p.contract,sequence:0});result={subscriptionId:id,feedEpoch:'epoch',sequence:'0',snapshot:{scope:{appGeneration:'app'},objectId,revision:'1',value:p.contract==='servers.catalog.v1'?{catalogRevision:'1'}:summary(p.target)}};
        if(p.contract==='communication.quality.v1')result.snapshot.value={configEpoch:p.target.configEpoch,serverId:p.target.serverId??null,revision:'1'};
      }
      if(request.method==='state.close'){feeds.delete(p.subscriptionId);result={accepted:true};}
      if(request.method==='state.ack')result={accepted:true};
      if(request.method==='communication.history.query')result={summary:summary(p),fromUtc:now-300000,toUtc:now,retainedFromUtc:now-300000,resolutionMs:1000,coverage:'partial',points:Array.from({length:300},(_,i)=>({fromUtc:now-(300-i)*1000,toUtc:now-(299-i)*1000,sentBytes:'1024',receivedBytes:'2048',validMs:i===100?0:1000,sendRate:i===100?null:1024+i*2,receiveRate:i===100?null:2048+i*8,gap:i===100}))};
      const stored=(range:any)=>{
        const point={fromUtc:range.fromUtc,toUtc:range.toUtc,sentBytes:p.target?.serverId?'1024':'2048',receivedBytes:'4096',validMs:60000,sendRate:1024,receiveRate:2048,gap:true,peakSendRate:16384,peakReceiveRate:32768,peakResolutionMs:1000,sampleCount:60,invalid:false};
        return {kind:'stored',target:p.target??target,layer:p.layer??'channel',storeEpoch:'store-epoch',highWatermark:'10',fromUtc:range.fromUtc,toUtc:range.toUtc,retainedFromUtc:now-3600000,resolutionMs:60000,points:[point],statistics:point,coverage:'partial',boundaryOmitted:false};
      };
      if(request.method==='communication.history.query'&&p.range.kind==='utc'){
        if(f.holdHistory)await new Promise<void>(r=>f.held.push(r));result=stored(p.range);
      }
      if(request.method==='communication.history.compare')result={first:stored(p.first),second:stored(p.second),comparable:false};
      if(['communication.sources','communication.connections'].includes(request.method)){
        if(f.hold)await new Promise<void>(r=>f.held.push(r));
        result={summary:summary(p),items:[row(p.cursor?'server-b':'server-a')],snapshotId:'snapshot',expiresAt:now+300000,nextCursor:p.cursor?null:'next',other:{sentBytes:'0',receivedBytes:'0',resources:0}};
      }
      return {requestId:request.requestId,result};
    }};
  });
  await page.setViewportSize({width:1000,height:720});await page.goto('/');
  await expect(page.getByRole('heading',{name:'通信监控 · 全部服务器',exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'测试服务器 A',exact:true})).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('three themes, keyboard curve, gaps and 200% text remain readable',async({page})=>{
  await mkdir('.qa/t019/ui',{recursive:true});
  for(const theme of ['light','dark','dopamine']) {
    await page.evaluate(theme=>(window as any).__PRESENTATION__.change(theme),theme);
    await expect(page.locator('html')).toHaveAttribute('data-theme',theme==='dopamine'?theme:`soft-${theme}`);
    const graph=page.getByRole('img',{name:/通信速率，单位/});await graph.focus();await page.keyboard.press('Home');
    await expect(page.locator('.sd-chart-readout')).toContainText('1');
    expect(await page.locator('.sd-chart-line[data-direction="send"]').getAttribute('d')).toMatch(/M.*M/);
    await page.locator('.monitor-content').evaluate(el=>el.scrollTop=0);
    await page.screenshot({path:`.qa/t019/ui/monitor-${theme}.png`});
  }
  await page.getByRole('button',{name:'显示数据表'}).click();await expect(page.locator('.sd-chart-table')).toContainText('采集缺口');
  await page.setViewportSize({width:720,height:520});await page.evaluate(()=>{document.documentElement.style.fontSize='26px';});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.locator('.monitor-content').evaluate(el=>el.scrollTop=0);
  await page.screenshot({path:'.qa/t019/ui/monitor-200-percent.png'});
  await page.emulateMedia({forcedColors:'active',reducedMotion:'reduce'});await page.getByRole('button',{name:'显示图表'}).click();
  await expect(page.locator('.sd-chart-line[data-direction="receive"]')).toHaveCSS('stroke-dasharray','6px, 4px');
});

test('table stays frozen under pointer and while paging, refresh is explicit',async({page})=>{
  await page.locator('.monitor-sources').hover();
  const count=()=>page.evaluate(()=>(window as any).__MONITOR__.calls.filter((r:any)=>r.method==='communication.sources').length);
  const before=await count();await page.waitForTimeout(5300);expect(await count()).toBe(before);
  await page.getByRole('button',{name:'下一页',exact:true}).click();await expect(page.getByText('第 2 页')).toBeVisible();
  await page.mouse.move(1,1);await page.getByRole('heading',{name:'通信监控 · 全部服务器',exact:true}).click();const paged=await count();
  await page.waitForTimeout(5300);expect(await count()).toBe(paged);
  await page.getByRole('button',{name:'刷新',exact:true}).click();await expect(page.getByText('第 1 页')).toBeVisible();expect(await count()).toBe(paged+1);
  await page.evaluate(()=>(window as any).__MONITOR__.emit());
  await expect(page.locator('.sd-metric-pair').first()).toContainText('1.5 KiB/s');
});

test('new navigation discards a delayed old source response',async({page})=>{
  await page.evaluate(()=>(window as any).__MONITOR__.hold=true);
  await page.getByRole('button',{name:'刷新',exact:true}).click();
  await page.waitForFunction(()=>(window as any).__MONITOR__.held.length===1);
  await page.getByLabel('监控范围',{exact:true}).selectOption('server-a');
  await expect(page.getByRole('heading',{name:'通信监控 · 测试服务器 A',exact:true})).toBeVisible();
  await page.evaluate(()=>{(window as any).__MONITOR__.hold=false;(window as any).__MONITOR__.release();});
  await expect(page.locator('.monitor-sources tbody')).toContainText('核心');
  await expect(page.locator('.monitor-sources tbody')).not.toContainText('测试服务器 A');
});

test('history compares honest coverage, preserves a range and supports keyboard tabs',async({page})=>{
  await page.getByRole('tab',{name:'概览',exact:true}).focus();await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab',{name:'历史',exact:true})).toHaveAttribute('aria-selected','true');
  await expect(page.getByRole('region',{name:'历史统计'})).toContainText('2.0 KiB');
  await expect(page.getByRole('region',{name:'历史统计'})).toContainText('16 KiB/s');
  await page.getByLabel('对比另一个区间').check();await page.getByRole('button',{name:'查询历史',exact:true}).click();
  await expect(page.getByRole('region',{name:'对比统计'})).toContainText('仅并列已知数值');
  const start=await page.getByLabel('开始时间',{exact:true}).inputValue();
  await page.getByRole('tab',{name:'概览',exact:true}).click();await page.getByRole('tab',{name:'历史',exact:true}).click();
  await expect(page.getByLabel('开始时间',{exact:true})).toHaveValue(start);
  await page.setViewportSize({width:720,height:520});await page.evaluate(()=>{document.documentElement.style.fontSize='26px';});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await expect(page.getByRole('alert')).toHaveCount(0);await page.screenshot({path:'.qa/t019/ui/history-200-percent.png'});
});

test('settings failure keeps the applied units, and a late history response cannot change scope',async({page})=>{
  await page.getByRole('tab',{name:'设置',exact:true}).click();await page.getByLabel('速率单位').selectOption('bits');
  await page.evaluate(()=>(window as any).__MONITOR__.failSettings=true);await page.getByRole('button',{name:'保存设置'}).click();
  await expect(page.getByRole('alert')).toContainText('暂不可写');
  await page.getByRole('tab',{name:'概览',exact:true}).click();await expect(page.locator('.sd-metric-pair').first()).toContainText('KiB/s');
  await page.getByRole('tab',{name:'设置',exact:true}).click();await page.evaluate(()=>(window as any).__MONITOR__.failSettings=false);await page.getByRole('button',{name:'保存设置'}).click();
  await expect(page.getByRole('status')).toContainText('设置已保存');
  await page.getByRole('tab',{name:'概览',exact:true}).click();await expect(page.locator('.sd-metric-pair').first()).toContainText('kbit/s');
  await page.evaluate(()=>(window as any).__MONITOR__.holdHistory=true);await page.getByRole('tab',{name:'历史',exact:true}).click();
  await page.waitForFunction(()=>(window as any).__MONITOR__.held.length===1);await page.getByLabel('监控范围',{exact:true}).selectOption('server-a');
  await page.waitForFunction(()=>(window as any).__MONITOR__.held.length===2);await page.evaluate(()=>{(window as any).__MONITOR__.holdHistory=false;(window as any).__MONITOR__.release();});
  await expect(page.getByRole('region',{name:'历史统计'})).toContainText('1.0 KiB');await expect(page.getByRole('region',{name:'历史统计'})).not.toContainText('2.0 KiB');
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('diagnostics has real states, honest sample thresholds and failed policy feedback',async({page})=>{
  await page.getByLabel('监控范围',{exact:true}).selectOption('server-a');await page.getByRole('tab',{name:'诊断',exact:true}).click();
  const toggle=page.getByLabel('持续探测：每 5 秒发送少量 SSH 请求');await expect(toggle).not.toBeChecked();
  await expect(page.getByText('尚无检测记录。主动探测默认关闭；未检测不代表连接异常。')).toBeVisible();
  await page.getByRole('button',{name:'检查响应',exact:true}).click();await expect(page.getByRole('status')).toContainText('24.5 ms');
  const stats=page.getByRole('region',{name:'测试服务器 A响应统计'});await expect(stats).toContainText('成功 1 次');await expect(stats.getByText('样本不足',{exact:true})).toHaveCount(3);
  await page.evaluate(()=>(window as any).__MONITOR__.failQuality=true);await toggle.click();await expect(page.getByRole('alert')).toContainText('暂不可写');await expect(toggle).not.toBeChecked();
  await page.evaluate(()=>(window as any).__MONITOR__.failQuality=false);await toggle.click();await expect(toggle).toBeChecked();
  for(const theme of ['light','dark','dopamine']){await page.evaluate(theme=>(window as any).__PRESENTATION__.change(theme),theme);await page.screenshot({path:`.qa/t019/ui/diagnostics-${theme}.png`});}
  await page.setViewportSize({width:720,height:520});await page.evaluate(()=>{document.documentElement.style.fontSize='26px';});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await page.screenshot({path:'.qa/t019/ui/diagnostics-200-percent.png'});
});

test('late check admission is canceled when its server view has gone away',async({page})=>{
  await page.getByLabel('监控范围',{exact:true}).selectOption('server-a');await page.getByRole('tab',{name:'诊断',exact:true}).click();
  await page.evaluate(()=>(window as any).__MONITOR__.holdCheck=true);await page.getByRole('button',{name:'检查响应',exact:true}).click();
  await page.waitForFunction(()=>(window as any).__MONITOR__.held.length===1);await page.getByLabel('监控范围',{exact:true}).selectOption('');
  await page.evaluate(()=>(window as any).__MONITOR__.release());
  await page.waitForFunction(()=>(window as any).__MONITOR__.calls.some((r:any)=>r.method==='quality.check.cancel'&&r.params.taskId==='check-1'));
  await expect(page.getByRole('button',{name:'检查响应',exact:true})).toHaveCount(0);await expect(page.getByRole('alert')).toHaveCount(0);
});
