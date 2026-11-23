import {attachMenuPosition,type MenuPosition,type MenuReference} from './menu-position.js';
import {updateDOM} from './dom-update';
import {escape as e} from './ui';
import {actionKey,type ServerAction,type ServerCommand} from './server-actions';

/** Owns the directory's one menu; target validity and commands belong to its caller. */
export class ServerMenu {
  private current?:{node:HTMLElement;anchor:HTMLElement;events:AbortController;position?:MenuPosition;actions:ServerAction[];restore:()=>void;valid:()=>boolean;title:string};
  constructor(private execute:(command:ServerCommand)=>void){}
  get open(){return !!this.current;}
  show(title:string,actions:ServerAction[],anchor:HTMLElement,reference:MenuReference,restore:()=>void,valid:()=>boolean){
    this.close();const node=document.createElement('div'),events=new AbortController();
    node.className='catalog-action-menu sd-menu';node.setAttribute('role','menu');node.setAttribute('aria-label',title);
    document.body.append(node);this.current={node,anchor,events,actions,restore,valid,title};if(anchor.id==='catalog-new-menu')anchor.setAttribute('aria-expanded','true');this.update(actions,title);
    const {signal}=events;
    node.addEventListener('click',event=>{
      const button=(event.target as Element).closest<HTMLButtonElement>('[data-menu-action]');
      const item=this.current?.actions.find(a=>actionKey(a.command)===button?.dataset.menuAction);
      if(!item||item.disabled||!valid())return;
      this.close();this.execute(item.command);
    },{signal});
    node.addEventListener('keydown',event=>{
      if(event.isComposing)return;
      if(event.key==='Escape'){event.preventDefault();event.stopPropagation();this.close(true);return;}
      if(event.key==='Tab'){this.close(true);return;}
      if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
        event.preventDefault();event.stopPropagation();const buttons=[...node.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')],index=buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+(event.key==='ArrowUp'?-1:1)+buttons.length)%buttons.length]?.focus();
      }
    },{signal});
    for(const name of ['pointerdown','focusin'] as const)document.addEventListener(name,event=>{if(!node.contains(event.target as Node)&&!anchor.contains(event.target as Node))this.close();},{signal});
    window.addEventListener('blur',()=>this.close(),{signal});
    const outsideScroll=(event:Event)=>{if(event.target instanceof Node&&!node.contains(event.target))this.close();};
    document.addEventListener('wheel',outsideScroll,{capture:true,signal});
    const current=this.current;
    current.position=attachMenuPosition(node,reference,{isValid:()=>this.current===current&&valid(),onInvalid:()=>this.close()});
    void current.position.ready.then(ready=>{if(ready&&this.current===current){node.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus({preventScroll:true});requestAnimationFrame(()=>{if(this.current===current)document.addEventListener('scroll',outsideScroll,{capture:true,signal});});}});
  }
  update(actions:ServerAction[],title?:string){
    const current=this.current;if(!current)return;
    if(!current.valid()){this.close();return;}
    current.actions=actions;if(title)current.title=title;
    const focused=(document.activeElement as HTMLElement)?.dataset.menuAction;let group='';
    const rows=actions.map(item=>{const separator=group&&group!==item.group?'<hr>':'';group=item.group;const key=actionKey(item.command),description=`catalog-menu-${key}-reason`;
      return `${separator}<button type="button" role="menuitem" tabindex="-1" data-dom-key="menu:${key}" data-menu-action="${key}" class="${item.danger?'danger':''}" ${item.disabled?`disabled aria-describedby="${description}"`:''}>${e(item.label)}</button>${item.disabled?`<p class="catalog-menu-reason" id="${description}">${e(item.disabled)}</p>`:''}`;
    }).join('');
    updateDOM(current.node,`<p class="catalog-menu-title">${e(current.title)}</p><div class="sd-menu-list">${rows}</div>`,false);
    if(focused)([...current.node.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')].find(b=>b.dataset.menuAction===focused)??current.node.querySelector<HTMLButtonElement>('button:not(:disabled)'))?.focus({preventScroll:true});
    void current.position?.update();
  }
  close(focus=false){const current=this.current;if(!current)return;this.current=undefined;current.position?.dispose();current.events.abort();current.node.remove();if(current.anchor.id==='catalog-new-menu')current.anchor.setAttribute('aria-expanded','false');if(focus)current.restore();}
  dispose(){this.close();}
}
