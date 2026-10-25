/** Clipboard work is bound to an input epoch, never to whichever tab becomes active later. */
export interface ClipboardTarget {
  key: string; active: boolean; canPaste: boolean; canCopy: boolean;
  selection: string; selectionVersion: number;
}
export class TerminalClipboard {
  private epoch = 0;
  private reading = false;
  constructor(private readonly target: () => ClipboardTarget,
    private readonly read: () => Promise<string>, private readonly write: (text:string) => Promise<unknown>,
    private readonly insert: (text:string) => void, private readonly clear: () => void,
    private readonly fail: (e:unknown) => void) {}
  invalidate(): void { this.epoch++; }
  async copy(): Promise<void> {
    const start={...this.target()};if(!start.canCopy||!start.selection)return;
    try { await this.write(start.selection);const next=this.target();if(next.key===start.key&&next.selectionVersion===start.selectionVersion)this.clear(); }
    catch(e){this.fail(e);}
  }
  async paste(): Promise<void> {
    const start={...this.target()},epoch=this.epoch;
    if(this.reading||!start.active||!start.canPaste)return;
    this.reading=true;
    try {const text=await this.read();const next=this.target();if(text&&epoch===this.epoch&&next.key===start.key&&next.active&&next.canPaste)this.insert(text);}
    catch(e){if(epoch===this.epoch)this.fail(e);}
    finally {this.reading=false;}
  }
}
