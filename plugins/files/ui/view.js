/* File business UI. Remote/local IO stays behind the core's scoped data endpoint. */
(() => {
 const root=document.querySelector('#root'),encoder=new TextEncoder();
 let id,textarea,status,tree,cancelButton,jobTimer,jobId,busy=false,queue=Promise.resolve(),localRevision=0,eol='\n',bom=false,loaded=false,pumping=false,pendingDraft=false;
 const request=(method,params)=>sodeck.request(method,params);
 const data=(action,args={})=>request('data',{op:'files',id,action,...args});
 const event=args=>request('event',args);
 function message(text,error=false){if(status){status.textContent=text;status.classList.toggle('error',error);}if(error)void event({action:'error',message:String(text).slice(0,2048)}).catch(()=>{});}
 function perform(fn){return Promise.resolve().then(fn).catch(e=>message(e.message,true));}
 const size=n=>n==null?'':Number(n)<1024?`${n} B`:`${(Number(n)/1024).toFixed(1)} KiB`;
 function button(text,fn,cls=''){const b=document.createElement('button');b.textContent=text;b.className=cls;b.addEventListener('click',()=>void perform(fn));return b;}
 async function watchJob(start){
  if(start.cancelled)return;
  jobId=start.id;busy=true;if(cancelButton)cancelButton.hidden=false;await event({action:'busy',busy:true});clearTimeout(jobTimer);
  const poll=async()=>{
   try{const state=await data('status');if(state.id!==jobId)return;
    if(state.state==='running'){message(state.kind==='download'?`下载中 ${size(state.received)} / ${size(state.total)}`:'正在检查并保存…');jobTimer=setTimeout(poll,300);return;}
    busy=false;if(cancelButton)cancelButton.hidden=true;await event({action:'busy',busy:false});
    if(state.state==='failed'){message(state.error,true);return;}
    if(state.result?.revision!==undefined){await event({action:'saved',revision:state.result.revision});message('已保存到服务器');}
    else message('下载完成：'+state.result.path);
   }catch(e){busy=false;if(cancelButton)cancelButton.hidden=true;message(e.message,true);}
  };await poll();
 }
 async function download(target){if(busy)throw new Error('请先等待当前文件任务结束');await watchJob(await data('download',{path:target}));}
 function encoded(){let text=textarea.value;if(eol==='\r\n')text=text.replaceAll('\n','\r\n');return encoder.encode((bom?'\uFEFF':'')+text);}
 function stage(){
  ++localRevision;pendingDraft=true;if(pumping)return;pumping=true;
  // At most one in-flight snapshot; newer input is coalesced, not queued as full documents.
  queue=Promise.resolve().then(async()=>{while(pendingDraft){
   pendingDraft=false;
   const revision=localRevision,bytes=encoded(),cursor=textarea.selectionStart,scroll=textarea.scrollTop;
   await event({action:'dirty'});
   if(bytes.length>1048576)throw new Error('草稿超过 1 MiB，无法保存；请缩小内容');
   let staged;
   for(let offset=0;offset<bytes.length||offset===0;offset+=16384){staged=await data('stage',{offset,begin:offset===0,done:offset+16384>=bytes.length,data:Array.from(bytes.subarray(offset,offset+16384))});}
   await event({action:'staged',revision:staged.revision});await event({action:'position',cursor,scroll});
   if(revision===localRevision)message('未保存 · UTF-8'+(bom?' BOM':'')+(eol==='\r\n'?' · CRLF':' · LF'));
  }}).finally(()=>{pumping=false;});
  queue.catch(e=>message(e.message,true));
 }
 async function save(){if(!loaded)throw new Error('文件尚未准备好编辑');await queue;if(busy)throw new Error('保存或下载正在进行');await watchJob(await data('save'));}
 function theme(tokens){for(const [name,value]of Object.entries(tokens||{}))document.documentElement.style.setProperty('--'+name,value);}
 async function init(m){
  id=m.data.fileId;theme(m.tokens);root.replaceChildren();
  status=document.createElement('div');status.className='status';status.setAttribute('role','status');
  const footer=document.createElement('footer');footer.append(status);
  // Ready precedes remote operations; describe/chunk may also be used by an incoming window.
  sodeck.mounted(true);
  if(m.viewType==='files'){
   root.className='browser-root';const host=document.createElement('section');root.append(host);
   cancelButton=button('取消下载',()=>data('cancel'));cancelButton.hidden=true;footer.append(cancelButton);root.append(footer);
   tree=new FileTree(host,{data,event,request,download,message,state:m.data.navigation});await tree.start();
  }else{
   root.className='editor-root';const header=document.createElement('header');header.className='editor-toolbar';root.append(header);
   const title=document.createElement('span');title.className='path';title.textContent=m.data.path;title.title=m.data.path;header.append(title,button('保存',save,'primary'),button('下载远端',()=>download(m.data.path)),button('导出草稿',async()=>{await queue;const result=await data('exportDraft',{path:m.data.path});if(!result.cancelled)message('草稿已导出：'+result.path);}));
   textarea=document.createElement('textarea');textarea.spellcheck=false;textarea.disabled=true;textarea.setAttribute('aria-label','远端文件编辑器');root.append(textarea,footer);
   let info=await data('describe');if(!info.path)info=await data('read',{path:m.data.path});
   const bytes=new Uint8Array(info.size);for(let offset=0;offset<bytes.length;offset+=16384){const block=await data('chunk',{offset});bytes.set(block.data,offset);}
   let text;try{text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(bytes);}catch{throw new Error('此文件不是有效 UTF-8 文本，请下载后查看');}
   if(text.includes('\0'))throw new Error('检测到二进制内容，请下载后查看');
   bom=text.startsWith('\uFEFF');if(bom)text=text.slice(1);
   const crlf=(text.match(/\r\n/g)||[]).length,lf=(text.match(/\n/g)||[]).length;
   if(text.replaceAll('\r\n','').includes('\r')||(crlf>0&&crlf!==lf))throw new Error('文件包含混合换行，为避免改变原文，请下载后编辑');
   eol=crlf?'\r\n':'\n';textarea.value=text.replaceAll('\r\n','\n');textarea.disabled=false;loaded=true;
   textarea.setSelectionRange(m.data.cursor||0,m.data.cursor||0);textarea.scrollTop=m.data.scroll||0;
   textarea.addEventListener('input',stage);textarea.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='s'){e.preventDefault();void perform(save);}});
   textarea.addEventListener('select',()=>void event({action:'position',cursor:textarea.selectionStart,scroll:textarea.scrollTop}).catch(()=>{}));
   message('UTF-8'+(bom?' BOM':'')+(eol==='\r\n'?' · CRLF':' · LF')+' · '+size(info.size));
  }
  const job=await data('status');if(job.state==='running')await watchJob(job);
 }
 addEventListener('sodeck:mount',e=>void perform(()=>init(e.detail)));
 addEventListener('sodeck:themeChanged',e=>theme(e.detail.tokens));
 addEventListener('sodeck:focus',()=>tree?tree.focus():textarea?.focus());
 addEventListener('sodeck:visibilityChanged',e=>{if(!e.detail.visible)tree?.closeMenu();});
 addEventListener('sodeck:contextChanged',()=>{tree?.contextChanged();message(tree?'连接已更换，刷新目录以读取最新文件':'服务器连接已更换；草稿保留，保存前将重新检查远端版本');});
 addEventListener('sodeck:flush',async e=>{try{await queue;await tree?.flush();parent.postMessage({channel:'sodeck-ui-v1',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id},'*');}catch(error){parent.postMessage({channel:'sodeck-ui-v1',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id,error:error.message},'*');}});
 addEventListener('unload',()=>clearTimeout(jobTimer));
})();
