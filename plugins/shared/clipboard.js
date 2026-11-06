/* UI v1 bounded clipboard transport. No terminal behavior belongs in the host. */
(() => {
  let available=false,surface;
  addEventListener('sodeck:mount',event=>{available=event.detail.capabilities?.clipboard===1;surface=event.detail.surfaceId;});
  const interaction=()=>{if(surface)parent.postMessage({channel:'sodeck-ui-v1',type:'interaction',surfaceId:surface},'*');};
  document.addEventListener('pointerdown',interaction,true);
  document.addEventListener('keydown',interaction,true);
  const request = params => available?sodeck.request('clipboard',params):Promise.reject(new Error('当前核心不支持原生剪贴板服务，请先升级核心'));
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
