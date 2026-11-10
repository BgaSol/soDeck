/* File business UI. Remote/local IO stays behind the core's scoped data endpoint. */
import {loadFont} from './font-loader.js';
import {fontPresets,fontAsset} from './font-presets.js';
import {readDocument} from './read-document.js';
import {connectFileState} from './shared-state.js';
(() => {
 const root=document.querySelector('#root');
 let id,codeEditor,status,tree,transfers,cancelButton,jobTimer,jobId,busy=false,queue=Promise.resolve(),dirtyQueue=Promise.resolve(),localRevision=0,eol='\n',bom=false,loaded=false,pumping=false,pendingDraft=false,positionTimer,stagedRevision=0,stagedLocalRevision=0;
 const request=(method,params)=>sodeck.request(method,params);
 let activated=false,readOnly=false,transferFrozen=false,composing=false,mountData;
 let settingsSnapshot,settingsEpoch=0,disposed=false,shared;
 async function applySettings(snapshot,force=false){
  if(!snapshot||disposed)return;
  if(settingsSnapshot&&settingsSnapshot.definitionToken===snapshot.definitionToken&&BigInt(settingsSnapshot.revision)>BigInt(snapshot.revision))return;
  if(!force&&settingsSnapshot?.definitionToken===snapshot.definitionToken&&settingsSnapshot.revision===snapshot.revision)return;
  settingsSnapshot=snapshot;const ticket=++settingsEpoch,v=snapshot.values;
  // Cache preferences while the first document snapshot is still being read.
  if(snapshot.pageId==='editor'&&!loaded)return;
  let error;
  if(snapshot.pageId==='browser'){tree?.preferences(v);shared?.settings(snapshot);}
  if(snapshot.pageId==='editor'&&codeEditor){
   let family;
   try{family=await loadFont(v.font,fontPresets,fontAsset);}catch(e){error=e.message;try{family=await loadFont({source:'preset',id:'jetbrains-mono'},fontPresets,fontAsset);}catch{family='monospace';}}
   if(disposed||ticket!==settingsEpoch)return;
   codeEditor.preferencesChanged(v,family);
  }
  if(disposed||ticket!==settingsEpoch)return;
  if(snapshot.revision)await request('settings',{op:'applied',pageId:snapshot.pageId,definitionToken:snapshot.definitionToken,revision:snapshot.revision,...(error?{error}:{})}).catch(()=>{});
 }
 const data=(action,args={})=>request('data',{op:'files',id,action,...args});
 const event=args=>!activated||readOnly||transferFrozen?(args.action==='navigation'?Promise.resolve({}):Promise.reject(new Error('界面正在交接'))):request('event',args);
 function message(text,error=false){if(status){status.textContent=text;status.classList.toggle('error',error);}if(error)void event({action:'error',message:String(text).slice(0,2048)}).catch(()=>{});}
 function perform(fn){return Promise.resolve().then(fn).catch(e=>message(e.message,true));}
 const size=n=>n==null?'':Number(n)<1024?`${n} B`:`${(Number(n)/1024).toFixed(1)} KiB`;
 function button(text,fn,cls=''){const b=document.createElement('button');b.textContent=text;b.className='sd-control '+cls;b.addEventListener('click',()=>void perform(fn));return b;}
 async function watchJob(start){
  if(start.cancelled)return;
  jobId=start.id;busy=true;if(cancelButton)cancelButton.hidden=false;await event({action:'busy',busy:true});clearTimeout(jobTimer);
  const poll=async()=>{
   try{const state=await data('status');if(state.id!==jobId)return;
    if(state.state==='running'){message(state.kind==='download'?`下载中 ${size(state.received)} / ${size(state.total)}`:'正在检查并保存…');jobTimer=setTimeout(poll,300);return;}
    busy=false;if(cancelButton)cancelButton.hidden=true;await event({action:'busy',busy:false});
    if(state.state==='failed'){message(state.error,true);return;}
    if(state.result?.revision!==undefined){await event({action:'saved',revision:state.result.revision});message(pendingDraft||pumping||localRevision!==stagedLocalRevision||state.result.revision!==stagedRevision?'已保存先前版本 · 当前内容仍未保存':'已保存到服务器');}
    else message('下载完成：'+state.result.path);
   }catch(e){busy=false;if(cancelButton)cancelButton.hidden=true;message(e.message,true);}
  };await poll();
 }
 async function download(target){await transfers.download([target]);}
 function encoded(){return codeEditor.bytes();}
 async function flushPosition(){clearTimeout(positionTimer);if(codeEditor&&activated&&!readOnly&&!transferFrozen)await event({action:'position',cursor:codeEditor.selection().active,scroll:Math.round(codeEditor.editor.getScrollTop()),editorState:codeEditor.state()});}
 function position(){clearTimeout(positionTimer);if(activated&&!transferFrozen)positionTimer=setTimeout(()=>void flushPosition().catch(()=>{}),150);}
 function stage(){
  const changed=++localRevision;pendingDraft=true;dirtyQueue=dirtyQueue.catch(()=>{}).then(()=>event({action:'dirty',editRevision:changed}));dirtyQueue.catch(e=>message(e.message,true));if(pumping)return;pumping=true;
  // At most one in-flight snapshot; newer input is coalesced, not queued as full documents.
  queue=Promise.resolve().then(async()=>{while(pendingDraft){
   pendingDraft=false;
   const revision=localRevision,bytes=encoded();
   await dirtyQueue;
   if(bytes.length>1048576)throw new Error('草稿超过 1 MiB，无法保存；请缩小内容');
   let staged;
   for(let offset=0;offset<bytes.length||offset===0;offset+=16384){staged=await data('stage',{offset,begin:offset===0,done:offset+16384>=bytes.length,data:Array.from(bytes.subarray(offset,offset+16384))});}
   stagedRevision=staged.revision;stagedLocalRevision=revision;await event({action:'staged',revision:staged.revision,editRevision:revision});await flushPosition();
   if(revision===localRevision)message('未保存 · UTF-8'+(bom?' BOM':'')+(eol==='\r\n'?' · CRLF':' · LF'));
  }}).finally(()=>{pumping=false;});
  queue.catch(e=>message(e.message,true));
 }
 async function save(){if(!loaded)throw new Error('文件尚未准备好编辑');if(codeEditor?.composing)throw new Error('请先完成当前输入法组合输入');await queue;if(busy)throw new Error('保存或下载正在进行');await watchJob(await event({action:'save'}));}
 function theme(tokens){for(const [name,value]of Object.entries(tokens||{}))document.documentElement.style.setProperty('--'+name,value);codeEditor?.theme(tokens||{});}
 async function init(m){
  loaded=false;activated=false;readOnly=Boolean(m.readOnly);mountData=m;root.inert=readOnly;
  settingsSnapshot=m.settings;
  id=m.data.fileId;theme(m.tokens);root.replaceChildren();
  status=document.createElement('div');status.className='status';status.setAttribute('role','status');
  const footer=document.createElement('footer');footer.className='sd-edge-top';footer.append(status);

  if(m.viewType==='files'){
   root.className='browser-root';const host=document.createElement('section');root.append(host);
   cancelButton=button('取消下载',()=>data('cancel'));cancelButton.hidden=true;footer.append(cancelButton);root.append(footer);
   transfers=new FileTransfers({fileId:id,data,event,request,message,host:root,target:()=>tree.targetDirectory(),ambiguous:()=>tree.ambiguousTarget(),refresh:()=>tree.refresh()});
   tree=new FileTree(host,{data,event,request,download:path=>transfers.download([path]),message,state:m.data.navigation,transfers});tree.preferences(settingsSnapshot?.values||{showHidden:true,density:'compact',fontSize:12});await tree.start();
   shared=connectFileState(tree,m,data,async followTerminal=>{const snapshot=await request('settings',{op:'get'});const result=await request('settings',{op:'save',requestId:crypto.randomUUID(),definitionToken:snapshot.definitionToken,baseRevision:snapshot.revision,values:{...snapshot.values,followTerminal}});await applySettings(result.snapshot,true);});
   void applySettings(settingsSnapshot,true);
  }else{
   root.className='editor-root';const header=document.createElement('header');header.className='editor-toolbar';root.append(header);
   const title=document.createElement('span');title.className='path';title.textContent=m.data.path;title.title=m.data.path;header.append(title,button('保存',save,'primary'),button('搜索',()=>codeEditor?.openSearch()),button('下载远端',()=>download(m.data.path)),button('导出草稿',async()=>{if(codeEditor?.composing)throw new Error('请先完成当前输入法组合输入');await queue;const result=await data('exportDraft',{path:m.data.path});if(!result.cancelled)message('草稿已导出：'+result.path);}));
   transfers=new FileTransfers({fileId:id,data,event,request,message,host:root,target:()=>m.data.path.slice(0,m.data.path.lastIndexOf('/'))||'/',refresh:async()=>{}});
   const exportHint=document.createElement('p');exportHint.className='editor-export-hint';exportHint.textContent='下载远端：服务器当前版本 · 导出草稿：此页当前内容（含未保存更改）';root.append(exportHint);
   const editorHost=document.createElement('section');editorHost.className='code-editor';editorHost.dataset.ready='false';editorHost.setAttribute('aria-label','远端文件编辑器');root.append(editorHost,footer);message('正在读取文件…');
   const {info,bytes}=await readDocument(data,m.data.path);
   let text;try{text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(bytes);}catch{throw new Error('此文件不是有效 UTF-8 文本，请下载后查看');}
   if(text.includes('\0'))throw new Error('检测到二进制内容，请下载后查看');
   bom=text.startsWith('\uFEFF');if(bom)text=text.slice(1);
   const crlf=(text.match(/\r\n/g)||[]).length,lf=(text.match(/\n/g)||[]).length;
   if(text.replaceAll('\r\n','').includes('\r')||(crlf>0&&crlf!==lf))throw new Error('文件包含混合换行，为避免改变原文，请下载后编辑');
   eol=crlf?'\r\n':'\n';
   const css=document.createElement('link');css.rel='stylesheet';css.href=new URL('./vendor/editor.css',import.meta.url).href;document.head.append(css);
   const {FileEditor}=await import('./vendor/editor.js');if(disposed)return;
   localRevision=m.data.editRevision||0;stagedLocalRevision=localRevision;stagedRevision=info.revision;
   codeEditor=new FileEditor({host:editorHost,footer,path:m.data.path,text:text.replaceAll('\r\n','\n'),bom,eol,state:m.data.editorState,cursor:m.data.cursor,scroll:m.data.scroll,notify:message,change:stage,position,save:()=>void perform(save)});codeEditor.theme(m.tokens||{});loaded=true;
   await applySettings(settingsSnapshot||{pageId:'editor',values:{font:{source:'preset',id:'jetbrains-mono'},fontSize:14,lineHeight:1.5,wordWrap:false,tabSize:4}},true);
   codeEditor.readOnly(readOnly||!activated);editorHost.dataset.ready='true';
   message('UTF-8'+(bom?' BOM':'')+(eol==='\r\n'?' · CRLF':' · LF')+' · '+size(info.size));
  }
  sodeck.mounted(true,m.viewType==='files');
 }
 addEventListener('sodeck:mount',e=>void perform(()=>init(e.detail)));
 addEventListener('sodeck:themeChanged',e=>theme(e.detail.tokens));
 addEventListener('sodeck:settingsChanged',e=>void applySettings(e.detail.settings,true));
 addEventListener('sodeck:textMenuError',e=>message(e.detail,true));
 addEventListener('sodeck:focus',()=>tree?tree.focus():codeEditor?.focus());
 addEventListener('sodeck:visibilityChanged',e=>{shared?.visibility(e.detail.visible);transfers?.visibility(e.detail.visible);codeEditor?.visibility(e.detail.visible);if(!e.detail.visible)tree?.closeMenu();});
 addEventListener('sodeck:localFileDrop',e=>tree?.nativeDrop(e.detail));
 addEventListener('sodeck:contextChanged',()=>{shared?.invalidate();transfers?.invalidate();tree?.contextChanged();message(tree?'连接已更换，刷新目录以读取最新文件':'服务器连接已更换；草稿保留，保存前将重新检查远端版本');});
 addEventListener('sodeck:flush',async e=>{try{if(composing||codeEditor?.composing)throw new Error('请先完成当前输入法组合输入');if(e.detail.freeze===true){root.inert=true;codeEditor?.readOnly(true);}await queue;await dirtyQueue;await flushPosition();await tree?.flush();transferFrozen=e.detail.freeze===true;root.inert=readOnly||transferFrozen;codeEditor?.readOnly(readOnly||transferFrozen);parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id},'*');}catch(error){transferFrozen=false;root.inert=readOnly;codeEditor?.readOnly(readOnly);parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id,error:error.message},'*');}});
 addEventListener('sodeck:surfaceReady',()=>{activated=true;readOnly=false;transferFrozen=false;root.inert=false;void data('status').then(job=>{if(job.state==='running')return watchJob(job);}).catch(error=>message(error.message,true));codeEditor?.readOnly(false);shared?.visibility(mountData?.visible!==false);});
 addEventListener('sodeck:transferCancelled',()=>{transferFrozen=false;root.inert=readOnly;codeEditor?.readOnly(readOnly);});
 addEventListener('sodeck:unmount',()=>{disposed=true;settingsEpoch++;clearTimeout(positionTimer);codeEditor?.dispose();shared?.dispose();transfers?.dispose();tree?.dispose();});
 addEventListener('unload',()=>{clearTimeout(jobTimer);clearTimeout(positionTimer);codeEditor?.dispose();transfers?.dispose();tree?.dispose();});
})();
