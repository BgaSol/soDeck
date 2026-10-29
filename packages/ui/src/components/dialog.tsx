import {useEffect,useId,useRef,type ReactNode} from 'react';
import {cx,IconButton} from './controls';

export function DialogBody({children}:{children:ReactNode}) {return <div className="sd-dialog-body">{children}</div>;}
export function DialogActions({children}:{children:ReactNode}) {return <div className="sd-dialog-actions">{children}</div>;}

export function Dialog({title,subtitle,onClose,children,wide=false,dismissOnOutside=false,width,height,className=''}:{title:string;subtitle?:string;onClose():void;children:ReactNode;width?:number;height?:number;wide?:boolean;dismissOnOutside?:boolean;className?:string}){
 const ref=useRef<HTMLDivElement>(null),close=useRef(onClose),titleId=useId();close.current=onClose;
 useEffect(()=>{const previous=document.activeElement as HTMLElement|null;const node=ref.current;const focus=node?.querySelector<HTMLElement>('[data-dialog-default]')??node?.querySelector<HTMLElement>('input:not(:disabled),select:not(:disabled),textarea:not(:disabled)')??node?.querySelector<HTMLElement>('button:not(:disabled)')??node;focus?.focus();return()=>{if(previous?.isConnected)previous.focus();};},[]);
 return <div className="sd-dialog-backdrop dialog-backdrop" onPointerDown={event=>{if(dismissOnOutside&&event.target===event.currentTarget)close.current();}}><div ref={ref} style={{width:width===undefined?undefined:Math.max(240,Math.min(960,width)),maxHeight:height===undefined?undefined:`min(${Math.max(160,Math.min(960,height))}px, calc(100vh - 32px))`}} tabIndex={-1} className={cx('sd-dialog','dialog',wide&&'wide',className)} role="dialog" aria-modal="true" aria-labelledby={titleId} onKeyDown={event=>{
  if(event.nativeEvent.isComposing)return;
  if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close.current();}
  if(event.key==='Tab'){const nodes=Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),summary,[tabindex="0"]')??[]).filter(n=>n.getClientRects().length>0);const first=nodes[0],last=nodes.at(-1);if(!first){event.preventDefault();ref.current?.focus();}else if(event.shiftKey&&(document.activeElement===first||document.activeElement===ref.current)){event.preventDefault();last?.focus();}else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}}
 }}><header><div><h2 id={titleId}>{title}</h2>{subtitle&&<p>{subtitle}</p>}</div><IconButton label="关闭对话框" variant="quiet" onClick={onClose}>×</IconButton></header>{children}</div></div>;
}
