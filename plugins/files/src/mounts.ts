import {
  decode,
  encode,
  display,
  contains,
  type RemotePath,
} from "./remote-path";
export interface Mount {
  id: string;
  parentId: string;
  device: string;
  root: RemotePath;
  target: RemotePath;
  source: string;
  fsType: string;
  options: string[];
  fsOptions: string[];
  readOnly: boolean;
  kind: string;
}
const safeFlags = new Set(
  "rw ro nosuid nodev noexec relatime noatime strictatime nodiratime sync async dirsync mand nomand lazytime acl noacl seclabel user_xattr nouser_xattr".split(
    " ",
  ),
);
const safeValues = new Set(
  "size mode uid gid vers rsize wsize bsize namlen timeo retrans sec proto local_lock subvolid".split(
    " ",
  ),
);
export function safeOptions(raw: string): string[] {
  return raw.split(",").map((option) => {
    const [key, ...v] = option.split("=");
    return safeFlags.has(key) && !v.length
      ? key
      : safeValues.has(key) && v.length && /^[\w.:-]+$/.test(v.join("="))
        ? option
        : `${/^[\w.-]+$/.test(key) ? key : "参数"}=〈已隐藏〉`;
  });
}
export function safeSource(raw: string): string {
  return raw
    .replace(/(\/\/|:\/\/)[^/@\s]*@/g, "$1〈已隐藏〉@")
    .replace(
      /([?&](?:password|token|secret|key|credential)[^=]*=)[^&]*/gi,
      "$1〈已隐藏〉",
    );
}
function bytes(field: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < field.length; i++) {
    if (field[i] === "\\") {
      const oct = field.slice(i + 1, i + 4);
      if (!["040", "011", "012", "134"].includes(oct))
        throw Error("挂载路径转义无效");
      out.push(parseInt(oct, 8));
      i += 3;
    } else out.push(field.charCodeAt(i));
  }
  return Uint8Array.from(out);
}
const remote = (s: string): RemotePath => ({ bytesBase64: encode(bytes(s)) });
export function parseMounts(data: Uint8Array): Mount[] {
  if (data.length > 4 * 1024 * 1024) throw Error("挂载表超过读取上限");
  let text = "";
  for (let i = 0; i < data.length; i += 8192)
    text += String.fromCharCode(...data.subarray(i, i + 8192));
  const lines = text.trimEnd().split("\n");
  if (lines.length > 8192) throw Error("挂载数量超过上限");
  const ids = new Set<string>();
  return lines.filter(Boolean).map((line) => {
    const split = line.split(" - ");
    if (split.length !== 2) throw Error("挂载表格式无效");
    const a = split[0].split(" "),
      b = split[1].split(" ");
    if (
      a.length < 6 ||
      b.length !== 3 ||
      !/^\d+$/.test(a[0]) ||
      !/^\d+$/.test(a[1]) ||
      !/^\d+:\d+$/.test(a[2]) ||
      ids.has(a[0])
    )
      throw Error("挂载标识无效");
    ids.add(a[0]);
    const target = remote(a[4]),
      root = remote(a[3]);
    if (decode(target.bytesBase64)[0] !== 47)
      throw Error("挂载路径不是绝对路径");
    // The kernel may render a filesystem root as a special identifier, e.g.
    // nsfs uses net:[inode]. Only the mount target is an operational path.
    const rootBytes = decode(root.bytesBase64);
    if (!rootBytes.length) throw Error("挂载源内根为空");
    const fsType = display(bytes(b[0])),
      options = safeOptions(a[5]);
    return {
      id: a[0],
      parentId: a[1],
      device: a[2],
      root,
      target,
      source: safeSource(display(bytes(b[1]))),
      fsType,
      options,
      fsOptions: safeOptions(b[2]),
      readOnly: a[5].split(",").includes("ro"),
      kind: ["nfs", "nfs4", "cifs", "smb3", "sshfs"].includes(fsType)
        ? "网络文件系统"
        : [
              "tmpfs",
              "proc",
              "sysfs",
              "devtmpfs",
              "cgroup",
              "cgroup2",
              "devpts",
              "overlay",
              "nsfs",
            ].includes(fsType)
          ? "虚拟文件系统"
          : fsType === "btrfs"
            ? "Btrfs 文件系统 / 子卷"
            : rootBytes[0] !== 47
              ? "文件系统（特殊源内根）"
              : rootBytes.length !== 1
                ? "文件系统子目录（绑定来源待核对）"
                : "本地文件系统",
    };
  });
}
const indexes = new WeakMap<Mount[], Map<string, Mount[]>>();
const byteKey = (path: RemotePath) => {
  const bytes = decode(path.bytesBase64);
  let key = "";
  for (let i = 0; i < bytes.length; i += 4096)
    key += String.fromCharCode(...bytes.subarray(i, i + 4096));
  return key;
};
export function matching(
  mounts: Mount[],
  p: RemotePath,
): { exact: boolean; candidates: Mount[] } {
  let index = indexes.get(mounts);
  if (!index) {
    index = new Map();
    for (const mount of mounts) {
      const key = byteKey(mount.target);
      const list = index.get(key) ?? [];
      list.push(mount);
      index.set(key, list);
    }
    indexes.set(mounts, index);
  }
  const target = byteKey(p);
  let key = target;
  for (;;) {
    const candidates = index.get(key);
    if (candidates) return { exact: key === target, candidates };
    if (key === "/") return { exact: false, candidates: [] };
    key = key.slice(0, key.lastIndexOf("/")) || "/";
  }
}
