import type { FileEntry } from "./types";

export interface RemoteMount {
  key: string;
  target: string;
  source: string;
  fsRoot: string;
  fsType: string;
  readOnly: boolean;
}
export interface RemoteMountUsage {
  mountKey: string;
  status: "ready" | "unavailable" | "unsupported";
  totalBytes: string | null;
  usedBytes: string | null;
  availableBytes: string | null;
  usedPercent: number | null;
  updatedAt: number | null;
  error?: string | null;
}
export interface MountTable {
  version: string;
  status: "ready" | "unsupported";
  mounts: RemoteMount[];
  issues?: MountIssue[];
  message?: string | null;
}
export interface MountIssue {
  kind: "ambiguousStack";
  target: string;
  layers: number;
}
export interface CachedMountUsage {
  value?: RemoteMountUsage;
  loading: boolean;
  stale: boolean;
  error?: string;
}
export interface MountSnapshot {
  sessionId: string;
  table?: MountTable;
  usage: Record<string, CachedMountUsage>;
  fetchedAt: number;
}
export interface MountState extends MountSnapshot {
  loading: boolean;
  error: string;
}
export function mountForFile(file: FileEntry, mounts: RemoteMount[]) {
  return file.isDir && !file.isSymlink
    ? mounts.find((mount) => mount.target === file.path)
    : undefined;
}
export function mountSource(mount: RemoteMount) {
  return mount.source + (mount.fsRoot !== "/" ? `[${mount.fsRoot}]` : "");
}
export function mountKind(mount: RemoteMount) {
  if (
    ["nfs", "nfs4", "cifs", "smb3", "fuse.sshfs", "ceph", "glusterfs"].includes(
      mount.fsType,
    )
  )
    return "network";
  if (["tmpfs", "ramfs", "devtmpfs"].includes(mount.fsType)) return "memory";
  return "disk";
}
export function usageSeverity(percent: number | null | undefined) {
  return percent != null && percent >= 95
    ? "critical"
    : percent != null && percent >= 80
      ? "warning"
      : "normal";
}
export function formatMountBytes(bytes: string | null | undefined) {
  if (bytes == null || !/^\d+$/.test(bytes)) return "—";
  const value = BigInt(bytes);
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let divisor = 1n,
    index = 0;
  while (index < units.length - 1 && value >= divisor * 1024n) {
    divisor *= 1024n;
    index++;
  }
  const tenths = (value * 10n + divisor / 2n) / divisor;
  return `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ""} ${units[index]}`;
}
