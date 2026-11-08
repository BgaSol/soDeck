(() => {
  const root=document.querySelector('#view'),error=document.querySelector('#error');let current;
  const fail=e=>{error.hidden=false;error.textContent=String(e);};
  const theme=t=>Object.entries(t||{}).forEach(([k,v])=>document.documentElement.style.setProperty('--'+k,v));
  const request=(method,params)=>sodeck.request(method,params).catch(fail);
  function remember() { const input=root.querySelector('textarea');if(input)request('event',{action:'edit',text:input.value,cursor:input.selectionStart,scroll:Math.round(input.scrollTop)}); }
  function position() { const input=root.querySelector('textarea');if(input)request('event',{action:'position',cursor:input.selectionStart,scroll:Math.round(input.scrollTop)}); }
  addEventListener('sodeck:mount',event=>{
    current=event.detail;theme(current.tokens);root.replaceChildren();
    const info=document.createElement('div');info.className='context';info.textContent=`测试夹具 · ${current.serverName||'服务器工作区'}`;root.append(info);
    if(current.viewType==='files') {
      const path=document.createElement('div');path.className='path';path.textContent='/etc/nginx';root.append(path);
      for(const name of ['nginx.conf','mime.types']) {
        const button=document.createElement('button');button.className='file';button.textContent=name;button.dataset.file=name;root.append(button);
        const open=()=>request('openView',{viewType:'editor',resourceKey:'/etc/nginx/'+name,params:{name}});
        button.addEventListener('dblclick',open);button.addEventListener('keydown',e=>{if(e.key==='Enter')open();});
      }
    } else if(current.viewType==='forwarding') {
      const p=document.createElement('p');p.className='empty';p.textContent='暂无转发规则。展开此面板不会创建监听端口。';root.append(p);
    } else {
      const toolbar=document.createElement('header'),label=document.createElement('span');label.textContent=current.data.title;toolbar.append(label);root.append(toolbar);
      if(current.viewType==='editor') { const save=document.createElement('button');save.textContent='保存';save.addEventListener('click',()=>request('event',{action:'save'}));toolbar.append(save); }
      else { const note=document.createElement('p');note.className='note';note.textContent='终端输入草稿夹具，此处不会执行命令。';root.append(note); }
      const input=document.createElement('textarea');input.spellcheck=false;input.maxLength=6000;input.setAttribute('aria-label',current.viewType==='editor'?'文件编辑草稿':'终端输入草稿');input.value=current.data.text;root.append(input);
      input.setSelectionRange(current.data.cursor||0,current.data.cursor||0);requestAnimationFrame(()=>input.scrollTop=current.data.scroll||0);
      input.addEventListener('input',remember);input.addEventListener('select',position);input.addEventListener('scroll',position,{passive:true});
      input.addEventListener('keydown',event=>{if((event.ctrlKey||event.metaKey)&&event.key==='s'){event.preventDefault();request('event',{action:'save'});}});
    }
    sodeck.mounted();
  });
  addEventListener('sodeck:themeChanged',event=>theme(event.detail.tokens));
  addEventListener('sodeck:flush',async event=>{
    try {
      const input=root.querySelector('textarea');
      if(input)await sodeck.request('event',{action:'edit',text:input.value,cursor:input.selectionStart,scroll:Math.round(input.scrollTop)});
      sodeck.flushed(event.detail.id);
    } catch(error) {sodeck.flushed(event.detail.id,String(error));}
  });
  addEventListener('sodeck:focus',()=>root.querySelector('textarea,button')?.focus());
})();
