// Byte paths are the identity. Display strings never become operation targets.
export interface RemotePath {
  bytesBase64: string;
}
const alphabet =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
export function encode(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n =
      (bytes[i] << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    out +=
      alphabet[n >>> 18] +
      alphabet[(n >>> 12) & 63] +
      (i + 1 < bytes.length ? alphabet[(n >>> 6) & 63] : "=") +
      (i + 2 < bytes.length ? alphabet[n & 63] : "=");
  }
  return out;
}
export function decode(value: string): Uint8Array {
  if (
    value.length % 4 ||
    !/^([A-Za-z0-9+/]{4})*([A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)
  )
    throw Error("无效的路径编码");
  const out: number[] = [];
  for (let i = 0; i < value.length; i += 4) {
    const n =
      (alphabet.indexOf(value[i]) << 18) |
      (alphabet.indexOf(value[i + 1]) << 12) |
      ((alphabet.indexOf(value[i + 2]) & 63) << 6) |
      (alphabet.indexOf(value[i + 3]) & 63);
    out.push((n >>> 16) & 255);
    if (value[i + 2] !== "=") out.push((n >>> 8) & 255);
    if (value[i + 3] !== "=") out.push(n & 255);
  }
  return Uint8Array.from(out);
}
export function utf8(text: string): Uint8Array {
  const out: number[] = [];
  for (const ch of text) {
    const n = ch.codePointAt(0)!;
    if (n < 128) out.push(n);
    else if (n < 2048) out.push(192 | (n >>> 6), 128 | (n & 63));
    else if (n < 65536)
      out.push(224 | (n >>> 12), 128 | ((n >>> 6) & 63), 128 | (n & 63));
    else
      out.push(
        240 | (n >>> 18),
        128 | ((n >>> 12) & 63),
        128 | ((n >>> 6) & 63),
        128 | (n & 63),
      );
  }
  return Uint8Array.from(out);
}
