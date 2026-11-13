import {connectFileState} from './shared-state.js';
import {openRemoteFile} from './file-editor/open.js';
(() => {
 const addEventListener=(type,fn)=>window.addEventListener(type,e=>{if(type==='sodeck:mount'||!disposed)fn(e);});
 const root=document.querySelector('#root');
 let id,status,tree,transfers,cancelButton,flushTicket=0;
 const request=(method,params)=>sodeck.request(method,params);
 let activated=false,readOnly=false,transferFrozen=false,mountData;
 let settingsSnapshot,settingsEpoch=0,disposed=false,shared;
 async function applySettings(snapshot){
  if(!snapshot||disposed)return;
  if(settingsSnapshot&&settingsSnapshot.definitionToken===snapshot.definitionToken&&BigInt(settingsSnapshot.revision)>BigInt(snapshot.revision))return;
  settingsSnapshot=snapshot;tree?.preferences(snapshot.values);shared?.settings(snapshot);
  if(snapshot.revision)await request('settings',{op:'applied',pageId:snapshot.pageId,definitionToken:snapshot.definitionToken,revision:snapshot.revision}).catch(()=>{});
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
 function theme(tokens){for(const [name,value]of Object.entries(tokens||{}))document.documentElement.style.setProperty('--'+name,value);}
 async function init(m){
  if(m.viewType!=='files'){
   disposed=true;
   const css=document.createElement('link');css.rel='stylesheet';css.href=new URL('./file-editor/editor.css',import.meta.url).href;document.head.append(css);
   const {mountRemoteFileEditor}=await import('./file-editor/index.js');
   mountRemoteFileEditor({root,mount:m,configureDownloads:({path,...bindings})=>new FileTransfers({...bindings,target:()=>path.slice(0,path.lastIndexOf('/'))||'/',refresh:async()=>{}})});return;
  }
  activated=false;readOnly=Boolean(m.readOnly);mountData=m;root.inert=readOnly;
  settingsSnapshot=m.settings;
  id=m.data.fileId;theme(m.tokens);root.replaceChildren();
  status=document.createElement('div');status.className='status';status.setAttribute('role','status');
  const footer=document.createElement('footer');footer.className='sd-edge-top';footer.append(status);

  if(m.viewType==='files'){
   root.className='browser-root';const host=document.createElement('section');root.append(host);
   cancelButton=button('取消下载',()=>data('cancel'));cancelButton.hidden=true;footer.append(cancelButton);root.append(footer);
   transfers=new FileTransfers({fileId:id,data,event,request,message,host:root,target:()=>tree.targetDirectory(),ambiguous:()=>tree.ambiguousTarget(),refresh:()=>tree.refresh()});
   tree=new FileTree(host,{data,event,request,openFile:path=>openRemoteFile({path,mount:m,fileId:id}),download:path=>transfers.download([path]),message,state:m.data.navigation,transfers});tree.preferences(settingsSnapshot?.values||{showHidden:true,density:'compact',fontSize:12});await tree.start();
   shared=connectFileState(tree,m,data,async followTerminal=>{const snapshot=await request('settings',{op:'get'});const result=await request('settings',{op:'save',requestId:crypto.randomUUID(),definitionToken:snapshot.definitionToken,baseRevision:snapshot.revision,values:{...snapshot.values,followTerminal}});await applySettings(result.snapshot,true);});
   void applySettings(settingsSnapshot,true);
  }
  sodeck.mounted(true,m.viewType==='files');
 }
 addEventListener('sodeck:mount',e=>void perform(()=>init(e.detail)));
 addEventListener('sodeck:themeChanged',e=>theme(e.detail.tokens));
 addEventListener('sodeck:settingsChanged',e=>void applySettings(e.detail.settings,true));
 addEventListener('sodeck:textMenuError',e=>message(e.detail,true));
 addEventListener('sodeck:focus',()=>tree?.focus());
 addEventListener('sodeck:visibilityChanged',e=>{shared?.visibility(e.detail.visible);transfers?.visibility(e.detail.visible);if(!e.detail.visible)tree?.closeMenu();});
 addEventListener('sodeck:localFileDrop',e=>tree?.nativeDrop(e.detail));
 addEventListener('sodeck:contextChanged',e=>{mountData.context=e.detail.context;shared?.invalidate();transfers?.invalidate();tree?.contextChanged();message('连接已更换，刷新目录以读取最新文件');});
 addEventListener('sodeck:flush',async e=>{
  const ticket=++flushTicket;
  try{
   if(e.detail.freeze===true){root.inert=true;}
   await tree?.flush();
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=e.detail.freeze===true;root.inert=readOnly||transferFrozen;
   parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id},'*');
  }catch(error){
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=false;root.inert=readOnly;
   parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:e.detail.surfaceId,id:e.detail.id,error:error.message},'*');
  }
 });
 addEventListener('sodeck:surfaceReady',()=>{
  activated=true;readOnly=false;transferFrozen=false;root.inert=false;
  shared?.visibility(mountData?.visible!==false);
 });
 addEventListener('sodeck:transferCancelled',()=>{flushTicket++;transferFrozen=false;root.inert=readOnly;});
 addEventListener('sodeck:unmount',()=>{disposed=true;flushTicket++;settingsEpoch++;shared?.dispose();transfers?.dispose();tree?.dispose();});
 addEventListener('unload',()=>{transfers?.dispose();tree?.dispose();});
})();
