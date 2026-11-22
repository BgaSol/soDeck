/* Surface-owned bounded clipboard transport. No terminal behavior belongs in the host. */
(() => {
  let available=false,surface;
  addEventListener('sodeck:unmount',()=>{available=false;surface=undefined;});
  addEventListener('sodeck:mount',event=>{available=true;surface=event.detail.surfaceId;});
  const interaction=()=>{if(surface)sodeck.interaction();};
  document.addEventListener('pointerdown',interaction,true);
  document.addEventListener('keydown',interaction,true);
  const request = params => available?sodeck.request('clipboard',params):Promise.reject(new Error('页面尚未挂载或已经结束'));
  window.sodeckClipboard = {
    async readText(valid=()=>true) {
      const result=await request({op:'read'});let text='';
      while(text.length<result.length){if(!valid())return null;const block=await request({op:'chunk',id:result.id,offset:text.length});if(!block.text)throw new Error('剪贴板内容不完整');text+=block.text;}
      return valid()?text:null;
    },
    async writeText(text,valid=()=>true) {
      if(new TextEncoder().encode(text).length>256*1024)throw new Error('剪贴板文本超过 256 KiB');
      if(!valid())return false;
      const result=await request({op:'beginWrite',length:text.length});
      for(let offset=0;offset<text.length;offset+=2048){if(!valid())return false;await request({op:'writeChunk',id:result.id,offset,text:text.slice(offset,offset+2048)});}
      if(!valid())return false;
      await request({op:'commitWrite',id:result.id});return true;
    }
  };
  const menu = window.installTextMenu?.({
    read: valid => sodeckClipboard.readText(valid),
    write: (text, valid) => sodeckClipboard.writeText(text, valid),
    notify: error => dispatchEvent(new CustomEvent('sodeck:textMenuError', { detail: error })),
  });
  for (const name of ['mount', 'contextChanged', 'themeChanged', 'visibilityChanged']) addEventListener('sodeck:' + name, () => menu?.close());
  addEventListener('sodeck:unmount', () => {surface=null;menu?.dispose();});
})();
