import { errorOf } from "./types";
import type { Directory, DirectoryBatch, FileEntry } from "./types";

export interface TerminalDirectory {
  sessionId: string;
  path: string;
  home?: string | null;
  sequence: number;
}
export interface CachedDirectory {
  directory?: Directory;
  error?: string;
  loading: boolean;
  stale?: boolean;
}
export interface RemoteFileTreeSnapshot {
  sessionId: string | null;
  directories: Record<string, Omit<CachedDirectory, "loading">>;
  expanded: string[];
  selected: string;
  selection?: string[];
  selectionAnchor?: string;
  currentPath: string;
  homePath: string;
  terminalCwd?: TerminalDirectory;
  scrollTop: number;
  revealTarget?: string;
}
export interface RemoteFileTreeState extends Omit<
  RemoteFileTreeSnapshot,
  "directories"
> {
  directories: Record<string, CachedDirectory>;
  revealing: boolean;
  revealVersion: number;
  error: string;
}
export interface RemoteDirectoryApi {
  directory: (sessionId: string, path?: string) => Promise<Directory>;
  directoryChain: (
    sessionId: string,
    path?: string,
    knownPaths?: string[],
  ) => Promise<DirectoryBatch>;
}
export interface FileTreeRow {
  file: FileEntry;
  depth: number;
  parent?: string;
  position: number;
  siblings: number;
}

export function normalizeRemotePath(path: string) {
  if (!path.startsWith("/")) return path;
  // Keep `..` for SFTP realpath: /link/.. may have a different parent than
  // the lexical path. Only the remote filesystem can resolve it correctly.
  return (
    "/" +
    path
      .split("/")
      .filter((part) => part && part !== ".")
      .join("/")
  );
}
export function remoteParent(path: string) {
  return path.slice(0, path.lastIndexOf("/")) || "/";
}
export function remotePathChain(path: string) {
  if (!path.startsWith("/") || path.split("/").includes("..")) return [];
  const normalized = normalizeRemotePath(path);
  const chain = ["/"];
  if (normalized !== "/") {
    const parts = normalized.slice(1).split("/");
    for (let i = 1; i <= parts.length; i++)
      chain.push("/" + parts.slice(0, i).join("/"));
  }
  return chain;
}
export function directoryEntry(path: string): FileEntry {
  return {
    name: path === "/" ? "/" : path.slice(path.lastIndexOf("/") + 1),
    path,
    isDir: true,
    size: 0,
    modified: 0,
  };
}

// Flatten only expanded branches. Required ancestors can still be displayed when
// SFTP permits traversal but denies listing one of the parent directories.
export function visibleFileRows(state: RemoteFileTreeState): FileTreeRow[] {
  const rows: FileTreeRow[] = [];
  const expanded = new Set(state.expanded);
  const required = new Set([
    ...state.expanded,
    ...remotePathChain(state.currentPath),
  ]);
  const walk = (
    file: FileEntry,
    depth: number,
    parent: string | undefined,
    position: number,
    siblings: number,
  ) => {
    rows.push({ file, depth, parent, position, siblings });
    if (!file.isDir || !expanded.has(file.path)) return;
    const node = state.directories[file.path];
    const entries = [...(node?.directory?.entries ?? [])];
    if (!node?.directory || node.error) {
      for (const path of required) {
        if (
          path !== "/" &&
          remoteParent(path) === file.path &&
          !entries.some((entry) => entry.path === path)
        )
          entries.push(directoryEntry(path));
      }
    }
    entries.sort(
      (a, b) =>
        Number(b.isDir) - Number(a.isDir) ||
        a.name.localeCompare(b.name, undefined, { numeric: true }),
    );
    entries.forEach((entry, index) =>
      walk(entry, depth + 1, file.path, index + 1, entries.length),
    );
  };
  walk(directoryEntry("/"), 0, undefined, 1, 1);
  return rows;
}

export class RemoteFileTree {
  private state: RemoteFileTreeState;
  private listeners = new Set<() => void>();
  private pending = new Map<string, Promise<void>>();
  private versions = new Map<string, number>();
  private navigation = 0;
  private initialized = false;
  private sessionId: string | null;

