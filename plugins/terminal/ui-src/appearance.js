import {loadFont} from './font-loader.js';
import {fontPresets,fontAsset} from './font-presets.js';
export const terminalDefaults = {font:{source:'preset',id:'jetbrains-mono'},fontSize:14,lineHeight:1.2,paddingBottom:24,cursorStyle:'block',cursorBlink:true};
export function createAppearance(terminal, host, ready, resize, invalidate) {
  let snapshot, epoch=0, disposed=false, applying;
  function receive(next) {
    if (!next) return;
    if(snapshot&&snapshot.definitionToken===next.definitionToken&&BigInt(snapshot.revision)>BigInt(next.revision))return;
    // Re-delivery also retries a failed font/resize application at the saved revision.
    snapshot=next;epoch++;void apply();
  }
  async function apply() {
    if(disposed)return;
    const ticket=epoch, current=snapshot, values={...terminalDefaults,...current?.values};
    if(applying===ticket)return;applying=ticket;
    let family,error;
    try {family=await loadFont(values.font,fontPresets,fontAsset);}
    catch(e){error=e.message;try{family=await loadFont(terminalDefaults.font,fontPresets,fontAsset);}catch{family='monospace';}}
    if(disposed||ticket!==epoch)return;
    const geometry=terminal.options.fontFamily!==family||terminal.options.fontSize!==values.fontSize||terminal.options.lineHeight!==values.lineHeight||host.style.paddingBottom!==values.paddingBottom+'px';
    terminal.options.cursorStyle=values.cursorStyle;terminal.options.cursorBlink=values.cursorBlink;
    if (!ready()) {applying=undefined;return;}
    if(geometry){
      const buffer=terminal.buffer.active,atBottom=buffer.viewportY>=buffer.baseY,scroll=buffer.viewportY;
      invalidate();terminal.clearSelection();terminal.options.fontFamily=family;terminal.options.fontSize=values.fontSize;terminal.options.lineHeight=values.lineHeight;
      terminal.options.fontWeight=400;terminal.options.fontWeightBold=700;host.style.paddingBottom=values.paddingBottom+'px';
      try{await resize();}catch(e){error=error||e.message;}
      if(!atBottom)terminal.scrollToLine(scroll);
    }
    if(disposed||ticket!==epoch)return;
    if(current)await sodeck.request('settings',{op:'applied',pageId:current.pageId,definitionToken:current.definitionToken,revision:current.revision,...(error?{error}:{})}).catch(()=>{});
  }
  return {receive,apply,dispose(){disposed=true;epoch++;}};
}
