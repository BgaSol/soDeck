export class TreeMenu {
  constructor(menu,panel){this.menu=menu;this.panel=panel;this.trigger=null;this.target=null;this.composing=false;
    document.addEventListener('pointerdown',e=>{if(!menu.hidden&&!menu.contains(e.target)&&!e.target.closest('[data-tree-action=menu]'))this.close(false);});
    menu.addEventListener('keydown',e=>{const buttons=[...menu.querySelectorAll('button:not([disabled])')];const index=buttons.indexOf(document.activeElement);if(['ArrowUp','ArrowDown','Home','End'].includes(e.key)){e.preventDefault();buttons[e.key==='Home'?0:e.key==='End'?buttons.length-1:(index+(e.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length]?.focus();}if(e.key==='Escape'){e.preventDefault();this.close();}});
    panel.addEventListener('compositionstart',()=>this.composing=true);panel.addEventListener('compositionend',()=>this.composing=false);
  }
  hideMenu(){this.position?.dispose();this.position=null;this.menu.hidden=true;this.expanded(false);}
  close(focus=true){const open=!this.menu.hidden||!this.panel.hidden;this.hideMenu();if(!this.panel.hidden)SoDeckMotion.exitMotion(this.panel);this.panel.hidden=true;this.expanded(false);if(focus&&open)this.restore?.();this.target=null;}
  expanded(value){if(this.trigger?.hasAttribute('aria-haspopup'))this.trigger.setAttribute('aria-expanded',String(value));}
  open(node,items,trigger,restore){this.close(false);this.target=node;this.trigger=trigger;this.restore=restore;this.menu.replaceChildren();
    for(const item of items){if(item.separator){const line=document.createElement('hr');line.setAttribute('role','separator');this.menu.append(line);continue;}const button=document.createElement('button');button.type='button';button.setAttribute('role','menuitem');button.textContent=item.label;button.disabled=!!item.disabled;button.title=item.reason||'';button.classList.toggle('danger',!!item.danger);button.onclick=()=>{this.hideMenu();item.run();};this.menu.append(button);}
    this.menu.hidden=false;this.expanded(true);
    const position=this.position=SoDeckMenuPosition.attachMenuPosition(this.menu,trigger,{onInvalid:()=>this.close(false)});
    void position.ready.then(ready=>{if(ready&&this.position===position)this.menu.querySelector('button:not([disabled])')?.focus();});
  }
  form({title,text,impact,input,label,value,submit='确认',danger=false,onSubmit}){
    this.hideMenu();this.panel.replaceChildren();const form=document.createElement('form');const h=document.createElement('h2');h.textContent=title;form.append(h);
    for(const copy of [text,impact].filter(Boolean)){const p=document.createElement('p');p.textContent=copy;form.append(p);}
    let field;if(input){const l=document.createElement('label');l.textContent=label;field=document.createElement('input');field.value=value||'';field.id='browser-input';field.required=true;field.spellcheck=false;field.autocomplete='off';l.htmlFor=field.id;form.append(l,field);}
    const tools=document.createElement('div');tools.className='toolbar';const cancel=document.createElement('button');cancel.type='button';cancel.textContent='取消';cancel.onclick=()=>this.close();const accept=document.createElement('button');accept.type='submit';accept.textContent=submit;accept.classList.toggle('danger',danger);tools.append(cancel,accept);form.append(tools);
    const notice=document.createElement('p');notice.className='muted';notice.dataset.validity='';form.append(notice);
    form.onsubmit=e=>{e.preventDefault();if(accept.disabled)return;const text=field?.value;this.close();onSubmit(text);};this.panel.append(form);this.panel.hidden=false;SoDeckMotion.enterMotion(this.panel);(input?field:cancel).focus();
  }
  valid(valid,reason='连接或资源观察已改变，请关闭后重新选择'){const button=this.panel.querySelector('[type=submit]');if(button)button.disabled=!valid;const p=this.panel.querySelector('[data-validity]');if(p)p.textContent=valid?'':reason;}
}
