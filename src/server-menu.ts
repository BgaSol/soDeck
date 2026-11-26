import {positionMenu,type MenuPosition,type MenuReference} from './menu-position.js';
import {ServerMenuMotion} from './server-menu-motion';
import {updateDOM} from './dom-update';
import {escape as e} from './ui';
import {actionKey,type ServerAction,type ServerCommand} from './server-actions';

export type MenuSource=Readonly<{kind:'node';path:string;role:boolean;stamp:string}|{kind:'root'}|{kind:'toolbar';menu?:'sharing'}>;
type CloseReason='dismiss'|'cancel'|'tab'|'execute'|'replace'|'immediate';
interface MenuOptions {
  source:MenuSource;title:string;actions:ServerAction[];anchor:HTMLElement;reference:MenuReference;
  restore:()=>void;valid:()=>boolean;scrollElement?:HTMLElement;
}
interface MenuSession extends MenuOptions {node:HTMLElement;events:AbortController;position?:MenuPosition}

/** Owns one complete directory menu session, including its semantic source. */
export class ServerMenu {
  private current?:MenuSession;
  private disposed=false;
  private motion=new ServerMenuMotion();
  constructor(private execute:(command:ServerCommand,source:MenuSource)=>void){}
  get source(){return this.current?.source;}
  show(options:MenuOptions){
    if(this.disposed)return;
    this.close('replace');
    const node=document.createElement('div'),events=new AbortController();
    const session:MenuSession={...options,source:Object.freeze({...options.source}),node,events};
    const {anchor,reference,source}=session,{signal}=events;
    const active=()=>this.current===session;
    const valid=()=>active()&&!this.disposed&&session.valid();
    node.className='catalog-action-menu sd-menu';node.tabIndex=-1;node.setAttribute('role','menu');node.setAttribute('aria-label',session.title);
    this.current=session;document.body.append(node);
    if(source.kind==='toolbar')anchor.setAttribute('aria-expanded','true');
    this.update(session.actions);
    node.addEventListener('click',event=>{
      if(!active())return;
      const button=(event.target as Element).closest<HTMLButtonElement>('[data-menu-action]');
      const item=session.actions.find(a=>actionKey(a.command)===button?.dataset.menuAction);
      if(!item||item.disabled){node.focus({preventScroll:true});return;}
      if(!valid()){this.close('immediate');return;}
      const {command}=item,origin=session.source;
      this.close('execute');this.execute(command,origin);
    },{signal});
    node.addEventListener('keydown',event=>{
      if(event.isComposing||!active()||!['ArrowDown','ArrowUp','Home','End'].includes(event.key))return;
      event.preventDefault();event.stopPropagation();
      const buttons=[...node.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')],index=buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[event.key==='Home'?0:event.key==='End'?buttons.length-1:index<0?event.key==='ArrowUp'?buttons.length-1:0:(index+(event.key==='ArrowUp'?-1:1)+buttons.length)%buttons.length]?.focus({preventScroll:true});
    },{signal});
    // Cancellation belongs to the session, even if a title or disabled item lost focus.
    document.addEventListener('keydown',event=>{
      if(!active()||event.isComposing)return;
      if(event.key==='Escape'){event.preventDefault();event.stopPropagation();this.close('cancel');}
      else if(event.key==='Tab')this.close('tab');
    },{capture:true,signal});
    const outside=(event:Event)=>{
      if(!active()||node.contains(event.target as Node))return;
      if(source.kind==='toolbar'&&anchor.contains(event.target as Node))return;
      this.close(event.target instanceof Element&&event.target.closest('dialog[open]')?'immediate':'dismiss');
    };
    document.addEventListener('pointerdown',outside,{capture:true,signal});
    document.addEventListener('focusin',outside,{capture:true,signal});
    // Point references already invalidate on source scrolling in the positioner.
    if(source.kind!=='toolbar'&&reference instanceof HTMLElement&&session.scrollElement){
      const scroller=session.scrollElement,left=scroller.scrollLeft,top=scroller.scrollTop;
      scroller.addEventListener('scroll',()=>{if(active()&&(scroller.scrollLeft!==left||scroller.scrollTop!==top))this.close('immediate');},{signal});
    }
    session.position=positionMenu(node,reference,{isValid:valid,onPosition:layout=>{if(valid())this.motion.present(node,layout);},onInvalid:()=>{if(active())this.close('immediate');}});
    void session.position.ready.then(ready=>{if(ready&&valid())(node.querySelector<HTMLElement>('button:not(:disabled)')??node).focus({preventScroll:true});});
  }
  update(actions:ServerAction[],title?:string){
    const session=this.current;if(!session)return;
    if(!session.valid()){this.close('immediate');return;}
    const {node}=session,hadFocus=node.contains(document.activeElement),focused=(document.activeElement as HTMLElement)?.dataset.menuAction;
    const oldIndex=session.actions.findIndex(a=>actionKey(a.command)===focused);
    session.actions=actions;if(title)session.title=title;
    node.setAttribute('aria-label',session.title);let group='';
    const rows=actions.map(item=>{const separator=group&&group!==item.group?'<hr>':'';group=item.group;const key=actionKey(item.command),description=`catalog-menu-${key}-reason`;
      return `${separator}<button type="button" role="menuitem" tabindex="-1" data-dom-key="menu:${key}" data-menu-action="${key}" class="${item.danger?'danger':''}" ${item.disabled?`disabled aria-describedby="${description}"`:''}>${e(item.label)}</button>${item.disabled?`<p class="catalog-menu-reason" id="${description}">${e(item.disabled)}</p>`:''}`;
    }).join('');
    updateDOM(node,`<p class="catalog-menu-title">${e(session.title)}</p><div class="sd-menu-list">${rows}</div>`,false);
    if(hadFocus&&this.current===session){
      const next=actions.find(a=>actionKey(a.command)===focused&&!a.disabled)??actions.slice(Math.max(0,oldIndex)).find(a=>!a.disabled)??actions.find(a=>!a.disabled);
      (next?node.querySelector<HTMLElement>(`[data-menu-action="${actionKey(next.command)}"]`):node)?.focus({preventScroll:true});
    }
    void session.position?.update();
  }
  close(reason:CloseReason='dismiss'){
    if(reason==='replace'||reason==='immediate')this.motion.clear();
    const session=this.current;if(!session)return;
    this.current=undefined;session.events.abort();session.position?.dispose();
    if(reason!=='replace'&&reason!=='immediate')this.motion.retire(session.node,session.anchor,reason==='tab'||reason==='execute'?'fade':'retract');
    session.node.remove();
    if(session.source.kind==='toolbar')session.anchor.setAttribute('aria-expanded','false');
    if(reason==='cancel'||reason==='tab')session.restore();
  }
  dispose(){this.disposed=true;this.close('immediate');}
}
