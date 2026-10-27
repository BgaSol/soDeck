import { Remote, fault } from "./remote";
import {
  path,
  pathText,
  encode,
  decode,
  posixPath,
  root,
  join,
  type RemotePath,
} from "./remote-path";
import { parseMounts, matching, type Mount } from "./mounts";
import { capacity, dfCapacity, type Capacity } from "./format";
export interface MountSnapshot {
  mounts: Mount[];
  raw: Uint8Array;
  observedAt: number;
  pid: string;
  start: string;
}
const marker = "\nSODECK_FILES_SECTION\n";
function text(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 8192)
    out += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return out;
}
function processIdentity(raw: string): { pid: string; start: string } {
  const p = raw.indexOf(" ("),
    end = raw.lastIndexOf(") ");
  const pid = raw.slice(0, p),
    start = raw
      .slice(end + 2)
      .trim()
      .split(/\s+/)[19];
  if (!/^\d+$/.test(pid) || !/^\d+$/.test(start ?? ""))
    throw fault("MOUNT_UNAVAILABLE", "无法核对 SFTP 进程身份");
  return { pid, start };
}
function fdIdentity(raw: string) {
  const inode = raw.match(/^ino:\s*(\d+)$/m)?.[1];
  const mountId = raw.match(/^mnt_id:\s*(\d+)$/m)?.[1];
  if (!inode || !mountId)
    throw fault("MOUNT_UNAVAILABLE", "无法核对根目录句柄");
  return { inode, mountId };
}
export async function mounts(remote: Remote): Promise<MountSnapshot> {
  const rootHandle = await remote.openDirectory(root);
  try {
    return await verifiedMounts(remote);
  } finally {
    await remote.close(rootHandle.directoryId);
  }
}
async function verifiedMounts(remote: Remote): Promise<MountSnapshot> {
  const before = processIdentity(
    text(await remote.readFile(path("/proc/self/stat"), 65536)),
  );
  const raw = await remote.readFile(path("/proc/self/mountinfo"));
  const parsed = parseMounts(raw);
  const ns = pathText(
    (
      await remote.call<{ path: RemotePath }>("files.readLink", {
        path: path("/proc/self/ns/mnt"),
      })
    ).path,
  );
  const directory = await remote.openDirectory(path("/proc/self/fd"));
  let rootInfo:
    | { path: RemotePath; linkPath: RemotePath; inode: string; mountId: string }
    | undefined;
  try {
    const page = await remote.page(directory.directoryId, 0);
    if (!page.eof)
      throw fault("MOUNT_UNAVAILABLE", "进程描述符数量超过核对范围");
    for (const entry of page.entries) {
      const fd = pathText(entry.name);
      if (!/^\d+$/.test(fd)) continue;
      const link = await remote
        .call<{
          path: RemotePath;
        }>("files.readLink", { path: join(path("/proc/self/fd"), entry.name) })
        .catch(() => null);
      if (!link || pathText(link.path) !== "/") continue;
      const infoPath = path("/proc/self/fdinfo/" + fd),
        info = text(await remote.readFile(infoPath, 65536));
      const inode = info.match(/^ino:\s*(\d+)$/m)?.[1],
        mountId = info.match(/^mnt_id:\s*(\d+)$/m)?.[1];
      const mount = parsed.find(
        (m) => m.id === mountId && m.target.bytesBase64 === root.bytesBase64,
      );
      if (inode && mountId && mount) {
        rootInfo = {
          path: infoPath,
          linkPath: join(path("/proc/self/fd"), entry.name),
          inode,
          mountId,
        };
        break;
      }
    }
  } finally {
    await remote.close(directory.directoryId);
  }
  if (!rootInfo) throw fault("MOUNT_UNAVAILABLE", "无法核对 SFTP 根目录句柄");
  const { pid } = before,
    sep = "printf '\\nSODECK_FILES_SECTION\\n'";
  const command =
    // Keep the shell's root open for the whole observation, just like SFTP's.
    // In the same mount namespace, (mount ID, inode) identifies the same root
    // without requiring GNU stat or interpreting architecture-specific dev_t.
    "LC_ALL=C; export LC_ALL; exec 9< / && " +
    [
      "cat /proc/" + pid + "/stat",
      "readlink /proc/$$/ns/mnt",
      "readlink /proc/$$/fd/9",
      "cat /proc/$$/fdinfo/9",
      "cat /proc/" + pid + "/mountinfo",
      "cat /proc/$$/mountinfo",
      "cat /proc/$$/fdinfo/9",
      "readlink /proc/$$/ns/mnt",
      "cat /proc/" + pid + "/stat",
    ].join(" && " + sep + " && ");
  const pieces = text(await remote.exec(command)).split(marker);
  if (pieces.length !== 9)
    throw fault("MOUNT_UNAVAILABLE", "无法核对目录与挂载的路径视图");
  const a = processIdentity(pieces[0]),
    b = processIdentity(pieces[8]);
  const after = processIdentity(
    text(await remote.readFile(path("/proc/self/stat"), 65536)),
  );
  const rootAfter = fdIdentity(text(await remote.readFile(rootInfo.path, 65536)));
  const execRoot = fdIdentity(pieces[3]), execAfter = fdIdentity(pieces[6]);
  const nsAfter = pathText((await remote.call<{ path: RemotePath }>(
    "files.readLink", { path: path("/proc/self/ns/mnt") },
  )).path);
  const rootLinkAfter = pathText((await remote.call<{ path: RemotePath }>(
    "files.readLink", { path: rootInfo.linkPath },
  )).path);
  const rawAfter = await remote.readFile(path("/proc/self/mountinfo"));
  if (
    [a, b, after].some((x) => x.pid !== pid || x.start !== before.start) ||
    !/^mnt:\[\d+\]$/.test(ns) ||
    pieces[1].trim() !== ns ||
    pieces[7].trim() !== ns || nsAfter !== ns ||
    pieces[2].trim() !== "/" || rootLinkAfter !== "/" ||
    [execRoot, execAfter, rootAfter].some(
      (identity) => identity.inode !== rootInfo.inode || identity.mountId !== rootInfo.mountId,
    ) ||
    pieces[4] !== text(raw) || pieces[5] !== text(raw) ||
    encode(rawAfter) !== encode(raw)
  )
    throw fault(
      "MOUNT_UNAVAILABLE",
      "目录与执行环境的根或挂载命名空间无法确认一致",
    );
  return {
    mounts: parsed,
    raw,
    observedAt: Date.now(),
    pid,
    start: before.start,
  };
}
export async function mountCapacity(
  remote: Remote,
  p: RemotePath,
  snapshot: MountSnapshot,
): Promise<Capacity> {
  const before = await remote.readFile(path("/proc/self/mountinfo"));
  if (encode(before) !== encode(snapshot.raw))
    throw fault("MOUNT_CHANGED", "挂载已变化，请刷新后重试");
  const command =
    "LC_ALL=C; export LC_ALL; " +
    posixPath(p) +
    "; if values=$(stat -f -c '%S %b %f %a' -- \"$p\" 2>/dev/null); " +
    "then printf 'SODECK_STAT\\n%s\\n' \"$values\"; " +
    "else printf 'SODECK_DF\\n'; df -Pk -- \"$p\"; fi";
  const output = text(await remote.exec(command, 10000));
  let result: Capacity;
  if (output.startsWith("SODECK_STAT\n")) {
    result = capacity(output.slice("SODECK_STAT\n".length));
  } else if (output.startsWith("SODECK_DF\n")) {
    const match = matching(snapshot.mounts, p);
    if (match.candidates.length !== 1)
      throw fault("MOUNT_UNAVAILABLE", "无法确定容量所属挂载");
    result = dfCapacity(
      output.slice("SODECK_DF\n".length),
      text(decode(match.candidates[0].target.bytesBase64)),
    );
  } else throw fault("MOUNT_UNAVAILABLE", "无法识别容量统计格式");
  const after = await remote.readFile(path("/proc/self/mountinfo"));
  if (encode(after) !== encode(snapshot.raw))
    throw fault("MOUNT_CHANGED", "统计期间挂载发生变化，请刷新");
  return result;
}
