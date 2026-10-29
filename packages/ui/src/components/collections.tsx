import {useRef,useEffect,type CSSProperties,type HTMLAttributes,type KeyboardEvent,type ReactNode} from 'react';
import {cx,Icon,Button,type IconType} from './controls';
import {capacityUsage,type CapacityInput} from '../capacity';

export function ListRow({selected=false,disabled=false,className,...props}:HTMLAttributes<HTMLDivElement>&{selected?:boolean;disabled?:boolean}){return <div {...props} className={cx('sd-list-row',className)} data-selected={selected} aria-disabled={disabled||undefined}/>;}
export function TreeRow({level=0,selected=false,expanded,capacity,children,className,style,title,...props}:HTMLAttributes<HTMLDivElement>&{level?:number;selected?:boolean;expanded?:boolean;capacity?:CapacityInput}){
 const usage=capacity&&capacityUsage(capacity);
 return <div {...props} role="treeitem" aria-level={level+1} aria-selected={selected} aria-expanded={expanded} data-selected={selected} data-capacity={usage?true:undefined} className={cx('sd-tree-row',className)} title={[title,usage?.description].filter(Boolean).join('\n')||undefined} style={{...style,'--sd-row-level':Math.max(0,Math.floor(level))} as CSSProperties}>
  {usage&&<span className="sd-capacity-meter" role="meter" aria-label="容量使用率" aria-valuemin={0} aria-valuemax={100} aria-valuenow={usage.percent} aria-valuetext={usage.description}>
   <span className="sd-capacity-fill" data-level={usage.level} data-empty={usage.percent===0||undefined} style={{width:`${usage.percent}%`}}/>
  </span>}{children}
 </div>;
}
export function TabItem({selected=false,className,...props}:HTMLAttributes<HTMLDivElement>&{selected?:boolean}){return <div {...props} role="tab" aria-selected={selected} data-selected={selected} className={cx('sd-tab',className)}/>;}
/** Keyboard navigation without application routing or data dependencies. */
export function SegmentedTabs<T extends string>({label,value,items,onChange,id}:{label:string;value:T;items:readonly {id:T;label:string}[];onChange(value:T):void;id:string}) {
 const root=useRef<HTMLDivElement>(null);
 const key=(event:KeyboardEvent)=>{
  if(event.nativeEvent.isComposing)return;
  const current=items.findIndex(item=>item.id===value);let index:number|undefined;
  if(event.key==='ArrowRight')index=(current+1)%items.length;if(event.key==='ArrowLeft')index=(current-1+items.length)%items.length;
  if(event.key==='Home')index=0;if(event.key==='End')index=items.length-1;
  if(index!==undefined){event.preventDefault();onChange(items[index].id);root.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[index]?.focus();}
 };
 return <div ref={root} role="tablist" aria-label={label} className="sd-segmented-tabs" onKeyDown={key}>{items.map(item=><Button key={item.id} id={`${id}-${item.id}`} role="tab" variant="quiet" aria-selected={value===item.id} aria-controls={`${id}-panel-${item.id}`} tabIndex={value===item.id?0:-1} onClick={()=>onChange(item.id)}>{item.label}</Button>)}</div>;
}
export type MenuEntry={id:string;label:string;icon?:IconType;detail?:string;badge?:string;disabled?:boolean;danger?:boolean;separator?:boolean};
export function MenuItem({entry,active,onActivate,onFocus}:{entry:MenuEntry;active:boolean;onActivate():void;onFocus():void}){return <button type="button" role="menuitem" aria-disabled={entry.disabled||undefined} tabIndex={active?0:-1} className="sd-menu-item" data-danger={entry.danger||undefined} onFocus={onFocus} onClick={()=>{if(!entry.disabled)onActivate();}}>{entry.icon&&<Icon icon={entry.icon}/>}<span className="sd-menu-label">{entry.label}{entry.detail&&<small>{entry.detail}</small>}</span>{entry.badge&&<span className="sd-menu-badge">{entry.badge}</span>}</button>;}
export function MenuList({entries,onSelect,onClose,initialFocus=false,label='应用菜单'}:{entries:readonly MenuEntry[];onSelect(id:string):void;onClose(reason:'escape'|'tab'):void;initialFocus?:boolean;label?:string}){
 const ref=useRef<HTMLDivElement>(null);
 useEffect(()=>{if(initialFocus)ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not([aria-disabled="true"])')?.focus();else ref.current?.focus();},[initialFocus]);
 const focusable=()=>Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([aria-disabled="true"])')??[]);
 const key=(event:KeyboardEvent)=>{if(event.nativeEvent.isComposing)return;const items=focusable(),index=items.indexOf(document.activeElement as HTMLButtonElement);let next:HTMLButtonElement|undefined;
   if(event.key==='Escape'||event.key==='Tab'){event.preventDefault();event.stopPropagation();onClose(event.key==='Tab'?'tab':'escape');return;}
   if(event.key==='Home')next=items[0];if(event.key==='End')next=items.at(-1);if(event.key==='ArrowDown')next=items[(index+1)%items.length];if(event.key==='ArrowUp')next=items[(index-1+items.length)%items.length];if(next){event.preventDefault();next.focus();}
 };
 return <div className="sd-menu" role="menu" aria-label={label} onKeyDown={key} tabIndex={-1} ref={ref}>{entries.map((entry,index)=><div key={entry.id} role="none">{entry.separator&&<div role="separator" className="sd-separator"/>}<MenuItem entry={entry} active={index===entries.findIndex(e=>!e.disabled)} onActivate={()=>onSelect(entry.id)} onFocus={()=>{for(const b of focusable())b.tabIndex=b===document.activeElement?0:-1;}}/></div>)}</div>;
}
