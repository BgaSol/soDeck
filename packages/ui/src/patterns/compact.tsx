import {useState,type ReactNode} from 'react';
import {ChevronRight} from 'lucide-react';
import {cx,Icon,type IconType} from '../components/controls';
import type {Tone} from './index';

export function IdentitySummary({icon,title,detail,tone='accent'}:{icon:IconType;title:ReactNode;detail?:ReactNode;tone?:Tone}) {
  return <div className="sd-identity"><span className="sd-identity-icon" data-tone={tone}><Icon icon={icon}/></span><div><strong>{title}</strong>{detail&&<div className="sd-identity-detail">{detail}</div>}</div></div>;
}
export function CodeValue({label,value,caption,action,muted=false,outlined=false}:{label:string;value:string;caption?:ReactNode;action?:ReactNode;muted?:boolean;outlined?:boolean}) {
  return <section className="sd-code-value" data-muted={muted||undefined} data-outlined={outlined||undefined} aria-label={label}><div className="sd-code-value-heading"><span>{label}</span>{action}</div><code>{value}</code>{caption&&<small>{caption}</small>}</section>;
}
export function PanelToolbar({children,actions}:{children:ReactNode;actions?:ReactNode}) {
  return <div className="sd-panel-toolbar"><div>{children}</div><div>{actions}</div></div>;
}
export function StatusLabel({children,tone='neutral'}:{children:ReactNode;tone?:Tone}) {
  return <span className="sd-status-label" data-tone={tone}><i aria-hidden="true"/>{children}</span>;
}
export function ResourceItem({title,subtitle,description,icon,selected=false,onSelect,status,actions,children}:{title:string;subtitle?:string;description?:string;icon:IconType;selected?:boolean;onSelect():void;status?:ReactNode;actions?:ReactNode;children?:ReactNode}) {
  return <div className="sd-list-row sd-resource-item" data-selected={selected||undefined}><button type="button" className="sd-resource-select" aria-label={title} title={description} aria-pressed={selected} onClick={onSelect}><span className="sd-resource-icon"><Icon icon={icon}/></span><span className="sd-resource-heading"><strong>{title}</strong>{subtitle&&<small>{subtitle}</small>}</span></button><div className="sd-resource-status">{status}</div><div className="sd-resource-actions">{actions}</div>{children&&<div className="sd-resource-content">{children}</div>}</div>;
}
export function Disclosure({title,children,onOpen,className}:{title:string;children:ReactNode;onOpen?():void;className?:string}) {
  const [open,setOpen]=useState(false);
  return <details className={cx('sd-disclosure',className)} open={open} onToggle={e=>{const next=e.currentTarget.open;if(next!==open){setOpen(next);if(next)onOpen?.();}}}><summary><span>{title}</span><Icon icon={ChevronRight}/></summary>{open&&<div className="sd-disclosure-body">{children}</div>}</details>;
}
