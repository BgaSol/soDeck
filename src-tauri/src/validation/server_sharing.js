(async()=>{
  const invoke=window.__TAURI_INTERNALS__.invoke;
  const send=payload=>invoke('plugin:event|emit',{event:'server-sharing-native-result',payload});
  const deadline=Date.now()+15000;
  const wait=async read=>{while(Date.now()<deadline){const value=await read();if(value)return value;await new Promise(r=>setTimeout(r,30));}throw Error('sharing UI timeout');};
  let original;
  try{
    original=await invoke('clipboard_text',{operation:'read',purpose:'serverShare'});
    document.querySelector('[data-console-page="connections"]').click();
    const row=await wait(()=>document.querySelector('[data-server-action="select"][data-id='+JSON.stringify(__SERVER_ID__)+']'));
    row.click();
    const share=await wait(()=>document.querySelector('.catalog-detail [data-server-command]')&&[...document.querySelectorAll('.catalog-detail [data-server-command]')].find(b=>JSON.parse(b.dataset.serverCommand).kind==='share'));
    share.click();
    const copy=await wait(()=>document.querySelector('dialog[open] [data-share-action="copy"]:not([disabled])'));
    const code=document.querySelector('dialog[open] textarea').value;
    copy.click();await wait(()=>document.querySelector('[data-share-action="copy"]')?.textContent==='已复制');
    if(await invoke('clipboard_text',{operation:'read',purpose:'serverShare'})!==code)throw Error('share button clipboard mismatch');
    document.querySelector('[data-share-action="close"]').click();
    await invoke('clipboard_text',{operation:'write',purpose:'serverShare',text:''});
    const copyAll=document.querySelector('[data-server-action="copy-all"]');
    if(!copyAll||copyAll.disabled)throw Error('copy all toolbar button unavailable');
    copyAll.click();
    const allCode=await wait(async()=>await invoke('clipboard_text',{operation:'read',purpose:'serverShare'}));
    const catalog=await invoke('server_catalog');
    const summary=await invoke('preview_server_share',{code:allCode});
    if(summary.targetCount!==catalog.servers.length)throw Error('copy all omitted servers');
    if(document.querySelector('dialog[open],.catalog-action-menu'))throw Error('copy all opened an extra step');
    document.querySelector('[data-server-action="import-share"]').click();
    const field=await wait(()=>document.querySelector('dialog[open] [data-server-share-code]'));
    const large=' '.repeat(300000)+allCode;
    await invoke('clipboard_text',{operation:'write',purpose:'serverShare',text:large});
    field.focus();field.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:field.getBoundingClientRect().left+20,clientY:field.getBoundingClientRect().top+20}));
    const paste=await wait(()=>[...document.querySelectorAll('.sd-text-menu button')].find(b=>b.textContent.startsWith('粘贴')));paste.click();
    await wait(()=>field.value===large);
    await wait(()=>document.querySelector('[data-share-action="import"]:not([disabled])'));
    document.querySelector('[data-share-action="close"]').click();
    await invoke('clipboard_text',{operation:'write',purpose:'serverShare',text:original});
    await send({ok:true,export:true,managedKey:true,reopen:true,copy:true,textMenuPaste:true,over256KiB:true});
  }catch(error){
    let restoreError;
    if(original!==undefined)try{await invoke('clipboard_text',{operation:'write',purpose:'serverShare',text:original});}catch(e){restoreError=String(e);}
    await send({ok:false,error:String(error),restoreError});
  }
})();
