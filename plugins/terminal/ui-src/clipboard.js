/* Terminal-only keyboard, selection and asynchronous-target rules. */
(() => {
  window.TerminalClipboard = class {
    constructor(state, read, write, paste, clear, fail) {Object.assign(this,{state,read,write,paste,clear,fail});this.epoch=0;this.reading=false;this.copying=false;}
    invalidate(){this.epoch++;}
    valid(snapshot,epoch){const now=this.state();return epoch===this.epoch&&snapshot.key===now.key&&now.active;}
    async copy(){
      const s=this.state(),epoch=this.epoch;
      if(!s.active||!s.canCopy||!s.selection||this.copying)return;
      this.copying=true;
      try{const ok=await this.write(s.selection,()=>this.valid(s,epoch));if(ok!==false&&this.valid(s,epoch)&&this.state().selectionVersion===s.selectionVersion)this.clear();}
      catch(e){if(this.valid(s,epoch))this.fail(e);}finally{this.copying=false;}
    }
    async pasteText(){
      const s=this.state(),epoch=this.epoch;
      if(!s.active||!s.canPaste||this.reading)return;
      this.reading=true;
      try{const text=await this.read(()=>this.valid(s,epoch)&&this.state().canPaste);if(text&&this.valid(s,epoch)&&this.state().canPaste)this.paste(text);}
      catch(e){if(this.valid(s,epoch))this.fail(e);}finally{this.reading=false;}
    }
  };
})();
