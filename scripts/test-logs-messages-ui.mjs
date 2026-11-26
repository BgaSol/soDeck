import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const vite=await createServer({server:{host:'127.0.0.1',port:0}});await vite.listen();
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1280,height:800}});
const url=vite.resolvedUrls.local[0],errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.route(url,route=>route.fulfill({contentType:'text/html; charset=utf-8',body:`<html data-theme="light"><head><meta charset="utf-8"><link rel="stylesheet" href="/style.css"></head><body><div id="app"></div><script type="module">
import {LogsPage} from '/logs.ts';import {MessagesPage} from '/messages.ts';import {Shell} from '/shell.ts';
window.calls=[];window.held=[];window.hold='';window.fail='';window.copies=[];window.located=[];window.changes=0;
const time=Date.parse('2026-11-26T06:32:08Z');
window.logData=Array.from({length:500},(_,i)=>({id:'l'+i,receivedAt:time+i,source:i%2?'plugin':'core',pluginId:'sodeck.docker',instanceId:'instance-'+i,level:i%5?'info':'error',message:i+' 多行正文 <script>literal</scr'+'ipt> '+(i===499?'长正文'.repeat(2000):'\\n第二行'),fields:{sequence:i,trace:'secret-structure-'+i}}));
window.messageData=Array.from({length:1000},(_,i)=>({id:'m'+i,receivedAt:time-i*1000,source:'plugin',pluginId:'sodeck.docker',instanceId:'message-instance',level:i%2?'info':'error',title:'消息 '+i,body:i===0?'首条消息正文\\n<svg onload="throw Error(1)">literal</svg>':'正文 '+i,read:i>2,count:1}));
window.context={plugins:[{manifest:{id:'sodeck.docker',name:'Docker'}}],storageErrors:[]};
window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
 calls.push({command,args});let value;
 if(command==='read_logs'){const f=args.filter;value=logData.filter(l=>(!f.query||l.message.includes(f.query))&&(!f.level||l.level===f.level)&&(!f.pluginId||f.pluginId==='core'&&l.source==='core'||l.pluginId===f.pluginId)&&(f.since===undefined||l.receivedAt>=f.since)&&(f.until===undefined||l.receivedAt<=f.until));}
 if(command==='read_messages')value=messageData;
 value=structuredClone(value);
 if(hold===command)await new Promise((resolve,reject)=>held.push({command,resolve,reject}));
 if(fail===command){fail='';throw Error('受控失败 '+command);}
 if(command==='mark_read'){messageData.forEach(m=>{if(args.id===null||args.id===m.id)m.read=true;});return;}
 return value;
}};
window.logs=new LogsPage({logs:filter=>__TAURI_INTERNALS__.invoke('read_logs',{filter}),copy:async text=>copies.push(text)});
window.messages=new MessagesPage(id=>located.push(id),()=>changes++);
window.shell=new Shell(document.querySelector('#app'),true);shell.attach({setHostVisible(){},setHostCallbacks(changed){changed({window:{activeWorkspace:'w'},workspaces:[]});}},{setNavigation(){}});
const host=document.querySelector('#layer-content');
shell.onPage=page=>{logs.mount(null);messages.mount(null);host.innerHTML='';host.className='layer-content '+page+'-content';if(page==='logs')logs.mount(host);if(page==='messages')messages.mount(host);logs.setContext(context);messages.setContext(context);};
shell.onConsoleVisibility=page=>{logs.setActive(page==='logs');messages.setActive(page==='messages');};
window.show=page=>shell.navigate(page);show('logs');window.ready=true;
</script></body></html>`}));
const settle=()=>page.waitForFunction(()=>!logs.state.reading&&!messages.state.reading&&!messages.state.writing);
const count=command=>page.evaluate(command=>calls.filter(call=>call.command===command).length,command);
try{
 await page.goto(url);await page.waitForFunction(()=>window.ready&&logs.state.loaded);await settle();
 assert.equal(await page.locator('.log-row').count(),500);assert.equal(await page.locator('#log-results pre').count(),0);
 assert(!await page.locator('#log-results').textContent().then(text=>text.includes('secret-structure')));
 await page.locator('[data-log-action="select"][data-id="l499"]').click();assert.match(await page.locator('.log-body').innerText(),/第二行|长正文/);
 await page.locator('.log-fields summary').click();assert.match(await page.locator('.log-fields pre').innerText(),/secret-structure-499/);
 assert.equal(await page.locator('.log-body script').count(),0);
 await page.evaluate(()=>{window.logNode=document.querySelector('[data-log-id="l499"]');return logs.refresh();});assert(await page.evaluate(()=>logNode===document.querySelector('[data-log-id="l499"]')));
 assert(await page.locator('.log-fields').evaluate(n=>n.open));
 await page.locator('#log-results').evaluate(n=>n.scrollTop=3800);await page.waitForFunction(()=>!logs.state.following);
 const anchor=await page.locator('[data-log-id="l100"]').evaluate(n=>n.getBoundingClientRect().top);
 await page.evaluate(()=>{const row=logData.at(-1);logData=[...logData.slice(1),{...row,id:'l500',message:'new record'}];return logs.refresh();});
 assert(Math.abs(await page.locator('[data-log-id="l100"]').evaluate(n=>n.getBoundingClientRect().top)-anchor)<2,'Sliding result retains the record anchor');
 await page.locator('[data-log-action="latest"]').click();
 await page.locator('[data-log-action="copy-one"]').click();assert.equal(JSON.parse(await page.evaluate(()=>copies.at(-1))).id,'l499');
 await page.evaluate(()=>{logData=logData.slice(1);return logs.refresh();});assert.equal(await page.locator('.log-row').count(),499);
 await page.evaluate(()=>{logData=logData.filter(l=>l.id!=='l499');return logs.refresh();});await page.getByText('已移出当前结果，保留所选记录快照。').waitFor();
 await page.evaluate(()=>{hold='read_logs';void logs.refresh();});await page.waitForFunction(()=>held.length===1);
 const readCount=await count('read_logs');await page.evaluate(()=>{for(let i=0;i<20;i++)void logs.refresh();});assert.equal(await count('read_logs'),readCount);
 const frozen=await page.locator('#log-results').innerText();await page.locator('[data-log-action="pause"]').click();
 await page.locator('#log-query').fill('absent');await page.evaluate(()=>{hold='';held.shift().resolve();});await settle();assert.equal(await page.locator('#log-results').innerText(),frozen);
 await page.locator('[data-log-action="clear"]').click();assert.equal(await page.locator('#log-query').inputValue(),'');
 await page.evaluate(()=>show('messages'));await page.waitForFunction(()=>messages.state.loaded);await page.evaluate(()=>show('logs'));
 assert.match(await page.locator('[data-log-action="pause"]').innerText(),/继续/);assert.equal(await page.locator('#log-results').innerText(),frozen);
 await page.locator('[data-log-action="pause"]').click();await settle();
 await page.evaluate(()=>{hold='read_logs';void logs.refresh();});await page.waitForFunction(()=>held.length===1);
 await page.locator('#log-query').fill('absent');await page.waitForTimeout(280);await page.evaluate(()=>{hold='';held.shift().resolve();});await page.waitForFunction(()=>logs.state.records.length===0&&!logs.state.reading);
 await page.locator('[data-log-action="clear"]').click();await settle();
 const beforeComposition=await count('read_logs');await page.locator('#log-query').dispatchEvent('compositionstart');await page.locator('#log-query').fill('多行');await page.evaluate(()=>logs.refresh());await page.waitForTimeout(280);assert.equal(await count('read_logs'),beforeComposition);
 await page.locator('#log-query').dispatchEvent('compositionend');await page.waitForFunction(()=>!logs.state.reading&&logs.state.resultFilter.query==='多行');
 await page.locator('.log-period summary').click();await page.locator('#log-since').fill('2026-11-27T00:00');await page.locator('#log-until').fill('2026-11-26T00:00');await page.getByText('开始时间不能晚于结束时间',{exact:true}).waitFor();
 await page.locator('[data-log-action="clear"]').click();await settle();
 await page.evaluate(()=>{fail='read_logs';return logs.refresh();});await page.getByText(/受控失败 read_logs/).waitFor();assert.equal(await page.locator('.log-row').count(),498);await page.locator('[data-log-action="retry"]').click();await settle();
 for(const fields of [[],false,0,'<b>text</b>',{}]){await page.evaluate(fields=>{logData[0].fields=fields;return logs.refresh();},fields);await page.locator('[data-log-action="select"]').first().click();assert.equal(await page.locator('.log-fields pre').textContent(),JSON.stringify(fields,null,2));}
 await page.evaluate(()=>show('messages'));await page.waitForFunction(()=>messages.state.loaded);assert.equal(await page.locator('.message-row').count(),1000);
 await page.evaluate(()=>{hold='read_messages';void messages.refresh();});await page.waitForFunction(()=>held.length===1);
 const messageReadCount=await count('read_messages');await page.evaluate(()=>{for(let i=0;i<20;i++)void messages.refresh();});assert.equal(await count('read_messages'),messageReadCount);
 await page.evaluate(()=>{messageData[0].title='新内容';show('logs');show('messages');hold='';held.shift().resolve();});await page.waitForFunction(()=>messages.state.messages[0].title==='新内容');
 await page.locator('[data-message-action="unread"]').click();await page.locator('[data-message-action="select"][data-id="m0"]').focus();await page.keyboard.press('Enter');await page.waitForFunction(()=>!messages.state.writing&&messages.state.messages[0].read);
 assert.equal(await page.locator('[data-message-action="select"][data-id="m0"]').count(),0);assert.equal(await page.locator('.message-detail h3').innerText(),'新内容');assert.equal(await page.locator('.message-body svg').count(),0);
 assert(await page.locator('[data-message-action="select"][data-id="m1"]').evaluate(n=>n===document.activeElement));
 await page.evaluate(()=>{messageData[0].read=false;messageData[0].count=2;messageData[0].body='去重更新';return messages.refresh();});assert.match(await page.locator('.message-body').innerText(),/去重更新/);assert.equal(await page.locator('[data-message-action="select"][data-id="m0"]').count(),1);
 await page.evaluate(()=>{hold='mark_read';});await page.locator('[data-message-action="select"][data-id="m0"]').click();await page.waitForFunction(()=>held.length===1);
 await page.locator('[data-message-action="select"][data-id="m1"]').click();assert.equal(await page.locator('.message-detail h3').innerText(),'消息 1');
 await page.evaluate(()=>{hold='';held.shift().resolve();});await settle();assert.equal(await page.locator('.message-detail h3').innerText(),'消息 1');
 await page.evaluate(()=>{fail='mark_read';});await page.locator('[data-message-action="select"][data-id="m2"]').click();await settle();await page.getByText(/已读保存失败/).waitFor();assert.equal(await page.locator('.message-detail h3').innerText(),'消息 2');
 await page.locator('[data-message-action="retry-read"]').click();await settle();assert.equal(await page.locator('[data-message-action="select"][data-id="m2"]').count(),0);
 await page.locator('[data-message-action="all"]').click();await page.locator('[data-message-action="select"][data-id="m3"]').click();const writes=await count('mark_read');await page.locator('[data-message-action="select"][data-id="m3"]').click();assert.equal(await count('mark_read'),writes);
 await page.evaluate(()=>{context={plugins:[],storageErrors:['消息存储损坏，请恢复原文件']};messages.setContext(context);});await page.getByText('来源插件已移除，历史消息保留。').waitFor();assert.equal(await page.locator('[data-message-action="plugin"]').count(),0);assert.match(await page.locator('.activity-storage').innerText(),/存储需要关注/);
 await page.evaluate(()=>{messageData=messageData.filter(m=>m.id!=='m3');return messages.refresh();});await page.getByText('此消息已不在保存范围').waitFor();
 const hiddenCount=await count('read_messages');await page.evaluate(()=>{shell.setWindowVisible(false);return messages.refresh();});assert.equal(await count('read_messages'),hiddenCount);await page.evaluate(()=>shell.setWindowVisible(true));await settle();
 fs.mkdirSync('artifacts/logs-messages',{recursive:true});
 for(const theme of ['light','dark','dopamine'])for(const [width,height]of [[1440,900],[1024,760],[640,700],[320,640]]){
  await page.setViewportSize({width,height});await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;context={plugins:[{manifest:{id:'sodeck.docker',name:'Docker'}}],storageErrors:[]};logs.setContext(context);messages.setContext(context);},theme);
  for(const kind of ['logs','messages']){
   await page.evaluate(kind=>show(kind),kind);await settle();
   const select=page.locator(kind==='logs'?'[data-log-action="select"]':'[data-message-action="select"]').first();if(!await select.isVisible())await page.locator('.activity-back:visible').click();await select.click();
   const detail=page.locator(kind==='logs'?'.log-detail':'.message-detail');assert(await detail.isVisible());
   if(kind==='messages'){
    await page.locator('.message-row.selected').evaluate(async row=>{await Promise.allSettled(row.getAnimations({subtree:true}).map(animation=>animation.finished));});
    const contrast=await page.locator('.message-row.selected').evaluate(row=>{
     const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const ctx=canvas.getContext('2d',{willReadFrequently:true});
     const lum=color=>{ctx.fillStyle=color;ctx.fillRect(0,0,1,1);const c=[...ctx.getImageData(0,0,1,1).data].slice(0,3).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};
     const bg=lum(getComputedStyle(row).backgroundColor);return [...row.querySelectorAll('time,.message-snippet,.message-meta')].map(n=>{const fg=lum(getComputedStyle(n).color);return(Math.max(bg,fg)+.05)/(Math.min(bg,fg)+.05);});
    });assert(contrast.every(value=>value>=4.5),theme+' selected message contrast '+contrast+' '+await page.locator('.message-row.selected').evaluate(row=>JSON.stringify({bg:getComputedStyle(row).backgroundColor,fg:getComputedStyle(row.querySelector('time')).color,theme:document.documentElement.dataset.theme})));
   }
   assert(await page.locator('.activity-page').evaluate(n=>n.scrollWidth<=n.clientWidth+1),kind+' '+theme+' '+width);
   if(width>=640)assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
   assert(await detail.evaluate(n=>n.scrollWidth<=n.clientWidth+1));
   if(width>=1024)await page.screenshot({path:'artifacts/logs-messages/'+kind+'-'+theme+'-'+width+'.png'});
   if(width===320){await page.locator('.activity-back:visible').click();assert(await select.isVisible());}
  }
 }
 await page.evaluate(()=>{for(let i=0;i<20;i++){show('logs');show('messages');}shell.setWindowVisible(false);logs.dispose();messages.dispose();});const finalCalls=await page.evaluate(()=>calls.length);await page.evaluate(()=>Promise.all([logs.refresh(),messages.refresh()]));assert.equal(await page.evaluate(()=>calls.length),finalCalls);
 assert.deepEqual(errors,[]);console.log('Logs/messages: bounded details, stable IDs, fields, single reads, stale responses, pause, IME, time validation, read/write queues, unread focus, dedupe, missing source, storage failure, themes, sizes and disposal passed.');
}catch(error){console.error(errors);console.error(await page.evaluate(()=>({logs:window.logs?.state,messages:window.messages?{...messages.state,messages:messages.state.messages.slice(0,4)}:null,calls:window.calls?.slice(-15)})));throw error;}
finally{await browser.close();await vite.close();}
