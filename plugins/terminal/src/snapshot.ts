import { CoreError } from "@sodeck/sdk";

/** 64 Ki UTF-16 units remain under the native 256 KiB UTF-8 chunk bound,
 * including JSON escaping inside an RPC frame. Never split a surrogate pair. */
export function snapshotChunks(snapshot: unknown): string[] {
  const serialized = JSON.stringify(snapshot);
  if (serialized === undefined) throw new CoreError({ code: "SNAPSHOT_INVALID", message: "终端快照格式无效" });
  if (new TextEncoder().encode(serialized).byteLength > 4 * 1024 * 1024) throw new CoreError({ code: "CAPACITY", message: "终端快照超过 4 MiB，未移动当前终端。" });
  const chunks: string[] = [];
  for (let offset = 0; offset < serialized.length;) {
    let end = Math.min(serialized.length, offset + 65536);
    const code = serialized.charCodeAt(end - 1);
    if (end < serialized.length && code >= 0xd800 && code <= 0xdbff) end--;
    chunks.push(serialized.slice(offset, end)); offset = end;
  }
  return chunks;
}
