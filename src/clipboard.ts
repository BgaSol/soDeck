const LIMIT = 256 * 1024;
const CHUNK = 2048; // UTF-16 units; even JSON-escaped control text fits UI v1's 24 KiB frame.
interface Transfer { id: string; text: string; length: number; offset: number; expires: number; writing: boolean }
export class ClipboardBridge {
  private transfers = new Map<string, Transfer>();
  private pending = new Set<string>();
  private read: () => Promise<string>;
  private write: (text: string) => Promise<void>;
  constructor(read: () => Promise<string>, write: (text: string) => Promise<void>) { this.read=read;this.write=write; }
  discard(surface: string) { this.transfers.delete(surface); }
  async request(surface: string, request: Record<string, unknown>, valid: () => boolean) {
    if (!request || typeof request !== "object" || new TextEncoder().encode(JSON.stringify(request)).length > 24000) throw new Error("剪贴板请求超过控制帧上限");
    const check = () => { if (!valid()) { this.discard(surface); throw new Error("终端或页面已切换，请重新操作剪贴板"); } };
    check();
    if (request.op === "read") {
      if (this.pending.has(surface)) throw new Error("剪贴板读取尚未完成");
      this.pending.add(surface);
      try {
        const text = await this.read(); check();
        if (new TextEncoder().encode(text).length > LIMIT) throw new Error("剪贴板超过 256 KiB，请分段粘贴；本次未发送");
        const id = crypto.randomUUID();
        this.transfers.set(surface, {id, text, length: text.length, offset: 0, expires: Date.now()+15000, writing:false});
        return {id, length: text.length};
      } finally { this.pending.delete(surface); }
    }
    if (request.op === "beginWrite") {
      const length = Number(request.length);
      if (!Number.isSafeInteger(length) || length < 0 || length > LIMIT) throw new Error("剪贴板文本超过 256 KiB");
      const id=crypto.randomUUID();this.transfers.set(surface,{id,text:"",length,offset:0,expires:Date.now()+15000,writing:true});
      return {id};
    }
    const transfer = this.transfers.get(surface);
    if (!transfer || transfer.id !== request.id || transfer.expires < Date.now()) { this.discard(surface); throw new Error("剪贴板操作已失效，请重试"); }
    if (request.op === "chunk" && !transfer.writing) {
      const offset=Number(request.offset);
      if (!Number.isSafeInteger(offset) || offset < 0 || offset > transfer.length) throw new Error("剪贴板分块位置无效");
      const text=transfer.text.slice(offset,offset+CHUNK);
      if(offset+text.length>=transfer.length)this.discard(surface);
      return {text};
    }
    if (request.op === "writeChunk" && transfer.writing) {
      if(typeof request.text!=="string" || request.text.length>CHUNK || request.offset!==transfer.offset || transfer.offset+request.text.length>transfer.length) throw new Error("剪贴板分块顺序或大小无效");
      transfer.text+=request.text;transfer.offset+=request.text.length;
      return {};
    }
    if (request.op === "commitWrite" && transfer.writing) {
      this.discard(surface);
      if(transfer.offset!==transfer.length || new TextEncoder().encode(transfer.text).length>LIMIT)throw new Error("剪贴板文本不完整或超过 256 KiB");
      check();await this.write(transfer.text);return {};
    }
    throw new Error("未知剪贴板操作");
  }
}
