import { decode, pathText, type RemotePath } from "./remote-path";
// Text clipboards cannot represent arbitrary Linux filename bytes. Keep that
// distinction visible instead of presenting an escaped display name as a path.
export function clipboardPath(p: RemotePath): { text: string; label: string } {
  try {
    return {
      text: new TextDecoder("utf-8", { fatal: true }).decode(
        decode(p.bytesBase64),
      ),
      label: "复制完整路径",
    };
  } catch {
    return { text: pathText(p), label: "复制转义路径" };
  }
}
