import { listen, type UnlistenFn } from "@tauri-apps/api/event";
export interface FileDrop {type: "enter" | "over" | "drop" | "leave"; dropId?: string; position?: {x: number; y: number}}
/** Native paths stay in Rust; only a hit surface receives a short-lived input reference. */
export class LocalFileDrop {
  private unlisten?: UnlistenFn;
  private disposed = false;
  constructor(private deliver: (event: FileDrop) => void) {
    void listen<FileDrop>("core-file-drop", event => { if (!this.disposed) this.deliver(event.payload); })
      .then(unlisten => { if (this.disposed) unlisten(); else this.unlisten = unlisten; })
      .catch(() => { /* Browser-only fixtures have no native drop source. */ });
  }
  dispose() { this.disposed = true; this.unlisten?.(); }
}
