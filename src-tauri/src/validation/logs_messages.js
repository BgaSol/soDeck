(async () => {
  const deadline=Date.now()+12000,delay=()=>new Promise(resolve=>setTimeout(resolve,30));
  const wait=async read=>{while(Date.now()<deadline){const value=await read();if(value)return value;await delay();}throw Error('native logs/messages UI timeout');};
  const send=payload=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'logs-messages-native-result',payload});
  try {
    document.querySelector('[data-console-page="logs"]').click();
    const query=await wait(()=>document.querySelector('#log-query'));
    query.value='精密工作台原生日志验收';query.dispatchEvent(new Event('input',{bubbles:true}));
    const row=await wait(()=>[...document.querySelectorAll('[data-log-action="select"]')].find(node=>node.textContent==='精密工作台原生日志验收'));
    row.click();
    await wait(()=>document.querySelector('.log-body')?.textContent==='精密工作台原生日志验收');
    document.querySelector('.log-fields summary').click();
    if(!document.querySelector('.log-fields pre')?.textContent.includes('logs-messages'))throw Error('structured fields missing');
    document.querySelector('[data-log-action="pause"]').click();
    if(!document.querySelector('[data-log-action="pause"]').textContent.includes('继续'))throw Error('pause state missing');
    document.querySelector('[data-console-page="messages"]').click();
    const message=await wait(()=>document.getElementById('message-'+__MESSAGE_ID__));
    message.click();
    await wait(()=>document.querySelector('.message-body')?.textContent==='原生正文\n<b>plain text</b>');
    if(document.querySelector('.message-body b'))throw Error('message body executed markup');
    await wait(async()=>{const records=await window.__TAURI_INTERNALS__.invoke('read_messages');return records.find(record=>record.id===__MESSAGE_ID__)?.read;});
    const themes=[];
    for(const theme of ['light','dark','dopamine']){
      await window.__TAURI_INTERNALS__.invoke('set_theme',{theme});await wait(()=>document.documentElement.dataset.theme===theme);
      const panel=document.querySelector('.message-detail');
      if(panel.getBoundingClientRect().width<=0||panel.scrollWidth>panel.clientWidth+1)throw Error('message reader layout overflow');
      themes.push({theme,width:panel.clientWidth,color:getComputedStyle(panel).color,background:getComputedStyle(panel).backgroundColor});
    }
    if(new Set(themes.map(theme=>theme.background)).size!==3)throw Error('themes did not reach the reader');
    document.querySelector('[data-console-page="logs"]').click();
    await wait(()=>document.querySelector('[data-log-action="pause"]')?.textContent.includes('继续'));
    if(document.querySelector('#log-query').value!=='精密工作台原生日志验收')throw Error('filter lost across navigation');
    await window.__TAURI_INTERNALS__.invoke('set_theme',{theme:'system'});
    await send({ok:true,logs:true,fields:true,pause:true,messages:true,markRead:true,plainText:true,themes});
  }catch(error){await send({ok:false,error:String(error)});}
})();
