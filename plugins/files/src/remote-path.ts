export {encode,decode,utf8} from "@sodeck/sdk/bytes";
import {encode,decode,utf8} from "@sodeck/sdk/bytes";
export interface RemotePath {bytesBase64:string}
export function display(bytes: Uint8Array): string {
  let text = "";
  for (let i = 0; i < bytes.length; ) {
    const b = bytes[i];
    if (b < 128) {
      text +=
        b < 32 || b === 127
          ? `\\x${b.toString(16).padStart(2, "0")}`
          : String.fromCharCode(b);
      i++;
      continue;
    }
    const length =
      b >= 194 && b <= 223
        ? 2
        : b >= 224 && b <= 239
          ? 3
          : b >= 240 && b <= 244
            ? 4
            : 0;
    let n = b & ((1 << (7 - length)) - 1);
    let valid = length > 0 && i + length <= bytes.length;
    for (let j = 1; valid && j < length; j++) {
      if ((bytes[i + j] & 192) !== 128) valid = false;
      else n = (n << 6) | (bytes[i + j] & 63);
    }
    valid =
      valid &&
      n >= (length === 2 ? 128 : length === 3 ? 2048 : 65536) &&
      n <= 0x10ffff &&
      !(n >= 0xd800 && n <= 0xdfff);
    if (valid) {
      text +=
        (n >= 128 && n <= 159) ||
        (n >= 0x202a && n <= 0x202e) ||
        (n >= 0x2066 && n <= 0x2069) ||
        n === 0x200e ||
        n === 0x200f
          ? "\\u{" + n.toString(16) + "}"
          : String.fromCodePoint(n);
      i += length;
    } else {
      text += `\\x${b.toString(16).padStart(2, "0")}`;
      i++;
    }
  }
  return text;
}
export const pathText = (p: RemotePath) => display(decode(p.bytesBase64));
export function path(text: string): RemotePath {
  const b = utf8(text);
  if (b[0] !== 47 || b.includes(0) || b.length > 16384)
    throw Error("请输入不含 NUL 的绝对路径");
  return { bytesBase64: encode(b) };
}
export const root = path("/");
export function join(parent: RemotePath, name: RemotePath): RemotePath {
  const p = decode(parent.bytesBase64),
    n = decode(name.bytesBase64);
  if (!n.length || n.includes(0) || n.includes(47) || n.length > 4096)
    throw Error("远端条目名无效");
  return {
    bytesBase64: encode(
      Uint8Array.from([...p, ...(p.length === 1 ? [] : [47]), ...n]),
    ),
  };
}
export function parent(p: RemotePath): RemotePath {
  const b = decode(p.bytesBase64);
  let i = b.length - 1;
  while (i > 0 && b[i] !== 47) i--;
  return { bytesBase64: encode(b.slice(0, Math.max(1, i))) };
}
export function basename(p: RemotePath): string {
  const b = decode(p.bytesBase64);
  return display(b.slice(b.lastIndexOf(47) + 1)) || "/";
}
export function contains(parent: RemotePath, child: RemotePath): boolean {
  const a = decode(parent.bytesBase64),
    b = decode(child.bytesBase64);
  return (
    a.every((v, i) => b[i] === v) &&
    (a.length === 1 || a.length === b.length || b[a.length] === 47)
  );
}
export function ancestors(p: RemotePath): RemotePath[] {
  const out: RemotePath[] = [];
  for (let current = p; ; current = parent(current)) {
    out.unshift(current);
    if (current.bytesBase64 === root.bytesBase64) break;
  }
  return out;
}
export function posixPath(p: RemotePath): string {
  const octal = [...decode(p.bytesBase64)]
    .map((b) => "\\" + b.toString(8).padStart(3, "0"))
    .join("");
  return `p=$(printf '${octal}x'); p=\${p%x}`;
}
