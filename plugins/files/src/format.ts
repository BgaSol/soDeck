export function size(value: string | null | undefined): string {
  if (value == null || !/^\d+$/.test(value)) return "未知";
  let n = BigInt(value),
    unit = 0,
    scale = 1n;
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  while (n >= scale * 1024n && unit < units.length - 1) {
    scale *= 1024n;
    unit++;
  }
  if (!unit) return `${n} B`;
  const tenths = (n * 10n + scale / 2n) / scale;
  return `${tenths / 10n}${tenths % 10n ? "." + (tenths % 10n) : ""} ${units[unit]}`;
}
export interface Capacity {
  total: string;
  used: string;
  free: string | null;
  available: string | null;
  percent: string | null;
  source: "stat" | "df";
}
const percent = (used: bigint, total: bigint) => total
  ? `${(used * 1000n) / total / 10n}.${((used * 1000n) / total) % 10n}%`
  : null;
export function capacity(text: string): Capacity {
  const fields = text.trim().split(/\s+/);
  if (fields.length !== 4 || fields.some((f) => !/^\d+$/.test(f)))
    throw Error("容量统计无效");
  const [fragment, blocks, free, available] = fields.map(BigInt);
  if (fragment <= 0n || free > blocks || available > free)
    throw Error("容量统计无效");
  const total = fragment * blocks,
    used = fragment * (blocks - free);
  return {
    total: String(total),
    used: String(used),
    free: String(fragment * free),
    available: String(fragment * available),
    percent: percent(used, total),
    source: "stat",
  };
}

// df has no byte-safe machine format. Match the verified mount's raw byte
// suffix first, then numeric columns from the right; reject ambiguous output.
export function dfCapacity(output: string, mountTarget: string): Capacity {
  const lines = output.split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 2 || /[\r\n\0]/.test(mountTarget) ||
      !mountTarget.startsWith("/") ||
      !/^Filesystem\s+1024-blocks\s+Used\s+Available\s+(?:Capacity|Use%)\s+Mounted on$/.test(lines[0].trim()) ||
      !lines[1].endsWith(" " + mountTarget))
    throw Error("容量统计格式或挂载位置无法确认");
  const values = lines[1].slice(0, -mountTarget.length).trimEnd()
    .match(/^(.+)\s+(\d+)\s+(\d+)\s+(-?\d+|-)\s+(?:\d+%|-)$/);
  if (!values) throw Error("容量统计格式无法确认");
  const total = BigInt(values[2]), used = BigInt(values[3]);
  const available = /^\d+$/.test(values[4]) ? BigInt(values[4]) : null;
  if (used > total || (available !== null && available > total))
    throw Error("容量统计无效");
  return {
    total: String(total * 1024n), used: String(used * 1024n),
    free: null, available: available === null ? null : String(available * 1024n),
    percent: percent(used, total), source: "df",
  };
}
