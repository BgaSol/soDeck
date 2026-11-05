import { Channel } from "@tauri-apps/api/core";
interface Frame {kind: string; packet: number; offset: number; data: number[]; metadata: unknown}
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
/** Per-surface channel; PTY bytes never enter the shell render/event queue. */
export class ComponentData {
  private channels = new Map<string, { channel: Channel<Frame>; ids: Set<string> }>();
  constructor(private call: Call) {}
  async request(token: string, request: Record<string, unknown>, deliver: (frame: Frame) => void) {
    let entry = this.channels.get(token);
    if (!entry) {const channel = new Channel<Frame>();channel.onmessage = deliver;entry = {channel, ids: new Set()};this.channels.set(token, entry);}
    if (request.op === "attach" && typeof request.id === "string") entry.ids.add(request.id);
    return this.call("component_data", {surfaceId: token, request, channel: entry.channel});
  }
  detach(token?: string) {
    if (!token) return;
    const entry = this.channels.get(token);this.channels.delete(token);
    if (entry) for (const id of entry.ids) void this.call("component_data", {surfaceId: token, request: {op: "detach", id}, channel: entry.channel}).catch(() => {});
  }
}
