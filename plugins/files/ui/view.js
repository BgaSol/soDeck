import {SharedState} from './sdk/state.js';
import {connectDirectoryFollow} from './directory-follow.js';
import {connectMounts} from './mounts.js';
import {openRemoteFile} from './file-editor/open.js';
(() => {
 const addEventListener=(type,fn)=>window.addEventListener(type,e=>{if(type==='sodeck:mount'||!disposed)fn(e);});
 const root=document.querySelector('#root');
 let id,status,tree,transfers,flushTicket=0;
 const request=(method,params)=>sodeck.request(method,params);
 let activated=false,readOnly=false,transferFrozen=false,mountData;
 let settingsSnapshot,disposed=false,following,mounts,persistTimer,pendingNavigation,persistJob;
 async function applySettings(snapshot){
  if(!snapshot||disposed)return;
  if(settingsSnapshot&&settingsSnapshot.definitionToken===snapshot.definitionToken&&BigInt(settingsSnapshot.revision)>BigInt(snapshot.revision))return;
  settingsSnapshot=snapshot;tree?.preferences(snapshot.values);following?.settings(snapshot);
  if(snapshot.revision)await request('settings',{op:'applied',pageId:snapshot.pageId,definitionToken:snapshot.definitionToken,revision:snapshot.revision}).catch(()=>{});
 }
 const data=async(action,args={})=>{
  const result=await request('data',{op:'files',id,action,...args});
  return result;
 };
 const event=args=>!activated||readOnly||transferFrozen?(args.action==='navigation'?Promise.resolve({}):Promise.reject(new Error('界面正在交接'))):request('event',args);
 function message(text,error=false){if(status){SoDeckMotion.setMotionText(status,text);status.classList.toggle('error',error);}if(error)void event({action:'error',message:String(text).slice(0,2048)}).catch(()=>{});}
 function perform(fn){return Promise.resolve().then(fn).catch(e=>message(e.message,true));}
 function theme(tokens){for(const [name,value]of Object.entries(tokens||{}))document.documentElement.style.setProperty('--'+name,value);}
 function scheduleNavigation(){if(!activated||readOnly||transferFrozen||disposed)return;pendingNavigation=true;clearTimeout(persistTimer);persistTimer=setTimeout(()=>void saveNavigation().catch(e=>message(e.message,true)),100);}
 async function saveNavigation(){
  clearTimeout(persistTimer);
  if(persistJob){await persistJob;if(!pendingNavigation)return;}
  persistJob=(async()=>{while(pendingNavigation&&!disposed){pendingNavigation=false;const state=tree.snapshot();if(following)state.follow=following.snapshot();await request('event',{action:'navigation',state});}})();
  try{await persistJob;}finally{persistJob=null;}
 }
 function controls(){
  following?.dispose();mounts?.dispose();following=mounts=null;
  if(!tree||!activated||readOnly||mountData.connected===false)return;
  const m={...mountData,settings:settingsSnapshot};
  following=connectDirectoryFollow(tree,m,{client:new SharedState(m),data,changed:scheduleNavigation,save:async followTerminal=>{const snapshot=await request('settings',{op:'get'});const result=await request('settings',{op:'save',requestId:crypto.randomUUID(),definitionToken:snapshot.definitionToken,baseRevision:snapshot.revision,values:{...snapshot.values,followTerminal}});await applySettings(result.snapshot);}});
  mounts=connectMounts(tree,m,{client:new SharedState(m),data});
  following.lifecycle({active:true,visible:m.visible!==false,frozen:transferFrozen});
 }
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
   root.append(footer);
   transfers=new FileTransfers({fileId:id,data,event,request,message,host:root,target:()=>tree.targetDirectory(),ambiguous:()=>tree.ambiguousTarget(),refresh:()=>tree.refresh()});
   tree=new FileTree(host,{data,event,request,openFile:path=>openRemoteFile({path,mount:m,fileId:id}),download:path=>transfers.download([path]),message,state:m.data.navigation,transfers});tree.preferences(settingsSnapshot?.values||{showHidden:true,density:'compact',fontSize:12});await tree.start();
   tree.onNavigationChanged=scheduleNavigation;
   void applySettings(settingsSnapshot,true);
  }
  sodeck.prepared(true,m.viewType==='files');
 }
 addEventListener('sodeck:mount',e=>void perform(()=>init(e.detail)));
 addEventListener('sodeck:themeChanged',e=>theme(e.detail.tokens));
 addEventListener('sodeck:settingsChanged',e=>void applySettings(e.detail.settings,true));
 addEventListener('sodeck:textMenuError',e=>message(e.detail,true));
 addEventListener('sodeck:focus',()=>tree?.focus());
 addEventListener('sodeck:visibilityChanged',e=>{mountData.visible=e.detail.visible;following?.lifecycle({visible:e.detail.visible});mounts?.visibility(e.detail.visible);transfers?.visibility(e.detail.visible);if(!e.detail.visible)tree?.closeMenu();});
 addEventListener('sodeck:localFileDrop',e=>tree?.nativeDrop(e.detail));
 addEventListener('sodeck:contextChanged',e=>{mountData.context=e.detail.context;mountData.data.navigation={...mountData.data.navigation,follow:null};following?.dispose();mounts?.dispose();following=mounts=null;pendingNavigation=false;clearTimeout(persistTimer);transfers?.invalidate();tree?.contextChanged();message('连接已更换，刷新目录以读取最新文件');controls();});
 addEventListener('sodeck:connectionChanged',e=>{mountData.connected=e.detail.connected;controls();});
 addEventListener('sodeck:flush',async e=>{
  const ticket=++flushTicket;
  try{
   if(e.detail.freeze===true){root.inert=true;}
   await tree?.flush();
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=e.detail.freeze===true;root.inert=readOnly||transferFrozen;
   sodeck.flushed(e.detail.id);
  }catch(error){
   if(disposed||ticket!==flushTicket)return;
   transferFrozen=false;root.inert=readOnly;
   sodeck.flushed(e.detail.id,error.message);
  }
 });
 addEventListener('sodeck:surfaceReady',()=>{
  activated=true;readOnly=false;transferFrozen=false;root.inert=false;
  controls();scheduleNavigation();
 });
 addEventListener('sodeck:transferCancelled',()=>{flushTicket++;transferFrozen=false;root.inert=readOnly;following?.lifecycle({frozen:false});mounts?.visibility(mountData.visible!==false);});
 addEventListener('sodeck:unmount',()=>{disposed=true;flushTicket++;clearTimeout(persistTimer);following?.dispose();mounts?.dispose();transfers?.dispose();tree?.dispose();});
 addEventListener('unload',()=>{following?.dispose();mounts?.dispose();transfers?.dispose();tree?.dispose();});
})();
