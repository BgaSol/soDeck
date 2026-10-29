import {useEffect,useLayoutEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Button,MenuList,type MenuEntry} from '@sodeck/ui';
import {Activity,ArrowDownToLine,ChevronDown,Grid2X2,Layers,Logs,Package,Palette,Power,Save} from 'lucide-react';
import {core,onCoreNotification} from '@sodeck/sdk';
import './menu.css';

type Payload={fontSize:number;limits?:{left:number;top:number;right:number;bottom:number};token:string;items:MenuEntry[];anchor:{x:number;y:number};keyboard:boolean};
declare global{interface Window{__SODECK_APPLICATION_MENU__?:Payload;__sodeckApplicationMenuReason?:string;}}
const icons={appearance:Palette,plugins:Package,communication:Activity,resources:Activity,calls:Logs,recall:Layers,merge:ArrowDownToLine,layoutRetry:Save,exit:Power};
export function ApplicationMenuPopup(){
 const data=window.__SODECK_APPLICATION_MENU__!;
 const panel=useRef<HTMLDivElement>(null),finished=useRef(false);
 const [placed,setPlaced]=useState(false);
 const reply=(reason:string,action:string|null=null)=>{if(finished.current)return;finished.current=true;void invoke('application_menu_reply',{token:data.token,reason,action}).catch(()=>{finished.current=false;});};
 useLayoutEffect(()=>{document.documentElement.style.fontSize=`${data.fontSize}px`;const el=panel.current!;const limit=data.limits??{left:0,top:0,right:innerWidth,bottom:innerHeight};el.style.maxHeight=`${Math.max(80,limit.bottom-limit.top-16)}px`;el.style.maxWidth=`${Math.max(120,limit.right-limit.left-16)}px`;const menu=el.firstElementChild as HTMLElement;menu.style.maxHeight=el.style.maxHeight;const box=el.getBoundingClientRect();el.style.left=`${Math.max(limit.left+8,Math.min(limit.right-box.width-8,data.anchor.x-box.width))}px`;el.style.top=`${Math.max(limit.top+8,Math.min(limit.bottom-box.height-8,data.anchor.y+6))}px`;setPlaced(true);},[data]);
 useEffect(()=>{if(!placed)return;const frame=requestAnimationFrame(()=>{void invoke('application_menu_reply',{token:data.token,reason:'ready',action:null}).then(()=>{const menu=panel.current?.querySelector<HTMLElement>('[role="menu"]');if(data.keyboard)menu?.querySelector<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])')?.focus();else menu?.focus();});});return()=>cancelAnimationFrame(frame);},[placed,data]);
 useEffect(()=>{const key=(event:KeyboardEvent)=>{if(event.isComposing)return;if(event.key==='Escape'||event.key==='Tab'){event.preventDefault();event.stopPropagation();reply(event.key==='Escape'?'escape':event.shiftKey?'shift-tab':'tab');}};window.addEventListener('keydown',key,true);return()=>window.removeEventListener('keydown',key,true);},[]);
 return <div className="application-menu-shield" onPointerDown={e=>{if(e.target===e.currentTarget){e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);}}} onPointerUp={e=>{if(e.target===e.currentTarget){e.preventDefault();reply('outside');}}} onContextMenu={e=>{e.preventDefault();reply('outside');}} onKeyDown={e=>{if(e.key==='Escape'||e.key==='Tab'){e.preventDefault();reply(e.key==='Tab'?'tab':'escape');}}}>
  <div ref={panel} className="application-menu-position" data-placed={placed}><MenuList entries={data.items.map(entry=>({...entry,icon:icons[entry.id as keyof typeof icons]}))} initialFocus={data.keyboard} onSelect={id=>reply('select',id)} onClose={reason=>reply(reason)}/></div>
 </div>;
}

async function openMenu(anchor:{x:number;y:number},keyboard:boolean){
 const menuId=crypto.randomUUID();let finish!:(id:string|null)=>void;const result=new Promise<string|null>(r=>{finish=r;});
 let ended=false;const complete=(payload:{menuId:string;status:string;selectedId:string|null})=>{if(!ended&&payload.menuId===menuId&&payload.status==='closed'){ended=true;finish(payload.selectedId);}};
 const off=onCoreNotification(n=>{if(n.type==='ui.menu.closed')complete(n.payload as Parameters<typeof complete>[0]);});let timer:ReturnType<typeof setInterval>|undefined;let polling=false;
 const cancel=()=>{void core.call('ui.menu.close',{menuId}).catch(()=>{});};window.addEventListener('pagehide',cancel);
 try{await invoke('application_menu_open',{menuId,anchor,keyboard,fontSize:parseFloat(getComputedStyle(document.documentElement).fontSize)});timer=setInterval(()=>{if(polling||ended)return;polling=true;void core.call<Parameters<typeof complete>[0]>('ui.menu.result',{menuId}).then(complete).catch(()=>{ended=true;finish(null);}).finally(()=>{polling=false;});},500);return await result;}
 finally{off();if(timer)clearInterval(timer);window.removeEventListener('pagehide',cancel);}
}
export function ApplicationMenuButton({onSelect,onError}:{onSelect(id:string):void;onError(error:unknown):void}){
 const [open,setOpen]=useState(false),flight=useRef(false),button=useRef<HTMLButtonElement>(null);
 const show=async(keyboard:boolean)=>{if(flight.current)return;flight.current=true;setOpen(true);try{const box=button.current!.getBoundingClientRect();const id=await openMenu({x:box.right,y:box.bottom},keyboard);setOpen(false);if(document.hasFocus()){button.current?.focus();if(['tab','shift-tab'].includes(window.__sodeckApplicationMenuReason??'')){const controls=Array.from(document.querySelectorAll<HTMLElement>('header button:not(:disabled),header [tabindex="0"]'));controls[controls.indexOf(button.current!)+(window.__sodeckApplicationMenuReason==='shift-tab'?-1:1)]?.focus();}}if(id)onSelect(id);}catch(error){onError(typeof error==='string'?new Error(error):error);}finally{flight.current=false;setOpen(false);}};
 return <Button ref={button} className="application-menu" aria-label="应用菜单" aria-haspopup="menu" aria-expanded={open} onClick={e=>void show(e.detail===0)} onKeyDown={e=>{if(e.key==='ArrowDown'){e.preventDefault();void show(true);}}}><Grid2X2 size={15}/><span>应用</span><ChevronDown size={13}/></Button>;
}
