(() => {
  window.installTerminalInput = (terminal, host, state, notify) => {
    let selectionVersion=0,composing=false,menu,disposed=false;
    const platform=navigator.userAgentData?.platform||navigator.platform||'';
    const mac=/mac/i.test(platform),linux=/linux/i.test(platform);
    const active=()=>!disposed&&document.hasFocus()&&(host.contains(document.activeElement)||Boolean(menu?.contains(document.activeElement)));
    const snapshot=()=>({...state(),active:active(),selection:terminal.getSelection(),selectionVersion});
    const clipboard=new TerminalClipboard(snapshot,valid=>sodeckClipboard.readText(valid),(text,valid)=>sodeckClipboard.writeText(text,valid),text=>terminal.paste(text),()=>terminal.clearSelection(),notify);
    const selection=terminal.onSelectionChange(()=>{selectionVersion++;});
    const closeMenu=(focus=false)=>{if(!menu)return;menu.remove();menu=undefined;if(focus&&!disposed)terminal.focus();};
    function showMenu(x,y){
      closeMenu();menu=document.createElement('div');menu.className='sd-menu terminal-menu';menu.setAttribute('role','menu');
      const rows=[['复制','copy',mac?'⌘C':linux?'Ctrl+Shift+C':'Ctrl+C',!terminal.hasSelection()],['粘贴','paste',mac?'⌘V':linux?'Ctrl+Shift+V':'Ctrl+V',!state().canPaste],['全选','all','',false]];
      for(const [label,action,shortcut,disabled]of rows){const b=document.createElement('button');b.type='button';b.setAttribute('role','menuitem');b.disabled=disabled;b.textContent=label;const hint=document.createElement('span');hint.className='shortcut';hint.textContent=shortcut;b.append(hint);b.addEventListener('click',()=>{closeMenu(true);if(action==='copy')void clipboard.copy();if(action==='paste')void clipboard.pasteText();if(action==='all')terminal.selectAll();});menu.append(b);}
      if(!state().canPaste){const reason=document.createElement('p');reason.textContent='当前终端不可输入，仍可复制已显示内容';menu.append(reason);}
      menu.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp'].includes(event.key)){event.preventDefault();const buttons=[...menu.querySelectorAll('button:not(:disabled)')],index=buttons.indexOf(document.activeElement);buttons[(index+(event.key==='ArrowDown'?1:buttons.length-1))%buttons.length]?.focus();}});
      document.body.append(menu);const r=menu.getBoundingClientRect();menu.style.left=Math.max(4,Math.min(x,innerWidth-r.width-4))+'px';menu.style.top=Math.max(4,Math.min(y,innerHeight-r.height-4))+'px';menu.querySelector('button:not(:disabled)')?.focus();
    }
    terminal.attachCustomKeyEventHandler(event=>{
      if(event.type!=='keydown'||composing||event.isComposing||event.keyCode===229)return true;
      const key=event.key.toLowerCase(),plain=!event.altKey;
      const copy=plain&&(mac?event.metaKey&&!event.ctrlKey&&!event.shiftKey&&key==='c':event.ctrlKey&&!event.metaKey&&key==='c'&&(event.shiftKey||(!linux&&terminal.hasSelection())));
      const paste=plain&&(mac?event.metaKey&&!event.ctrlKey&&!event.shiftKey&&key==='v':!event.metaKey&&((event.ctrlKey&&key==='v'&&(!linux||event.shiftKey))||(!event.ctrlKey&&event.shiftKey&&key==='insert')));
      const localMenu=key==='contextmenu'||(event.shiftKey&&!event.ctrlKey&&!event.altKey&&!event.metaKey&&key==='f10');
      if(!copy&&!paste&&!localMenu)return true;
      event.preventDefault();event.stopPropagation();if(!event.repeat){if(copy)void clipboard.copy();if(paste)void clipboard.pasteText();if(localMenu){const r=host.getBoundingClientRect();showMenu(r.left+12,r.top+16);}}return false;
    });
    const pasteEvent=event=>{event.preventDefault();event.stopImmediatePropagation();if(!composing)void clipboard.pasteText();};
    const context=event=>{if(terminal.modes.mouseTrackingMode!=='none'&&!event.shiftKey)return;event.preventDefault();event.stopImmediatePropagation();showMenu(event.clientX,event.clientY);};
    const begin=()=>{composing=true;},end=()=>{composing=false;};
    const outside=event=>{if(menu&&!menu.contains(event.target)){closeMenu();clipboard.invalidate();}};
    const blur=()=>{clipboard.invalidate();closeMenu();};
    const focusOut=event=>{if(!host.contains(event.relatedTarget)&&!menu?.contains(event.relatedTarget))clipboard.invalidate();};
    host.addEventListener('paste',pasteEvent,true);host.addEventListener('contextmenu',context,true);host.addEventListener('compositionstart',begin);host.addEventListener('compositionend',end);host.addEventListener('focusout',focusOut);
    document.addEventListener('pointerdown',outside,true);window.addEventListener('blur',blur);
    return {invalidate(){clipboard.invalidate();closeMenu();},themeChanged(){closeMenu(true);},dispose(){disposed=true;clipboard.invalidate();closeMenu();selection.dispose();host.removeEventListener('paste',pasteEvent,true);host.removeEventListener('contextmenu',context,true);host.removeEventListener('compositionstart',begin);host.removeEventListener('compositionend',end);host.removeEventListener('focusout',focusOut);document.removeEventListener('pointerdown',outside,true);window.removeEventListener('blur',blur);}};
  };
})();
