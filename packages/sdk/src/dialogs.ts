import {context,core,onCoreNotification,CoreError} from './index';
import type {DialogResult} from './workbench-ui.generated';

export type DialogContext={sessionId:string;dialogId:string;title:string;width:number;height:number;initialState:unknown};
export function dialogContext():Readonly<DialogContext>|undefined{return context().dialog;}
const terminal=(r:DialogResult)=>['completed','cancelled','failed'].includes(r.status);

/** Install the result listener before opening. Query the retained receipt if a
 * notice is lost; never create a second native session after an uncertain open. */
export async function openDialog<T=unknown>(dialogId:string,initialState:unknown):Promise<T|null>{
  const requestId=crypto.randomUUID();
  let sessionId:string|undefined,finished=false,resolve!:(r:DialogResult)=>void;
  const early=new Map<string,DialogResult>();
  const completion=new Promise<DialogResult>(r=>{resolve=r;});
  const receive=(r:DialogResult)=>{if(!terminal(r))return;if(!sessionId){early.set(r.sessionId,r);return;}if(r.sessionId===sessionId&&!finished){finished=true;resolve(r);}};
  const off=onCoreNotification(n=>{if(n.type==='ui.dialog.closed')receive(n.payload as DialogResult);});
  let timer:ReturnType<typeof setInterval>|undefined,polling=false;
  const cancel=()=>{if(sessionId)void core.call('ui.dialog.close',{sessionId}).catch(()=>{});};
  window.addEventListener('pagehide',cancel);
  try{
    const params={requestId,dialogId,initialState};
    let opened:DialogResult;
    try{opened=await core.call<DialogResult>('ui.dialog.open',params);}
    catch(error){if(!(error instanceof CoreError)||error.code!=='RESULT_UNKNOWN')throw error;opened=await core.call<DialogResult>('ui.dialog.open',params);}
    sessionId=opened.sessionId;receive(early.get(sessionId)??opened);early.clear();
    timer=setInterval(()=>{if(polling||finished)return;polling=true;void core.call<DialogResult>('ui.dialog.result',{sessionId}).then(receive).catch(error=>{finished=true;resolve({sessionId:sessionId!,status:'failed',error:{code:error instanceof CoreError?error.code:'DIALOG_EXPIRED',message:error instanceof Error?error.message:String(error)}});}).finally(()=>{polling=false;});},750);
    const result=await completion;
    if(result.status==='failed')throw new CoreError(result.error??{code:'DIALOG_START_FAILED',message:'弹窗打开失败'});
    return result.status==='completed'?result.result as T:null;
  }finally{off();if(timer)clearInterval(timer);window.removeEventListener('pagehide',cancel);}
}
export async function readyDialog(){const d=dialogContext();if(!d)throw new Error('当前表面不是插件弹窗');return core.call<DialogResult>('ui.dialog.ready',{sessionId:d.sessionId});}
export async function completeDialog(result:unknown){const d=dialogContext();if(!d)throw new Error('当前表面不是插件弹窗');return core.call<DialogResult>('ui.dialog.complete',{sessionId:d.sessionId,result});}
export async function closeDialog(){const d=dialogContext();if(!d)throw new Error('当前表面不是插件弹窗');return core.call<DialogResult>('ui.dialog.close',{sessionId:d.sessionId});}
export function onDialogCancel(callback:()=>void):()=>void {
  const off=onCoreNotification(n=>{if(n.type==='ui.dialog.cancelRequested'&&(n.payload as {sessionId:string}).sessionId===dialogContext()?.sessionId)callback();});
  const key=(event:KeyboardEvent)=>{if(event.key==='Escape'&&!event.isComposing&&!event.defaultPrevented){event.preventDefault();event.stopPropagation();callback();}};
  document.addEventListener('keydown',key);return()=>{off();document.removeEventListener('keydown',key);};
}
