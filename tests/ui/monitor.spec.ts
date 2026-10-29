import {test,expect} from '@playwright/test';
import {mkdir} from 'node:fs/promises';
import {presentationFixture} from './presentation-fixture';
test.beforeEach(async({page})=>{
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const scope={workspaceId:'w',workspaceGeneration:'g'},viewId='monitor-view';
    (window as any).__SODECK_CONTEXT__={...scope,viewId,serverId:'s',pluginId:'sodeck.monitor',mountGeneration:1};
    const metric=(value:any)=>({quality:'fresh',value,updatedAt:Date.now(),ageMs:1000});
    const mount={id:'1',identity:'root',path:'/',device:'8:0',fsType:'ext4',source:'/dev/sda',shared:false,network:false,temporary:false,capacity:metric({total:'107374182400',used:'53687091200',available:'42949672960',denominator:'96636764160',percent:55.5,source:'stat'}),nextReadMs:20000,readAllowed:true};
    const points=Array.from({length:31},(_,i)=>({time:(i-30)*2000,value:i===10?null:20+i}));
    const data:any={protocolVersion:1,revision:'1',viewId,serverId:'s',serverName:'开发服务器 A',workspaceGeneration:'g',connection:'connected',status:'当前 SSH 可见环境',visible:true,cleanupPending:false,limited:false,options:{refreshSeconds:2,rangeSeconds:60,paused:false,showLoopback:false,showVirtual:false},selection:{network:'eth0',disk:'sda',mount:'1',mountPage:0,mountExpanded:false,networkPage:0,diskPage:0},system:{host:'dev-a',kernel:'6.12',os:'Linux',uptime:86401},cpu:metric({percent:28.4,iowait:1.2,cores:8,load:[.4,.5,.6]}),memory:metric({total:'8589934592',used:'4294967296',available:'4294967296',percent:50,estimated:false,swapTotal:'0',swapUsed:'0'}),network:metric([{id:'eth0',name:'eth0',first:2048,second:1024}]),disk:metric([{id:'sda',name:'sda',first:4096,second:1024}]),mounts:[mount],selectedMount:mount,mountCount:1,networkCount:1,diskCount:1,partial:false,historyReleased:false,trends:{cpu:points,memory:points,networkFirst:points,networkSecond:points,diskFirst:points,diskSecond:points,capacity:[]},capturedAt:Date.now(),timelineMs:100000,diagnostics:{activeJobs:0,cacheBytes:10240}};
    let revision=1;const feeds=new Map<string,any>(),calls:any[]=[];
    const f:any={data,calls,visible:true,emit(visible:boolean){this.visible=visible;for(const [id,entry]of feeds)(window as any).__sodeckNotify({type:'state.update',payload:{subscriptionId:id,feedEpoch:'epoch',sequence:String(++entry.sequence),snapshot:{scope,objectId:viewId,revision:String(entry.sequence+1),value:{viewId,scope,visible,focused:false,revision:entry.sequence+1}}}});},next(){data.revision=String(++revision);data.timelineMs+=2000;data.trends.cpu=data.trends.cpu.map((p:any)=>({...p,time:p.time-2000})).concat({time:0,value:80});}};
    (window as any).__RESOURCE_MONITOR__=f;
    (window as any).ipc={postMessage(text:string){const request=JSON.parse(text);calls.push(request);Promise.resolve().then(()=>{
      const theme=(window as any).__PRESENTATION__.handle(request);if(theme){(window as any).__sodeckReceive(theme);return;}
      const p=request.params;let result:any={accepted:true};
      if(request.method==='view.initialize')result=data;
      if(request.method==='state.open'){const id=crypto.randomUUID();feeds.set(id,{sequence:0});result={subscriptionId:id,feedEpoch:'epoch',sequence:'0',snapshot:{scope,objectId:viewId,revision:'1',value:{viewId,scope,visible:f.visible,focused:false,revision:1}}};}
      if(request.method==='state.close')feeds.delete(p.subscriptionId);
      if(request.method==='plugin.invoke'){
        if(p.command==='monitor.snapshot')result=p.args.knownRevision===data.revision?{unchanged:true,revision:data.revision}:data;
        else {data.revision=String(++revision);if(p.command==='monitor.setOptions')Object.assign(data.options,p.args.options);if(p.command==='monitor.select'&&p.args.kind==='mount')data.selection.mountExpanded=p.args.expanded??data.selection.mountExpanded;}
      }
      (window as any).__sodeckReceive({requestId:request.requestId,result});
    });}};
  });
  await page.setViewportSize({width:320,height:900});await page.goto('http://127.0.0.1:1435');await expect(page.getByText('28.4%',{exact:true})).toBeVisible();
});
test('three themes, compact charts, keyboard and 200% text',async({page})=>{
  await mkdir('.qa/t024/ui',{recursive:true});
  for(const theme of ['light','dark','dopamine']){
    await page.evaluate(theme=>(window as any).__PRESENTATION__.change(theme),theme);
    await expect(page.locator('html')).toHaveAttribute('data-theme',theme==='dopamine'?theme:`soft-${theme}`);
    await page.screenshot({path:`.qa/t024/ui/monitor-${theme}.png`});
  }
  const graph=page.getByRole('img',{name:/CPU 使用率趋势/});await graph.focus();await graph.press('Home');
  await expect(page.locator('.sd-chart-readout').first()).toContainText('CPU 使用率 20.0');
  expect(await page.locator('.sd-chart-line').first().getAttribute('d')).toMatch(/M.*M/);
  await page.evaluate(()=>(window as any).__RESOURCE_MONITOR__.next());await page.waitForTimeout(1200);
  await expect(page.locator('.sd-chart-readout').first()).toContainText('CPU 使用率 20.0');
  await page.getByRole('button',{name:'显示数据表'}).first().click();await expect(page.locator('.sd-chart-table').first()).toContainText('暂无有效样本');
  await page.evaluate(()=>document.documentElement.style.fontSize='26px');
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await page.screenshot({path:'.qa/t024/ui/monitor-200-percent.png'});
  await page.emulateMedia({forcedColors:'active',reducedMotion:'reduce'});await expect(page.getByRole('alert')).toHaveCount(0);
});
test('pause, options, lazy mount details and hidden polling',async({page})=>{
  await page.getByRole('button',{name:'暂停采集'}).click();await expect(page.getByRole('button',{name:'恢复采集'})).toBeVisible();
  await page.getByLabel('采样间隔').selectOption('10');await page.getByLabel('趋势范围').selectOption('600');
  const fs=page.locator('.monitor-card').filter({has:page.getByText('文件系统',{exact:true})});await fs.locator('summary').click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__RESOURCE_MONITOR__.data.selection.mountExpanded)).toBe(true);
  const count=()=>page.evaluate(()=>(window as any).__RESOURCE_MONITOR__.calls.filter((c:any)=>c.method==='plugin.invoke'&&c.params.command==='monitor.snapshot').length);
  await page.evaluate(()=>(window as any).__RESOURCE_MONITOR__.emit(false));await page.waitForTimeout(300);const before=await count();await page.waitForTimeout(1400);expect(await count()).toBe(before);
  await page.evaluate(()=>(window as any).__RESOURCE_MONITOR__.emit(true));await expect.poll(count).toBeGreaterThan(before);
});
