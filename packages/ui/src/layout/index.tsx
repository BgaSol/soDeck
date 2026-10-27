import type {HTMLAttributes, ReactNode} from 'react';
import {cx} from '../components/controls';
export type Gap='none'|'xs'|'sm'|'md'|'lg';
type LayoutProps=HTMLAttributes<HTMLDivElement> & {gap?:Gap;fill?:boolean};
export function Stack({gap='sm',fill,className,...props}:LayoutProps){return <div {...props} className={cx('sd-stack',fill&&'sd-fill',className)} data-gap={gap}/>;}
export function Inline({gap='sm',fill,className,...props}:LayoutProps){return <div {...props} className={cx('sd-inline',fill&&'sd-fill',className)} data-gap={gap}/>;}
export function Grid({className,children}:{className?:string;children:ReactNode}){return <div className={cx('sd-grid',className)}>{children}</div>;}
export function ScrollArea({className,...props}:HTMLAttributes<HTMLDivElement>&{ref?:import('react').Ref<HTMLDivElement>}){return <div {...props} className={cx('sd-scroll',className)}/>;}
