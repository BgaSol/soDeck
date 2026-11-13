/* Standard remote file editor. The core owns IO and document facts. */
import './style.css';
import {loadFont} from '../../../src/font-loader.js';
import {fontPresets,fontAsset} from '../../../plugins/shared/font-presets.js';
import {readDocument} from './read-document.js';
import {createDraftSync} from './draft-sync.js';
let mounted=false;
export function mountRemoteFileEditor({root,mount,configureDownloads}) {
 if(mounted)throw new Error('同一页面只能挂载一个编辑器');
 mounted=true;
 const listeners=[];
 const addEventListener=(type,fn)=>{window.addEventListener(type,fn);listeners.push([type,fn]);};
 let id,codeEditor,status,transfers,cancelButton,eol='\n',bom=false,loaded=false,positionTimer,draft,saveRequest,exportRequest,saveSequence=0n,exportSequence=0n,flushTicket=0;
 const jobWatches=new Map();
 const request=(method,params)=>sodeck.request(method,params);
 let activated=false,readOnly=false,transferFrozen=false,composing=false,mountData;
 let settingsSnapshot,settingsEpoch=0,disposed=false;
 async function applySettings(snapshot,force=false){
  if(!snapshot||disposed)return;
  if(settingsSnapshot&&settingsSnapshot.definitionToken===snapshot.definitionToken&&BigInt(settingsSnapshot.revision)>BigInt(snapshot.revision))return;
  if(!force&&settingsSnapshot?.definitionToken===snapshot.definitionToken&&settingsSnapshot.revision===snapshot.revision)return;
  settingsSnapshot=snapshot;const ticket=++settingsEpoch,v=snapshot.values;
  // Cache preferences while the first document snapshot is still being read.
  if(!loaded)return;
  let error;
  if(codeEditor){
   let family;
   try{family=await loadFont(v.font,fontPresets,fontAsset);}catch(e){error=e.message;try{family=await loadFont({source:'preset',id:'jetbrains-mono'},fontPresets,fontAsset);}catch{family='monospace';}}
   if(disposed||ticket!==settingsEpoch)return;
   codeEditor.preferencesChanged(v,family);
  }
  if(disposed||ticket!==settingsEpoch)return;
  if(snapshot.revision)await request('settings',{op:'applied',pageId:snapshot.pageId,definitionToken:snapshot.definitionToken,revision:snapshot.revision,...(error?{error}:{})}).catch(()=>{});
 }
 const data=async(action,args={})=>{
  const result=await request('data',{op:'files',id,action,...args});
  if(result?.ok===false){const error=new Error(result.error.message);error.code=result.error.code;throw error;}
  return result?.ok===true?result.value:result;
 };
 const event=args=>!activated||readOnly||transferFrozen?(args.action==='navigation'?Promise.resolve({}):Promise.reject(new Error('界面正在交接'))):request('event',args);
 function message(text,error=false){if(status){status.textContent=text;status.classList.toggle('error',error);}if(error)void event({action:'error',message:String(text).slice(0,2048)}).catch(()=>{});}
 function perform(fn){return Promise.resolve().then(fn).catch(e=>message(e.message,true));}
 const size=n=>n==null?'':Number(n)<1024?`${n} B`:`${(Number(n)/1024).toFixed(1)} KiB`;
 function button(text,fn,cls=''){const b=document.createElement('button');b.textContent=text;b.className='sd-control '+cls;b.addEventListener('click',()=>void perform(fn));return b;}
 async function project(){if(loaded)await event({action:'editorState',editRevision:draft.state().revision});}
 async function watchJob(kind,operation){
  clearTimeout(jobWatches.get(kind)?.timer);
  const watch={};jobWatches.set(kind,watch);if(cancelButton)cancelButton.hidden=false;
  const poll=async()=>{
   if(disposed||jobWatches.get(kind)!==watch)return;
   try{
    const state=await data(kind+'.status',{requestId:operation.requestId});
    if(disposed||jobWatches.get(kind)!==watch)return;
    const busy=['running','committing','waitingInput','writing'].includes(state.state);
    void project().catch(()=>{});
    if(busy){message(kind==='export'?'等待导出完成…':state.state==='committing'?'正在确认服务器提交结果…':'正在检查并保存…');watch.timer=setTimeout(poll,300);return;}
    jobWatches.delete(kind);if(cancelButton)cancelButton.hidden=jobWatches.size===0;
    if(['notCommitted','unknown','failed'].includes(state.state)){message(state.error||'操作未完成，草稿保留',true);return;}
    if(kind==='save')message(state.result?.warning||(state.result?.draftRef?.revision===draft.state().reference.revision&&draft.state().revision===draft.state().committedRevision?'已保存到服务器':'已保存先前版本 · 当前内容仍未保存'));
    else message(state.state==='cancelled'?'已取消导出':'草稿已导出：'+state.path);
   }catch(e){if(disposed||jobWatches.get(kind)!==watch)return;message('结果待确认：'+e.message,true);watch.timer=setTimeout(poll,1000);}
  };await poll();
 }
 async function previous(kind,operation){
  if(!operation)return;
  const state=await data(kind+'.status',{requestId:operation.requestId}).catch(()=>accept(kind,operation));
  if(['running','committing','waitingInput','writing'].includes(state.state)){await watchJob(kind,operation);throw new Error('原操作尚未完成，请等待结果');}
  if(state.state==='unknown')throw new Error(state.error||'原保存结果未知，请核对远端；仍可导出草稿');
  await data(kind+'.release',{requestId:operation.requestId});
 }
 async function accept(kind,operation){
  try{return kind==='save'?await event({action:'save',...operation}):await data('export.start',{path:mountData.data.path,...operation});}
  catch(cause){try{return await data(kind+'.status',{requestId:operation.requestId});}catch{throw cause;}}
 }
 async function exportDraft(){
  if(!loaded||codeEditor?.composing)throw new Error('请先完成当前输入');
  await previous('export',exportRequest);exportRequest=undefined;
  await draft.action(async ref=>{exportRequest={requestId:crypto.randomUUID(),sequence:String(++exportSequence),expectedDraftRef:ref};try{await accept('export',exportRequest);}finally{void watchJob('export',exportRequest);}});
 }
 async function download(target){await transfers.download([target]);}
 function encoded(){return codeEditor.bytes();}
 async function flushPosition(){clearTimeout(positionTimer);if(codeEditor&&activated&&!readOnly&&!transferFrozen)await event({action:'position',cursor:codeEditor.selection().active,scroll:Math.round(codeEditor.editor.getScrollTop()),editorState:codeEditor.state()});}
 function position(){clearTimeout(positionTimer);if(activated&&!transferFrozen)positionTimer=setTimeout(()=>void flushPosition().catch(()=>{}),150);}
 function stage(){draft?.change();}
 async function save(){
  if(!loaded)throw new Error('文件尚未准备好编辑');
  if(codeEditor?.composing)throw new Error('请先完成当前输入法组合输入');
  await previous('save',saveRequest);saveRequest=undefined;
  await draft.action(async ref=>{saveRequest={requestId:crypto.randomUUID(),sequence:String(++saveSequence),expectedDraftRef:ref,expectedContext:mountData.context};try{await accept('save',saveRequest);}finally{void watchJob('save',saveRequest);}});
 }
 function theme(tokens){for(const [name,value]of Object.entries(tokens||{}))document.documentElement.style.setProperty('--'+name,value);codeEditor?.theme(tokens||{});}
 async function init(m){
  loaded=false;activated=false;readOnly=Boolean(m.readOnly);mountData=m;root.inert=readOnly;
  settingsSnapshot=m.settings;
  id=m.data.fileId;theme(m.tokens);root.replaceChildren();
  status=document.createElement('div');status.className='status';status.setAttribute('role','status');
  const footer=document.createElement('footer');footer.className='sd-edge-top';footer.append(status);

  {
   root.className='editor-root';const header=document.createElement('header');header.className='editor-toolbar';root.append(header);
   const title=document.createElement('span');title.className='path';title.textContent=m.data.path;title.title=m.data.path;header.append(title,button('保存',save,'primary'),button('搜索',()=>codeEditor?.openSearch()));
   if(configureDownloads)header.append(button('下载远端',()=>download(m.data.path)));
   header.append(button('导出草稿',exportDraft));
   transfers=configureDownloads?.({fileId:id,data,event,request,message,host:root,path:m.data.path});
   cancelButton=button('取消当前操作',async()=>{
    for(const [kind,operation]of [['save',saveRequest],['export',exportRequest]])if(operation)await data(kind+'.cancel',operation);
   });cancelButton.hidden=true;header.append(cancelButton);
   const exportHint=document.createElement('p');exportHint.className='editor-export-hint';exportHint.textContent=(configureDownloads?'下载远端：服务器当前版本 · ':'')+'导出草稿：此页当前内容（含未保存更改）';root.append(exportHint);
   const editorHost=document.createElement('section');editorHost.className='code-editor';editorHost.dataset.ready='false';editorHost.setAttribute('aria-label','远端文件编辑器');root.append(editorHost,footer);message('正在读取文件…');
   const {info,bytes}=await readDocument(data,m.data.path);
   let text;try{text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(bytes);}catch{throw new Error('此文件不是有效 UTF-8 文本，请下载后查看');}
   if(text.includes('\0'))throw new Error('检测到二进制内容，请下载后查看');
   bom=text.startsWith('\uFEFF');if(bom)text=text.slice(1);
   const crlf=(text.match(/\r\n/g)||[]).length,lf=(text.match(/\n/g)||[]).length;
   if(text.replaceAll('\r\n','').includes('\r')||(crlf>0&&crlf!==lf))throw new Error('文件包含混合换行，为避免改变原文，请下载后编辑');
   eol=crlf?'\r\n':'\n';
   const {FileEditor}=await import('./editor.js');if(disposed)return;
   saveSequence=BigInt(info.saveSequence||0);exportSequence=BigInt(info.exportSequence||0);
   draft=createDraftSync({data,read:encoded,info,editRevision:m.data.editRevision||0,changed:()=>{void project().catch(e=>message(e.message,true));},error:e=>message(e.message,true)});
   codeEditor=new FileEditor({host:editorHost,footer,path:m.data.path,text:text.replaceAll('\r\n','\n'),bom,eol,state:m.data.editorState,cursor:m.data.cursor,scroll:m.data.scroll,notify:message,change:stage,position,save:()=>void perform(save)});codeEditor.theme(m.tokens||{});loaded=true;
   await applySettings(settingsSnapshot||{pageId:'editor',values:{font:{source:'preset',id:'jetbrains-mono'},fontSize:14,lineHeight:1.5,wordWrap:false,tabSize:4}},true);
   codeEditor.readOnly(readOnly||!activated);editorHost.dataset.ready='true';
   message('UTF-8'+(bom?' BOM':'')+(eol==='\r\n'?' · CRLF':' · LF')+' · '+size(info.size));
  }
  sodeck.mounted(true,false);
 }
 void perform(()=>init(mount));
 addEventListener('sodeck:themeChanged',e=>theme(e.detail.tokens));
 addEventListener('sodeck:settingsChanged',e=>void applySettings(e.detail.settings,true));
 addEventListener('sodeck:textMenuError',e=>message(e.detail,true));
 addEventListener('sodeck:focus',()=>codeEditor?.focus());
 addEventListener('sodeck:visibilityChanged',e=>{transfers?.visibility(e.detail.visible);codeEditor?.visibility(e.detail.visible);});
 addEventListener('sodeck:contextChanged',e=>{mountData.context=e.detail.context;transfers?.invalidate();message('服务器连接已更换；草稿保留，保存前将重新检查远端版本');});
 addEventListener('sodeck:flush',async e=>{
  const ticket=++flushTicket;
  try{
   if(composing||codeEditor?.composing)throw new Error('请先完成当前输入法组合输入');
   if(e.detail.freeze===true){root.inert=true;codeEditor?.readOnly(true);}
   await draft?.flush();await project();await flushPosition();
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=e.detail.freeze===true;root.inert=readOnly||transferFrozen;codeEditor?.readOnly(readOnly||transferFrozen);
   parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id},'*');
  }catch(error){
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=false;root.inert=readOnly;codeEditor?.readOnly(readOnly);
   parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id,error:error.message},'*');
  }
 });
 addEventListener('sodeck:surfaceReady',()=>{
  activated=true;readOnly=false;transferFrozen=false;root.inert=false;codeEditor?.readOnly(false);
  if(draft)void data('save.status').then(job=>{saveRequest=job.request||undefined;saveSequence=BigInt(job.draft?.saveSequence||saveSequence);if(saveRequest)return watchJob('save',saveRequest);}).catch(error=>message(error.message,true));
 });
 addEventListener('sodeck:transferCancelled',()=>{flushTicket++;transferFrozen=false;root.inert=readOnly;codeEditor?.readOnly(readOnly);});
 function dispose(){if(disposed)return;disposed=true;flushTicket++;settingsEpoch++;draft?.dispose();for(const watch of jobWatches.values())clearTimeout(watch.timer);clearTimeout(positionTimer);codeEditor?.dispose();transfers?.dispose();for(const [type,fn]of listeners)window.removeEventListener(type,fn);mounted=false;}
 addEventListener('sodeck:unmount',dispose);
 addEventListener('unload',dispose);
 return {dispose};
}
