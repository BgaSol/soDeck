import { errorOf } from "./types";
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
  message?: string | null;
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
export interface RemoteMountApi {
  list: (sessionId: string) => Promise<MountTable>;
  usage: (
    sessionId: string,
    version: string,
    keys: string[],
  ) => Promise<RemoteMountUsage[]>;
}
export const MOUNT_REFRESH_MS = 30_000;

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

// Separate from directory loading: mount errors cannot change navigation or selection.
export class RemoteMounts {
  private state: MountState;
  private listeners = new Set<() => void>();
  private active = false;
  private epoch = 0;
  private visible = new Set<string>();
  private running = new Set<string>();
  private failures = new Map<string, number>();
  private nextAttempt = new Map<string, number>();
  private tableAttempt = 0;
  private pendingTable: Promise<void> | undefined;
  constructor(
    private sessionId: string,
    private api: RemoteMountApi,
    snapshot?: MountSnapshot,
    private now = Date.now,
  ) {
    const saved = snapshot?.sessionId === sessionId ? snapshot : undefined;
    this.state = {
      sessionId,
      table: saved?.table,
      usage: Object.fromEntries(
        Object.entries(saved?.usage ?? {}).map(([key, value]) => [
          key,
          { ...value, loading: false, stale: true },
        ]),
      ),
      fetchedAt: saved?.fetchedAt ?? 0,
      loading: false,
      error: "",
    };
  }
  getState = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(patch: Partial<MountState>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((l) => l());
  }
  private cache(key: string, patch: Partial<CachedMountUsage>) {
    this.update({
      usage: {
        ...this.state.usage,
        [key]: {
          ...(this.state.usage[key] ?? { loading: false, stale: false }),
          ...patch,
        },
      },
    });
  }
  snapshot(): MountSnapshot {
    return {
      sessionId: this.sessionId,
      table: this.state.table,
      usage: this.state.usage,
      fetchedAt: this.state.fetchedAt,
    };
  }
  setActive(active: boolean) {
    if (active === this.active) return;
    this.active = active;
    if (active) {
      void this.refresh();
      this.pump();
    } else {
      this.epoch++;
      this.pendingTable = undefined;
      this.running.clear();
      this.update({
        loading: false,
        usage: Object.fromEntries(
          Object.entries(this.state.usage).map(([k, v]) => [
            k,
            { ...v, loading: false },
          ]),
        ),
      });
    }
  }
  setVisible(paths: string[]) {
    this.visible = new Set(paths);
    this.pump();
  }
  refresh = (force = false): Promise<void> => {
    if (
      !this.active ||
      this.pendingTable ||
      (!force && this.now() < this.tableAttempt)
    )
      return this.pendingTable ?? Promise.resolve();
    const epoch = this.epoch;
    if (force) {
      this.nextAttempt.clear();
      this.failures.clear();
    }
    this.update({ loading: true });
    const request = this.api
      .list(this.sessionId)
      .then((table) => {
        if (epoch !== this.epoch || !this.active) return;
        const changed = table.version !== this.state.table?.version;
        if (changed) {
          this.nextAttempt.clear();
          this.failures.clear();
        }
        this.tableAttempt =
          this.now() +
          (table.status === "unsupported" ? 300_000 : MOUNT_REFRESH_MS);
        this.update({
          table,
          fetchedAt: this.now(),
          usage: changed ? {} : this.state.usage,
          error: "",
        });
        if (force) {
          this.update({
            usage: Object.fromEntries(
              Object.entries(this.state.usage).map(([k, v]) => [
                k,
                { ...v, stale: true },
              ]),
            ),
          });
        }
      })
      .catch((error) => {
        if (epoch !== this.epoch || !this.active) return;
        this.tableAttempt = this.now() + MOUNT_REFRESH_MS;
        this.update({
          error: errorOf(error).message,
          usage: Object.fromEntries(
            Object.entries(this.state.usage).map(([k, v]) => [
              k,
              { ...v, stale: true },
            ]),
          ),
        });
      })
      .finally(() => {
        if (epoch !== this.epoch) return;
        this.pendingTable = undefined;
        this.update({ loading: false });
        this.pump();
      });
    this.pendingTable = request;
    return request;
  };
  private pump() {
    if (
      !this.active ||
      this.state.loading ||
      this.state.error ||
      this.state.table?.status !== "ready"
    )
      return;
    const { version, mounts } = this.state.table;
    // New/oldest entries go first so slow network mounts cannot starve later rows.
    const ordered = [...mounts].sort(
      (a, b) =>
        (this.nextAttempt.get(a.key) ?? 0) - (this.nextAttempt.get(b.key) ?? 0),
    );
    for (const mount of ordered) {
      if (this.running.size >= 2) break;
      if (!this.visible.has(mount.target) || this.running.has(mount.key))
        continue;
      const cached = this.state.usage[mount.key];
      if (this.now() < (this.nextAttempt.get(mount.key) ?? 0)) continue;
      if (cached?.value?.status === "unsupported" && !cached.stale) continue;
      if (
        !cached?.stale &&
        cached?.value?.updatedAt != null &&
        this.now() - cached.value.updatedAt < MOUNT_REFRESH_MS
      )
        continue;
      const epoch = this.epoch;
      this.running.add(mount.key);
      this.cache(mount.key, { loading: true });
      void this.api
        .usage(this.sessionId, version, [mount.key])
        .then((results) => {
          if (epoch !== this.epoch || version !== this.state.table?.version)
            return;
          const value = results.find((r) => r.mountKey === mount.key);
          if (!value) throw new Error("未返回挂载容量");
          if (value.status === "unavailable")
            throw new Error(value.error || "容量暂不可用");
          this.failures.delete(mount.key);
          this.nextAttempt.set(mount.key, this.now() + MOUNT_REFRESH_MS);
          this.cache(mount.key, {
            value,
            loading: false,
            stale: false,
            error: undefined,
          });
        })
        .catch((error) => {
          if (epoch !== this.epoch || version !== this.state.table?.version)
            return;
          const failure = errorOf(error);
          const count = (this.failures.get(mount.key) ?? 0) + 1;
          this.failures.set(mount.key, count);
          this.nextAttempt.set(
            mount.key,
            this.now() + Math.min(300_000, MOUNT_REFRESH_MS * 2 ** (count - 1)),
          );
          this.cache(mount.key, {
            loading: false,
            stale: true,
            error: failure.message,
          });
          if (failure.kind === "mountChanged") {
            // Hide an invalid table immediately, then rediscover on the next tick/manual refresh.
            this.update({
              table: undefined,
              usage: {},
              error: failure.message,
            });
            this.tableAttempt = this.now() + MOUNT_REFRESH_MS;
          }
        })
        .finally(() => {
          if (epoch !== this.epoch) return;
          this.running.delete(mount.key);
          this.pump();
        });
    }
  }
}
