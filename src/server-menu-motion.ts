import {animateMotion,cancelMotion,exitMotion} from './motion.js';
import type {MenuLayout} from './menu-position.js';
import {subscribeTheme} from './theme';

/** Visual resources only; the menu session owns input, commands and focus. */
export class ServerMenuMotion {
  private entry?:{node:HTMLElement;layout:MenuLayout;dx:number;dy:number;scale:boolean};
  private stopTheme?:()=>void;
  private tail?:()=>void;
  present(node:HTMLElement,layout:MenuLayout){
    const first=this.entry?.node!==node;
    if(first){this.clear();this.stopTheme=subscribeTheme(()=>cancelMotion(node));}
    else if(['x','y','width','height','placement'].every(key=>this.entry!.layout[key as keyof MenuLayout]===layout[key as keyof MenuLayout]))return;
    else cancelMotion(node);
    const {x,y,width,height,reference}=layout,view=node.ownerDocument.defaultView!;
    const ox=Math.max(0,Math.min(width,reference.left-x)),oy=Math.max(0,Math.min(height,reference.top-y));
    const dx=ox<=width/2?-Math.min(4,Math.max(0,x-8)):Math.min(4,Math.max(0,view.innerWidth-8-x-width));
    const dy=oy<=height/2?-Math.min(4,Math.max(0,y-8)):Math.min(4,Math.max(0,view.innerHeight-8-y-height));
    const scale=height<=320;
    node.style.transformOrigin=`${ox}px ${oy}px`;
    this.entry={node,layout,dx,dy,scale};
    if(first)animateMotion(node,[{opacity:0,transform:`translate(${dx}px,${dy}px) scale(${scale ? .985 : 1})`},{opacity:1,transform:'none'}]);
  }
  retire(node:HTMLElement,anchor:HTMLElement,mode:'retract'|'fade'){
    const entry=this.entry;
    if(entry?.node!==node){this.clear();return;}
    const transform=mode==='fade'?'none':`translate(${entry.dx/2}px,${entry.dy/2}px) scale(${entry.scale ? .99 : 1})`;
    const exit=exitMotion(node,{transform});
    this.clear();
    if(!exit)return;
    const document=node.ownerDocument,view=document.defaultView!,events=new AbortController(),{signal}=events;
    const positions=new Map<EventTarget,{node:Element;left:number;top:number}>();
    const invalid=()=>!anchor.isConnected||!!anchor.closest('[hidden],[inert]')||!anchor.getClientRects().length||getComputedStyle(anchor).visibility!=='visible'||!!document.querySelector('dialog[open]');
    const observer=new MutationObserver(()=>{if(invalid())clear();});
    for(let parent:HTMLElement|null=anchor;parent;parent=parent.parentElement){
      observer.observe(parent,{childList:true,attributes:true,attributeFilter:['hidden','inert','style','class']});
      positions.set(parent,{node:parent,left:parent.scrollLeft,top:parent.scrollTop});
    }
    const root=document.scrollingElement;if(root)positions.set(document,{node:root,left:root.scrollLeft,top:root.scrollTop});
    const stopTheme=subscribeTheme(()=>clear());
    const clear=()=>{exit.cancel();events.abort();observer.disconnect();stopTheme();if(this.tail===clear)this.tail=undefined;};
    this.tail=clear;
    view.addEventListener('blur',clear,{signal});
    document.addEventListener('focusin',()=>{if(invalid())clear();},{capture:true,signal});
    document.addEventListener('scroll',event=>{const before=positions.get(event.target!);if(before&&(before.node.scrollLeft!==before.left||before.node.scrollTop!==before.top))clear();},{capture:true,signal});
    void exit.finished.then(clear);
    if(invalid())clear();
  }
  clear(){
    this.tail?.();
    if(this.entry)cancelMotion(this.entry.node);
    this.entry=undefined;this.stopTheme?.();this.stopTheme=undefined;
  }
}
