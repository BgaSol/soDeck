import { Channel } from "@tauri-apps/api/core";
interface Frame {kind: string; packet: number; offset: number; data: number[]; metadata: unknown}
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
/** Per-surface channel; PTY bytes never enter the shell render/event queue. */
export class ComponentData {
  private channels = new Map<string, { channel: Channel<Frame>; ids: Map<string, Record<string, unknown>> }>();
  constructor(private call: Call) {}
  async request(token: string, request: Record<string, unknown>, deliver: (frame: Frame) => void) {
    let entry = this.channels.get(token);
    if (!entry) {const channel = new Channel<Frame>();channel.onmessage = deliver;entry = {channel, ids: new Map()};this.channels.set(token, entry);}
    if (typeof request.id === "string") {
      if (request.op === "attach") entry.ids.set(request.id, {op: "detach", id: request.id});
      if (request.op === "sessionStream" && request.action === "attach") entry.ids.set(request.id, {op: "sessionStream", action: "detach", id: request.id});
      if (request.op === "detach" || request.action === "detach") entry.ids.delete(request.id);
    }
    return this.call("component_data", {surfaceId: token, request, channel: entry.channel});
  }
  detach(token?: string) {
    if (!token) return;
    const entry = this.channels.get(token);this.channels.delete(token);
    if (entry) for (const request of entry.ids.values()) void this.call("component_data", {surfaceId: token, request, channel: entry.channel}).catch(() => {});
  }
}
