import { Channel } from "@tauri-apps/api/core";
interface Frame {kind: string; packet: number; offset: number; data: number[]; metadata: unknown}
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
/** Each native invocation owns its Channel sequence; surfaces own only cleanup requests. */
export class ComponentData {
  private subscriptions = new Map<string, Map<string, Record<string, unknown>>>();
  constructor(private call: Call) {}
  async request(token: string, request: Record<string, unknown>, deliver: (frame: Frame) => void) {
    const channel = new Channel<Frame>();
    channel.onmessage = deliver;
    let entries = this.subscriptions.get(token);
    let detach: Record<string, unknown> | undefined;
    if (typeof request.id === "string") {
      if (request.op === "attach") detach = {op: "detach", id: request.id};
      if (request.op === "processOutput" && request.action === "attach") detach = {op: "processOutput", action: "detach", id: request.id};
      if (detach) {
        if (!entries) { entries = new Map(); this.subscriptions.set(token, entries); }
        entries.set(request.id, detach);
      }
      if (request.op === "detach" || request.action === "detach") entries?.delete(request.id);
    }
    try {
      return await this.call("component_data", {surfaceId: token, request, channel});
    } catch (error) {
      if (detach && entries?.get(request.id as string) === detach) entries.delete(request.id as string);
      throw error;
    }
  }
  detach(token?: string) {
    if (!token) return;
    const entries = this.subscriptions.get(token);this.subscriptions.delete(token);
    if (entries) for (const request of entries.values()) void this.request(token,request,()=>{}).catch(() => {});
  }
}