  constructor(
    sessionId: string | null,
    private api: RemoteDirectoryApi,
    snapshot?: RemoteFileTreeSnapshot,
  ) {
    this.sessionId = sessionId;
    const saved = snapshot?.sessionId === sessionId ? snapshot : undefined;
    this.state = {
      sessionId,
      directories: Object.fromEntries(
        Object.entries(saved?.directories ?? {}).map(([path, node]) => [
          path,
          { ...node, loading: false },
        ]),
      ),
      expanded: saved?.expanded ?? ["/"],
      selected: saved?.selected ?? "",
      selection: saved?.selection ?? (saved?.selected ? [saved.selected] : []),
      selectionAnchor: saved?.selectionAnchor ?? saved?.selected ?? "",
      currentPath: saved?.currentPath ?? "/",
      homePath: saved?.terminalCwd?.home || saved?.homePath || "",
      terminalCwd: saved?.terminalCwd,
      scrollTop: saved?.scrollTop ?? 0,
      revealTarget: saved?.revealTarget,
      revealing: false,
      revealVersion: 0,
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
  private update(patch: Partial<RemoteFileTreeState>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }
  private updateNode(path: string, patch: Partial<CachedDirectory>) {
    this.update({
      directories: {
        ...this.state.directories,
        [path]: { ...this.state.directories[path], loading: false, ...patch },
      },
    });
  }
  private applyDirectory(path: string, directory: Directory) {
    const children = new Set(
      directory.entries
        .filter((entry) => entry.isDir)
        .map((entry) => entry.path),
    );
    const known = new Set([
      ...Object.keys(this.state.directories),
      ...this.state.expanded,
      ...remotePathChain(this.state.currentPath),
    ]);
    const removed = [...known].filter(
      (item) =>
        item !== "/" && remoteParent(item) === path && !children.has(item),
    );
    const gone = (item: string) =>
      removed.some((root) => item === root || item.startsWith(root + "/"));
    const directories = {
      ...this.state.directories,
      [path]: { directory: { ...directory, path }, loading: false },
    };
    for (const item of Object.keys(directories)) {
      if (gone(item)) {
        delete directories[item];
        this.nextVersion(item);
        this.pending.delete(item);
      }
    }
    let currentPath = this.state.currentPath;
    if (gone(currentPath)) currentPath = path;
    const selected =
      gone(this.state.selected) ||
      (this.state.selected &&
        remoteParent(this.state.selected) === path &&
        this.state.selected !== "/" &&
        !directory.entries.some((entry) => entry.path === this.state.selected))
        ? path
        : this.state.selected;
    const cancel = this.state.revealTarget && gone(this.state.revealTarget);
    if (cancel) this.navigation++;
    this.update({
      directories,
      expanded: this.state.expanded.filter((item) => !gone(item)),
      selected,
      selection: (this.state.selection ?? []).filter(
        (p) =>
          !gone(p) &&
          !(
            remoteParent(p) === path &&
            p !== "/" &&
            !directory.entries.some((e) => e.path === p)
          ),
      ),
      currentPath,
      ...(cancel ? { revealing: false, revealTarget: undefined } : {}),
    });
  }
  private nextVersion(path: string) {
    const version = (this.versions.get(path) ?? 0) + 1;
    this.versions.set(path, version);
    return version;
  }

  snapshot(): RemoteFileTreeSnapshot {
    const {
      sessionId,
      expanded,
      selected,
      currentPath,
      homePath,
      terminalCwd,
      scrollTop,
      revealTarget,
    } = this.state;
    return {
      sessionId,
      expanded,
      selected,
      currentPath,
      homePath,
      terminalCwd,
      scrollTop,
      revealTarget,
      selection: [...(this.state.selection ?? [])],
      selectionAnchor: this.state.selectionAnchor,
      directories: Object.fromEntries(
        Object.entries(this.state.directories).map(
          ([path, { directory, error, stale }]) => [
            path,
            { directory, error, stale },
          ],
        ),
      ),
    };
  }

  invalidateRemotePath(path: string, active: boolean) {
    for (const [key, node] of Object.entries(this.state.directories)) {
      if (key !== path && node.directory?.resolvedPath !== path) continue;
      this.nextVersion(key);
      this.updateNode(key, { stale: true });
      if (active && this.state.expanded.includes(key)) void this.refresh(key);
    }
  }
  activate() {
    for (const path of this.state.expanded) {
      if (this.state.directories[path]?.stale) void this.refresh(path);
    }
    if (!this.sessionId) return;
    if (!this.initialized) {
      this.initialized = true;
      if (this.state.revealing) return;
      if (this.state.revealTarget)
        void this.revealPath(this.state.revealTarget);
      else if (
        !this.state.homePath ||
        Object.keys(this.state.directories).length === 0
      )
        void this.revealPath(this.state.homePath || undefined);
      else {
        // A transferred window never serializes in-flight requests. Resume
        // expanded nodes that had no completed directory response yet.
        for (const path of this.state.expanded) {
          const node = this.state.directories[path];
          if (!node?.directory && !node?.error) void this.loadDirectory(path);
        }
      }
    } else if (this.state.revealTarget && !this.state.revealing)
      void this.revealPath(this.state.revealTarget);
  }

  acceptTerminalDirectory(cwd: TerminalDirectory, active: boolean) {
    if (cwd.sessionId !== this.sessionId || !cwd.path.startsWith("/")) return;
    if (
      this.state.terminalCwd &&
      cwd.sequence <= this.state.terminalCwd.sequence
    )
      return;
    const next = { ...cwd, path: normalizeRemotePath(cwd.path) };
    const unchanged = this.state.terminalCwd?.path === next.path;
    this.update({
      terminalCwd: next,
      ...(next.home?.startsWith("/")
        ? { homePath: normalizeRemotePath(next.home) }
        : {}),
    });
    if (unchanged) return;
    if (active) void this.revealPath(next.path);
    else {
      this.navigation++;
      this.update({ revealTarget: next.path, revealing: false });
    }
  }

  select(
    path: string,
    isDirectory = false,
    mode: "single" | "toggle" | "range" = "single",
  ) {
    const cancel = this.state.revealing || !!this.state.revealTarget;
    if (cancel) this.navigation++;
    let selection = [path];
    if (mode === "toggle") {
      const selected = new Set(this.state.selection ?? []);
      if (selected.has(path)) selected.delete(path);
      else selected.add(path);
      selection = [...selected];
    } else if (mode === "range") {
      const paths = visibleFileRows(this.state).map((row) => row.file.path);
      const from = paths.indexOf(
        this.state.selectionAnchor || this.state.selected,
      );
      const to = paths.indexOf(path);
      if (from >= 0 && to >= 0)
        selection = paths.slice(Math.min(from, to), Math.max(from, to) + 1);
    }
    this.update({
      selected: path,
      selection,
      selectionAnchor: mode === "range" ? this.state.selectionAnchor : path,
      ...(isDirectory && mode === "single" ? { currentPath: path } : {}),
      ...(cancel ? { revealing: false, revealTarget: undefined } : {}),
    });
  }
  setScrollTop(scrollTop: number) {
    if (scrollTop !== this.state.scrollTop) this.update({ scrollTop });
  }
  collapse(path: string) {
    const target = this.state.revealTarget;
    const cancel =
      this.state.revealing &&
      (!target ||
        target === path ||
        target.startsWith(path === "/" ? "/" : path + "/"));
    if (cancel) this.navigation++;
    this.update({
      expanded: this.state.expanded.filter((item) => item !== path),
      ...(cancel ? { revealing: false, revealTarget: undefined } : {}),
    });
  }
  async expand(path: string) {
    if (!this.state.expanded.includes(path))
      this.update({ expanded: [...this.state.expanded, path] });
    await this.loadDirectory(path);
  }
  async refresh(path = this.state.currentPath) {
    await this.loadDirectory(path, true);
  }

  private async loadDirectory(path: string, force = false): Promise<void> {
    if (!this.sessionId) return;
    const existing = this.pending.get(path);
    if (existing) {
      await existing;
      if (!force && !this.state.directories[path]?.stale) return;
    }
    if (
      !force &&
      this.state.directories[path]?.directory &&
      !this.state.directories[path]?.stale
    )
      return;
    const version = this.nextVersion(path);
    this.updateNode(path, { loading: true, error: undefined });
    const request = this.api
      .directory(this.sessionId, path)
      .then((directory) => {
        if (version !== this.versions.get(path)) return;
        this.applyDirectory(path, directory);
      })
      .catch((error: unknown) => {
        if (version === this.versions.get(path))
          this.updateNode(path, {
            loading: false,
            error: errorOf(error).message,
          });
      })
      .finally(() => {
        if (this.pending.get(path) === request) this.pending.delete(path);
      });
    this.pending.set(path, request);
    await request;
  }

  private async readChain(
    path?: string,
  ): Promise<{ path?: string; error?: string }> {
    if (!this.sessionId) return {};
    const chain = path ? remotePathChain(path) : ["/"];
    // Wait for shared ancestors before batching missing paths, so simultaneous
    // navigation and manual expansion never request the same directory twice.
    await Promise.all([
      ...new Set(
        chain
          .map((item) => this.pending.get(item))
          .filter((request): request is Promise<void> => !!request),
      ),
    ]);
    // A loaded parent may predate `mkdir new && cd new` or a file being
    // replaced by a directory. Refresh only those stale edges in this batch.
    const staleParents = new Set(
      chain.slice(0, -1).filter((parent, index) => {
        const cached = this.state.directories[parent]?.directory;
        return (
          cached &&
          !cached.entries.some(
            (entry) => entry.path === chain[index + 1] && entry.isDir,
          )
        );
      }),
    );
    const known = Object.entries(this.state.directories)
      .filter(
        ([item, node]) =>
          node.directory &&
          !staleParents.has(item) &&
          (!path || chain.length === 0 || chain.includes(item)),
      )
      .map(([item]) => item)
      .slice(-4096);
    const missing = chain.filter(
      (item) =>
        !this.state.directories[item]?.directory || staleParents.has(item),
    );
    if (
      path?.startsWith("/") &&
      chain.length > 0 &&
      missing.length === 0 &&
      this.state.homePath
    )
      return { path };
    const versions = new Map(
      missing.map((item) => [item, this.nextVersion(item)]),
    );
    const directories = { ...this.state.directories };
    for (const item of missing)
      directories[item] = {
        ...directories[item],
        loading: true,
        error: undefined,
      };
    this.update({ directories });
    let resultPath: string | undefined;
    let failure: string | undefined;
    const request = this.api
      .directoryChain(this.sessionId, path, known)
      .then((result) => {
        resultPath = normalizeRemotePath(result.path);
        const next = { ...this.state.directories };
        for (const item of result.directories) {
          const expected = versions.get(item.path);
          if (
            expected !== undefined &&
            expected !== this.versions.get(item.path)
          )
            continue;
          // Paths only discovered by the home lookup may have a newer individual
          // request. Leave that request in control of its node.
          if (expected === undefined && this.pending.has(item.path)) continue;
          next[item.path] = {
            directory: item.directory,
            error: item.error,
            loading: false,
          };
        }
        for (const item of missing) {
          if (
            next[item]?.loading &&
            versions.get(item) === this.versions.get(item)
          )
            next[item] = {
              ...next[item],
              loading: false,
              error: "服务器未返回此目录",
            };
        }
        this.update({
          homePath: this.state.terminalCwd?.home || result.homePath,
          directories: next,
        });
      })
      .catch((error: unknown) => {
        const message = errorOf(error).message;
        failure = message;
        const next = { ...this.state.directories };
        for (const item of missing) {
          if (versions.get(item) === this.versions.get(item))
            next[item] = { ...next[item], loading: false, error: message };
        }
        this.update({ directories: next });
      })
      .finally(() => {
        for (const item of missing)
          if (this.pending.get(item) === request) this.pending.delete(item);
      });
    for (const item of missing) this.pending.set(item, request);
    await request;
    return { path: resultPath, error: failure };
  }

  async revealPath(path?: string) {
    if (!this.sessionId) return;
    const token = ++this.navigation;
    const home = this.state.terminalCwd?.home || this.state.homePath;
    const expandedHome =
      home && path === "~"
        ? home
        : home && path?.startsWith("~/")
          ? home.replace(/\/$/, "") + path.slice(1)
          : path;
    const target = expandedHome ? normalizeRemotePath(expandedHome) : undefined;
    this.update({ revealTarget: target, revealing: true, error: "" });
    const result = await this.readChain(target);
    if (token !== this.navigation) return;
    if (result.path) {
      this.update({
        expanded: [
          ...new Set([...this.state.expanded, ...remotePathChain(result.path)]),
        ],
        selected: result.path,
        selection: [result.path],
        selectionAnchor: result.path,
        currentPath: result.path,
        revealVersion: this.state.revealVersion + 1,
        revealTarget: undefined,
        revealing: false,
      });
    } else
      this.update({
        revealTarget: undefined,
        revealing: false,
        error: result.error || "无法定位目录，请重试",
      });
  }
}
