import {
  Fragment,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowUp,
  ChevronDown,
  ChevronRight,
  Link,
  Download,
  Folder,
  FileText,
  Pencil,
  RefreshCw,
  X,
  Upload,
  FilePlus2,
  FolderPlus,
  ArrowRightLeft,
  Copy,
  Scissors,
  ClipboardPaste,
  HardDrive,
  Network,
  MemoryStick,
  ClipboardCopy,
  Info,
  Plus,
  Trash2,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { transferApi } from "./transferApi";
import type { RemoteClipboard } from "./transferApi";
import { DeleteFilesDialog } from "./DeleteFilesDialog";
import type { DeletePreview } from "./fileApi";
import { FileOperationDialog } from "./FileOperationDialog";
import { CopyTargetDialog } from "./CopyTargetDialog";
import type { FileOperation } from "./FileOperationDialog";
import {
  useDesktopDrag,
  useNativeDragStart,
  fileDropTarget,
} from "./nativeDrag";
import { fileApi, notifyDownloadStarted } from "./fileApi";
import type { DownloadConflict } from "./fileApi";
import { errorOf, formatSize } from "./types";
import type { FileEntry } from "./types";
import { remoteParent, visibleFileRows } from "./remoteFileTree";
import type {
  RemoteFileTreeSnapshot,
  TerminalDirectory,
} from "./remoteFileTree";
import { useRemoteFileTree } from "./useRemoteFileTree";
import { useRemoteMounts } from "./useRemoteMounts";
import {
  formatMountBytes,
  mountForFile,
  mountKind,
  mountSource,
  usageSeverity,
} from "./remoteMounts";
import type { MountSnapshot, RemoteMount } from "./remoteMounts";
import { MountDetailsPopover } from "./MountDetailsPopover";
import "./files.css";

export interface FilePanelSnapshot extends RemoteFileTreeSnapshot {
  mounts?: MountSnapshot;
}

export function FilePanel({
  sessionId,
  serverName,
  active = true,
  initialState,
  onStateChange,
  terminalCwd,
  revealRequest,
}: {
  sessionId: string | null;
  serverName: string;
  active?: boolean;
  initialState?: FilePanelSnapshot;
  onStateChange?: (state: FilePanelSnapshot) => void;
  terminalCwd?: TerminalDirectory;
  revealRequest?: { path: string; sequence: number };
}) {
  const { model, state } = useRemoteFileTree(
    sessionId,
    active,
    initialState,
    terminalCwd,
  );
  const appliedReveal = useRef(0);
  useEffect(() => {
    if (
      !active ||
      !sessionId ||
      !revealRequest ||
      appliedReveal.current === revealRequest.sequence
    )
      return;
    appliedReveal.current = revealRequest.sequence;
    void model.revealPath(revealRequest.path);
  }, [active, sessionId, revealRequest, model]);
  const { selected } = state;
  const selection = state.selection ?? (selected ? [selected] : []);
  const panel = useRef<HTMLElement>(null);
  const [operation, setOperation] = useState<FileOperation | null>(null);
  const [deleteDraft, setDeleteDraft] = useState<DeletePreview | null>(null);
  const preparingDelete = useRef(false);
  const [clipboard, setClipboard] = useState<RemoteClipboard | null>(null);
  const [copyPaths, setCopyPaths] = useState<string[] | null>(null);
  const viewRegistration = useRef<{ id: string; update: () => void } | null>(
    null,
  );
  const currentPath = useRef(state.currentPath);
  currentPath.current = state.currentPath;
  const [dropPath, setDropPath] = useState<string | null>(null);
  const [dropKind, setDropKind] = useState("remoteFiles");
  const hover = useRef<{ path: string; since: number } | null>(null);
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;
  const [path, setPath] = useState(state.currentPath);
  const [focusedPath, setFocusedPath] = useState(selected || "/");
  const list = useRef<HTMLDivElement>(null);
  const contextMenu = useRef<HTMLDivElement>(null);
  const rowElements = useRef(new Map<string, HTMLDivElement>());
  const rows = useMemo(() => visibleFileRows(state), [state]);
  const mountPaths = useMemo(
    () =>
      rows
        .filter(({ file }) => file.isDir && !file.isSymlink)
        .map(({ file }) => file.path),
    [rows],
  );
  const mounts = useRemoteMounts(
    sessionId,
    active,
    mountPaths,
    initialState?.mounts,
  );
  const visibleMountIssues = useMemo(() => {
    const visible = new Set(mountPaths);
    return (mounts.state.table?.issues ?? []).filter(
      (issue) => issue.kind === "ambiguousStack" && visible.has(issue.target),
    );
  }, [mountPaths, mounts.state.table?.issues]);
  const mountNotice =
    mounts.state.table?.status === "unsupported"
      ? mounts.state.table.message
      : visibleMountIssues.length === 1
        ? `无法确认 ${visibleMountIssues[0].target} 当前可见的挂载层，暂不展示该路径容量`
        : visibleMountIssues.length > 1
          ? `当前目录中有 ${visibleMountIssues.length} 个挂载点层级无法确认，暂不展示容量`
          : null;
  const [mountDetail, setMountDetail] = useState<{
    key: string;
    path: string;
    anchor: HTMLElement;
  } | null>(null);
  const detailMount = mounts.state.table?.mounts.find(
    (mount) =>
      mount.key === mountDetail?.key && mount.target === mountDetail.path,
  );
  const [pathNotice, setPathNotice] = useState<{
    message: string;
    error?: boolean;
  } | null>(null);
  const expanded = useMemo(() => new Set(state.expanded), [state.expanded]);
  const loading =
    state.revealing ||
    Object.values(state.directories).some((node) => node.loading);
  const [error, setError] = useState("");
  const [working, setBusy] = useState(false);
  const busy = working || !!deleteDraft;
  const [menu, setMenu] = useState<{
    file: FileEntry;
    blank?: boolean;
    create?: boolean;
    anchor?: HTMLButtonElement;
    x: number;
    y: number;
  } | null>(null);
  const [download, setDownload] = useState<FileEntry | null>(null);
  const [conflict, setConflict] = useState<DownloadConflict>("rename");
  const downloadTitle = useId();
  const downloadDialog = useRef<HTMLDialogElement>(null);
  const downloadReturn = useRef<{
    path: string;
    session: string | null;
  } | null>(null);
  const choosingDownload = useRef(false);
  function beginDownload(file: FileEntry) {
    downloadReturn.current = { path: file.path, session: sessionId };
    setMenu(null);
    setDownload(file);
  }
  function restoreDownloadFocus() {
    const origin = downloadReturn.current;
    if (
      !origin ||
      choosingDownload.current ||
      origin.session !== sessionRef.current ||
      document.querySelector("dialog[open]")
    )
      return;
    downloadReturn.current = null;
    (
      rowElements.current.get(origin.path) ??
      rowElements.current.get(model.getState().selected) ??
      list.current
    )?.focus({ preventScroll: true });
  }
  function closeDownload() {
    setDownload(null);
  }

  const previousSession = useRef(sessionId);
  const scrollOwner = useRef(model);
  const restoredScroll = useRef(false);
  const lastReveal = useRef(0);
  const report = useRef(onStateChange);
  report.current = onStateChange;

  useEffect(() => {
    let previous = model.getState().currentPath;
    let existed = !!model.getState().directories[previous];
    return model.subscribe(() => {
      const next = model.getState();
      if (
        existed &&
        previous !== next.currentPath &&
        previous !== "/" &&
        !next.directories[previous] &&
        previous.startsWith(
          next.currentPath === "/" ? "/" : next.currentPath + "/",
        )
      ) {
        setPathNotice({
          message: `原目录已不存在，已返回 ${next.currentPath}`,
        });
      }
      previous = next.currentPath;
      existed = !!next.directories[previous];
    });
  }, [model]);

  useEffect(() => {
    setPath(state.currentPath);
  }, [state.currentPath, state.revealVersion]);
  useEffect(() => {
    if (previousSession.current === sessionId) return;
    previousSession.current = sessionId;
    setError("");
    setMenu(null);
    setDownload(null);
    setFocusedPath("/");
    setOperation(null);
    setCopyPaths(null);
    setDropPath(null);
    setMountDetail(null);
    setPathNotice(null);
  }, [sessionId]);
  useEffect(() => {
    if (!desktop || !sessionId) return;
    const id = crypto.randomUUID();
    let revision = 0;
    const update = () => {
      void transferApi
        .registerView(sessionId, id, currentPath.current, ++revision)
        .catch(() => {});
    };
    viewRegistration.current = { id, update };
    update();
    const timer = window.setInterval(update, 15000);
    return () => {
      clearInterval(timer);
      if (viewRegistration.current?.id === id) viewRegistration.current = null;
      void transferApi.unregisterView(id).catch(() => {});
    };
  }, [sessionId]);
  useEffect(() => {
    viewRegistration.current?.update();
  }, [state.currentPath]);
  useEffect(() => {
    report.current?.(model.snapshot());
  }, [model, state]);
  useEffect(() => {
    if (
      !active ||
      (mountDetail && (!detailMount || !mountPaths.includes(mountDetail.path)))
    )
      setMountDetail(null);
  }, [active, detailMount, mountDetail, mountPaths]);
  useEffect(() => {
    if (!pathNotice || pathNotice.error) return;
    const timer = window.setTimeout(() => setPathNotice(null), 2500);
    return () => clearTimeout(timer);
  }, [pathNotice]);
  useLayoutEffect(() => {
    if (scrollOwner.current !== model) {
      scrollOwner.current = model;
      restoredScroll.current = false;
      lastReveal.current = 0;
    }
    if (!active) {
      restoredScroll.current = false;
      return;
    }
    if (!list.current) return;
    if (!restoredScroll.current) {
      list.current.scrollTop = state.scrollTop;
      restoredScroll.current = true;
    }
    if (state.revealVersion !== lastReveal.current) {
      lastReveal.current = state.revealVersion;
      rowElements.current
        .get(state.selected)
        ?.scrollIntoView({ block: "nearest" });
    }
  }, [model, active, state.revealVersion, state.scrollTop, state.selected]);
  useEffect(() => {
    if (!rows.some((row) => row.file.path === focusedPath)) {
      setFocusedPath(
        rows.some((row) => row.file.path === selected) ? selected : "/",
      );
    }
  }, [rows, focusedPath, selected]);

  const dragStart = useNativeDragStart((e) => setError(errorOf(e).message));
  useDesktopDrag((event) => {
    if (
      !active ||
      !sessionId ||
      !panel.current ||
      event.kind === "workspaceTab"
    )
      return;
    if (event.phase === "leave") {
      setDropPath(null);
      hover.current = null;
      return;
    }
    const target = fileDropTarget(panel.current, event.x, event.y);
    setDropKind(event.intent === "move" ? "move" : event.kind);
    setDropPath(target);
    if (target && event.phase === "over") {
      if (hover.current?.path !== target)
        hover.current = { path: target, since: Date.now() };
      else if (Date.now() - hover.current.since > 650 && !expanded.has(target))
        void model.expand(target);
      if (list.current) {
        const bounds = list.current.getBoundingClientRect();
        if (event.y < bounds.top + 30) list.current.scrollTop -= 12;
        if (event.y > bounds.bottom - 30) list.current.scrollTop += 12;
      }
    }
    if (event.phase === "drop") {
      setDropPath(null);
      hover.current = null;
      if (!target || operation || busy) return;
      const owner = sessionId;
      void transferApi
        .claimDrop(event.token, owner, target)
        .then((draft) => {
          if (sessionRef.current === owner)
            setOperation({
              kind: draft.kind,
              draft,
            });
        })
        .catch((e) => setError(errorOf(e).message));
    }
  });
  useEffect(() => {
    if (!desktop || !sessionId) return;
    let live = true;
    let off: (() => void) | undefined;
    void getCurrentWindow()
      .listen<{ sessionId: string; path: string }>(
        "remote-directory-changed",
        ({ payload }) => {
          if (payload.sessionId === sessionId)
            model.invalidateRemotePath(payload.path, active);
        },
      )
      .then((cleanup) => {
        if (live) off = cleanup;
        else cleanup();
      });
    return () => {
      live = false;
      off?.();
    };
  }, [sessionId, model, active]);
  async function pickUpload(target: string) {
    if (!sessionId || busy) return;
    const owner = sessionId;
    setMenu(null);
    setError("");
    setBusy(true);
    try {
      const draft = await transferApi.pickFiles(owner, target);
      if (draft && sessionRef.current === owner)
        setOperation({ kind: "upload", draft });
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    if (!desktop || !sessionId) {
      setClipboard(null);
      return;
    }
    let live = true;
    let off: (() => void) | undefined;
    let revision = 0;
    const refresh = async () => {
      const current = ++revision;
      try {
        const value = await transferApi.clipboard(sessionId);
        if (live && current === revision) setClipboard(value);
      } catch {
        if (live && current === revision) setClipboard(null);
      }
    };
    void getCurrentWindow()
      .listen("remote-clipboard-changed", () => void refresh())
      .then((cleanup) => {
        if (live) {
          off = cleanup;
          void refresh();
        } else cleanup();
      });
    return () => {
      live = false;
      off?.();
    };
  }, [sessionId]);
  async function clipboardCopy(intent: "copy" | "move", file?: FileEntry) {
    if (!sessionId || busy || operation) return;
    const owner = sessionId;
    const paths = (
      file && !selection.includes(file.path) ? [file.path] : selection
    ).filter((p) => p !== "/");
    if (!paths.length) return;
    setMenu(null);
    setError("");
    try {
      await transferApi.copy(owner, paths, intent);
      const value = await transferApi.clipboard(owner);
      if (sessionRef.current === owner) setClipboard(value);
    } catch (e) {
      setError(errorOf(e).message);
    }
  }
  async function copyAbsolutePath(path: string) {
    const owner = sessionId;
    setMenu(null);
    setPathNotice(null);
    try {
      await navigator.clipboard.writeText(path);
      if (sessionRef.current === owner)
        setPathNotice({ message: "已复制绝对路径" });
    } catch {
      if (sessionRef.current === owner)
        setPathNotice({
          message: "复制绝对路径失败，请检查系统剪贴板权限后重试",
          error: true,
        });
    }
  }
  function showMount(mount: RemoteMount) {
    const anchor = rowElements.current.get(mount.target);
    setMenu(null);
    if (anchor) setMountDetail({ key: mount.key, path: mount.target, anchor });
  }
  async function paste(target: string) {
    if (!sessionId || busy || operation) return;
    const owner = sessionId;
    setMenu(null);
    setError("");
    setBusy(true);
    try {
      const draft = await transferApi.paste(owner, target);
      if (sessionRef.current === owner)
        setOperation({ kind: draft.kind, draft });
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  const pasteAllowed =
    !!clipboard &&
    !clipboard.reserved &&
    (clipboard.intent === "copy" || clipboard.sameServer);
  const pasteTarget =
    rows.find((row) => row.file.path === selected && row.file.isDir)?.file
      .path ?? state.currentPath;
  function create(kind: "file" | "directory", path: string) {
    if (!sessionId) return;
    setMenu(null);
    setOperation({ kind, sessionId, path });
  }
  const deletePaths = (file?: FileEntry) =>
    file && !selection.includes(file.path) ? [file.path] : selection;
  const canDelete = (paths: string[]) =>
    !!sessionId &&
    !busy &&
    !operation &&
    !download &&
    !copyPaths &&
    paths.length > 0 &&
    paths.every(
      (path) =>
        path !== "/" &&
        !mounts.state.table?.mounts.some((mount) => mount.target === path),
    );
  async function requestDelete(file?: FileEntry) {
    const paths = [...deletePaths(file)];
    if (!sessionId || preparingDelete.current || !canDelete(paths)) return;
    const owner = sessionId;
    preparingDelete.current = true;
    setMenu(null);
    setMountDetail(null);
    setError("");
    setBusy(true);
    try {
      const draft = await fileApi.prepareDelete(owner, paths);
      if (sessionRef.current === owner) {
        setDeleteDraft(draft);
      }
    } catch (e) {
      if (sessionRef.current === owner) setError(errorOf(e).message);
    } finally {
      preparingDelete.current = false;
      setBusy(false);
    }
  }
  function copyToServer(file?: FileEntry) {
    if (!sessionId || busy) return;
    const paths =
      file && !selection.includes(file.path) ? [file.path] : selection;
    const selectedPaths = paths.filter((path) => path !== "/");
    if (!selectedPaths.length) return;
    setMenu(null);
    setCopyPaths(selectedPaths);
  }
  function focusRow(next: string) {
    setFocusedPath(next);
    rowElements.current.get(next)?.focus({ preventScroll: true });
    rowElements.current.get(next)?.scrollIntoView({ block: "nearest" });
  }
  function choose(
    file: FileEntry,
    mode: "single" | "toggle" | "range" = "single",
  ) {
    setMountDetail(null);
    model.select(file.path, file.isDir, mode);
    if (file.isDir && mode === "single") {
      if (expanded.has(file.path)) model.collapse(file.path);
      else void model.expand(file.path);
    }
  }
  useLayoutEffect(() => {
    if (!menu || !contextMenu.current) return;
    const bounds = contextMenu.current.getBoundingClientRect();
    const x = Math.max(
      5,
      Math.min(
        Number.isFinite(menu.x) ? menu.x : 5,
        window.innerWidth - Math.ceil(bounds.width) - 5,
      ),
    );
    const y = Math.max(
      5,
      Math.min(
        Number.isFinite(menu.y) ? menu.y : 5,
        window.innerHeight - Math.ceil(bounds.height) - 5,
      ),
    );
    if (x !== menu.x || y !== menu.y) setMenu({ ...menu, x, y });
  }, [menu]);
  useEffect(() => {
    if (!menu) return;
    contextMenu.current
      ?.querySelector<HTMLButtonElement>("button:not(:disabled)")
      ?.focus();
    const dismiss = () => setMenu(null);
    const outside = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !contextMenu.current?.contains(event.target) &&
        !menu.anchor?.contains(event.target)
      )
        dismiss();
    };
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [menu]);
  function menuKey(event: React.KeyboardEvent) {
    if (
      !menu ||
      event.defaultPrevented ||
      event.nativeEvent.isComposing ||
      document.querySelector("dialog[open]")
    )
      return;
    const dismiss = () => setMenu(null);
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss();
      if (menu.anchor?.isConnected) menu.anchor.focus();
      else rowElements.current.get(menu.file.path)?.focus();
    }
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      const buttons = Array.from(
        contextMenu.current?.querySelectorAll<HTMLButtonElement>(
          "button:not(:disabled)",
        ) ?? [],
      );
      if (!buttons.length) return;
      event.preventDefault();
      const current = buttons.indexOf(
        document.activeElement as HTMLButtonElement,
      );
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? buttons.length - 1
            : (current +
                (event.key === "ArrowDown" ? 1 : -1) +
                buttons.length) %
              buttons.length;
      buttons[next]?.focus();
    }
  }

  useEffect(() => {
    if (download) {
      downloadDialog.current?.showModal();
      downloadDialog.current
        ?.querySelector<HTMLSelectElement>("select")
        ?.focus();
    } else {
      downloadDialog.current?.close();
      restoreDownloadFocus();
    }
  }, [download]);
  useEffect(() => {
    if (!active) setMenu(null);
  }, [active]);

  async function edit(file: FileEntry) {
    if (!sessionId || file.isDir || busy) return;
    setMenu(null);
    setError("");
    setBusy(true);
    try {
      await fileApi.openEditor(sessionId, file.path);
    } catch (error) {
      setError(errorOf(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function startDownload() {
    if (!sessionId || !download || busy) return;
    const target = download;
    choosingDownload.current = true;
    setDownload(null);
    setError("");
    setBusy(true);
    try {
      const id = await fileApi.startDownload(
        sessionId,
        target.path,
        target.isDir,
        conflict,
      );
      if (id) notifyDownloadStarted();
    } catch (error) {
      setError(errorOf(error).message);
    } finally {
      setBusy(false);
      choosingDownload.current = false;
      restoreDownloadFocus();
    }
  }

  const selectedFile =
    rows.find((row) => row.file.path === selected)?.file ??
    Object.values(state.directories)
      .flatMap((node) => node.directory?.entries ?? [])
      .find((file) => file.path === selected);
  const refreshPath = selectedFile?.isDir
    ? selectedFile.path
    : state.currentPath;
  const refreshLoading = state.directories[refreshPath]?.loading ?? false;
  const currentDirectory = state.directories[state.currentPath]?.directory;
  return (
    <section
      ref={panel}
      className="file-panel"
      aria-label="远程文件"
      data-session-id={sessionId ?? undefined}
    >
      <header className="panel-head">
        <div className="panel-title">
          <h2>
            <Folder size={16} aria-hidden="true" />
            远程文件
          </h2>
          <div className="file-toolbar">
            <button
              className="icon-button"
              aria-label="新建"
              title="新建文件或文件夹"
              aria-haspopup="menu"
              aria-expanded={menu?.create || undefined}
              disabled={!sessionId || busy}
              onClick={(event) => {
                if (menu?.create) {
                  setMenu(null);
                  return;
                }
                const bounds = event.currentTarget.getBoundingClientRect();
                setMenu({
                  create: true,
                  anchor: event.currentTarget,
                  file: {
                    name: state.currentPath,
                    path: state.currentPath,
                    isDir: true,
                    size: 0,
                    modified: 0,
                  },
                  x: bounds.right - 190,
                  y: bounds.bottom + 5,
                });
              }}
            >
              <Plus size={16} />
            </button>
            <button
              className="icon-button"
              aria-label="上传文件"
              title="选择本地文件上传"
              disabled={!sessionId || busy}
              onClick={() => void pickUpload(state.currentPath)}
            >
              <Upload size={15} />
            </button>
            <button
              className="icon-button"
              aria-label="刷新目录"
              disabled={!sessionId || refreshLoading}
              onClick={() => {
                void model.refresh(refreshPath);
                void mounts.model.refresh(true);
              }}
            >
              <RefreshCw
                size={15}
                className={refreshLoading ? "spinning" : ""}
              />
            </button>
          </div>
        </div>
        <span className="panel-server">{serverName || "未选择服务器"}</span>
        <div className="path-row">
          <button
            className="icon-button"
            aria-label="上一级目录"
            disabled={!sessionId || state.currentPath === "/"}
            onClick={() =>
              void model.revealPath(remoteParent(state.currentPath))
            }
          >
            <ArrowUp size={16} />
          </button>
          <input
            aria-label="远程目录路径"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            disabled={!sessionId}
            placeholder="远程目录"
            onKeyDown={(event) => {
              if (event.key === "Enter") void model.revealPath(path);
            }}
          />
        </div>
      </header>
      <div className="file-columns">
        <span>名称</span>
        <span>大小 / 使用率</span>
      </div>
      <div
        className="file-list remote-file-tree"
        ref={list}
        data-drop-directory={sessionId ? state.currentPath : undefined}
        data-drop-active={dropPath === state.currentPath || undefined}
        role={sessionId ? "tree" : undefined}
        aria-label={sessionId ? "远程文件树" : undefined}
        aria-multiselectable={sessionId ? true : undefined}
        tabIndex={0}
        onContextMenu={(event) => {
          if (
            !sessionId ||
            (event.target as HTMLElement).closest("[role=treeitem]")
          )
            return;
          event.preventDefault();
          setMountDetail(null);
          setMenu({
            blank: true,
            file: {
              name: state.currentPath,
              path: state.currentPath,
              isDir: true,
              size: 0,
              modified: 0,
            },
            x: event.clientX,
            y: event.clientY,
          });
        }}
        onKeyDown={(event) => {
          if (
            !active ||
            operation ||
            busy ||
            (event.target as HTMLElement).closest(
              "input,textarea,[contenteditable=true]",
            )
          )
            return;
          const key = event.key.toLowerCase();
          if (
            (event.ctrlKey || event.metaKey) &&
            !event.altKey &&
            ["c", "x", "v"].includes(key)
          ) {
            event.preventDefault();
            event.stopPropagation();
            if (key === "v")
              void paste(
                event.target === event.currentTarget
                  ? state.currentPath
                  : pasteTarget,
              );
            else void clipboardCopy(key === "x" ? "move" : "copy");
          } else if (
            !event.repeat &&
            !event.altKey &&
            !event.shiftKey &&
            (/Mac/i.test(navigator.platform)
              ? event.key === "Backspace" && event.metaKey && !event.ctrlKey
              : event.key === "Delete" && !event.metaKey && !event.ctrlKey)
          ) {
            event.preventDefault();
            event.stopPropagation();
            void requestDelete();
          } else if (event.key === "Escape" && clipboard?.intent === "move") {
            event.preventDefault();
            void transferApi
              .clearClipboard(clipboard.id)
              .then(() => (sessionId ? transferApi.clipboard(sessionId) : null))
              .then((value) => {
                if (sessionRef.current === sessionId) setClipboard(value);
              })
              .catch((e) => setError(errorOf(e).message));
          }
        }}
        onScroll={(event) => {
          if (active && restoredScroll.current)
            model.setScrollTop(event.currentTarget.scrollTop);
        }}
      >
        {!sessionId ? (
          <div className="panel-empty">
            <Folder size={28} aria-hidden="true" />
            <strong>连接服务器后浏览文件</strong>
            <p>目录与文件将显示在这里</p>
          </div>
        ) : (
          rows.map(({ file, depth, parent, position, siblings }, index) => {
            const node = state.directories[file.path];
            const isExpanded = file.isDir && expanded.has(file.path);
            const mount = mountForFile(file, mounts.state.table?.mounts ?? []);
            const capacity = mount ? mounts.state.usage[mount.key] : undefined;
            const value = capacity?.value;
            const percent = value?.usedPercent;
            const mountLabel =
              value?.status === "unsupported"
                ? "—"
                : percent != null
                  ? `${percent}%`
                  : capacity?.loading
                    ? "读取中"
                    : "暂不可用";
            const mountTotal =
              value?.status === "ready"
                ? formatMountBytes(value.totalBytes)
                : null;
            const mountAccessibleLabel =
              value?.status === "ready"
                ? `${file.path}，总容量 ${mountTotal}，已用 ${formatMountBytes(value.usedBytes)}，可用 ${formatMountBytes(value.availableBytes)}，${percent == null ? "使用率不可用" : `使用率 ${percent}%`}${capacity?.stale ? "，上次数据" : ""}，查看挂载详情`
                : `${file.path}，${mountLabel}${capacity?.stale ? "，上次数据" : ""}，查看挂载详情`;
            const mountTitle =
              value?.status === "ready"
                ? `总计 ${mountTotal} · 已用 ${formatMountBytes(value.usedBytes)} · 可用 ${formatMountBytes(value.availableBytes)}${value.updatedAt ? `\n${capacity?.stale ? "上次采样" : "更新于"} ${new Date(value.updatedAt).toLocaleTimeString()}` : ""}`
                : value?.updatedAt
                  ? `${capacity?.stale ? "上次采样" : "更新于"} ${new Date(value.updatedAt).toLocaleTimeString()}`
                  : "尚无容量采样";
            return (
              <Fragment key={file.path}>
                <div
                  ref={(element) => {
                    if (element) rowElements.current.set(file.path, element);
                    else rowElements.current.delete(file.path);
                  }}
                  className={
                    "file-row file-tree-row" +
                    (mount
                      ? ` file-mount-row mount-${usageSeverity(percent)}`
                      : "") +
                    (selection.includes(file.path) ? " selected" : "") +
                    (clipboard?.intent === "move" &&
                    clipboard.sameServer &&
                    clipboard.paths.some(
                      (p) => file.path === p || file.path.startsWith(p + "/"),
                    )
                      ? " cut-pending"
                      : "") +
                    (dropPath === file.path ? " drop-target" : "") +
                    (state.terminalCwd?.path === file.path
                      ? " terminal-directory"
                      : "")
                  }
                  style={{ paddingLeft: 5 + depth * 15 }}
                  role="treeitem"
                  data-path={file.path}
                  data-drop-directory={file.isDir ? file.path : undefined}
                  aria-label={file.name}
                  aria-level={depth + 1}
                  aria-posinset={position}
                  aria-setsize={siblings}
                  aria-selected={selection.includes(file.path)}
                  aria-expanded={file.isDir ? isExpanded : undefined}
                  aria-busy={node?.loading || undefined}
                  tabIndex={focusedPath === file.path ? 0 : -1}
                  onFocus={() => setFocusedPath(file.path)}
                  onPointerDown={(event) => {
                    if (
                      (event.target as HTMLElement).closest(
                        "[data-mount-detail]",
                      )
                    )
                      return;
                    if (!sessionId || file.path === "/") return;
                    dragStart.down(event, () => {
                      if (!selection.includes(file.path))
                        model.select(file.path, file.isDir);
                      return transferApi.beginDrag(
                        sessionId,
                        selection.includes(file.path) ? selection : [file.path],
                      );
                    });
                  }}
                  onClick={(event) => {
                    if (
                      (event.target as HTMLElement).closest(
                        "[data-mount-detail]",
                      )
                    )
                      return;
                    if (!dragStart.suppressClick())
                      choose(
                        file,
                        event.shiftKey
                          ? "range"
                          : event.metaKey || event.ctrlKey
                            ? "toggle"
                            : "single",
                      );
                  }}
                  onDoubleClick={() => {
                    if (!dragStart.suppressClick() && !file.isDir)
                      void edit(file);
                  }}
                  onKeyDown={(event) => {
                    if (event.target !== event.currentTarget) return;
                    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                      event.preventDefault();
                      const next =
                        rows[
                          Math.min(
                            rows.length - 1,
                            Math.max(
                              0,
                              index + (event.key === "ArrowDown" ? 1 : -1),
                            ),
                          )
                        ].file;
                      focusRow(next.path);
                      if (event.shiftKey)
                        model.select(next.path, next.isDir, "range");
                    } else if (event.key === "Home" || event.key === "End") {
                      event.preventDefault();
                      focusRow(
                        rows[event.key === "Home" ? 0 : rows.length - 1].file
                          .path,
                      );
                    } else if (event.key === "ArrowRight") {
                      event.preventDefault();
                      if (!file.isDir) return;
                      if (!isExpanded) void model.expand(file.path);
                      else if (rows[index + 1]?.parent === file.path)
                        focusRow(rows[index + 1].file.path);
                    } else if (event.key === "ArrowLeft") {
                      event.preventDefault();
                      if (isExpanded) model.collapse(file.path);
                      else if (parent) focusRow(parent);
                    } else if (event.key === " " || event.key === "Enter") {
                      event.preventDefault();
                      if (event.key === "Enter" && !file.isDir) void edit(file);
                      else
                        choose(
                          file,
                          event.shiftKey
                            ? "range"
                            : event.metaKey || event.ctrlKey
                              ? "toggle"
                              : "single",
                        );
                    } else if (
                      event.key === "ContextMenu" ||
                      (event.key === "F10" && event.shiftKey)
                    ) {
                      event.preventDefault();
                      const rect = event.currentTarget.getBoundingClientRect();
                      if (!selection.includes(file.path))
                        model.select(file.path, file.isDir);
                      setMenu({
                        file,
                        x: rect.x + 20,
                        y: rect.bottom,
                      });
                    } else if (
                      event.key.length === 1 &&
                      !event.ctrlKey &&
                      !event.metaKey &&
                      !event.altKey
                    ) {
                      const ordered = [
                        ...rows.slice(index + 1),
                        ...rows.slice(0, index + 1),
                      ];
                      const match = ordered.find((row) =>
                        row.file.name
                          .toLocaleLowerCase()
                          .startsWith(event.key.toLocaleLowerCase()),
                      );
                      if (match) {
                        event.preventDefault();
                        focusRow(match.file.path);
                      }
                    }
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setMountDetail(null);
                    if (!selection.includes(file.path))
                      model.select(file.path, file.isDir);
                    setMenu({
                      file,
                      x: event.clientX,
                      y: event.clientY,
                    });
                  }}
                >
                  <span className="file-tree-chevron" aria-hidden="true">
                    {file.isDir &&
                      (node?.loading ? (
                        <RefreshCw size={12} className="spinning" />
                      ) : isExpanded ? (
                        <ChevronDown size={13} />
                      ) : (
                        <ChevronRight size={13} />
                      ))}
                  </span>
                  <span className="file-icon" aria-hidden="true">
                    {mount ? (
                      mountKind(mount) === "network" ? (
                        <Network size={16} />
                      ) : mountKind(mount) === "memory" ? (
                        <MemoryStick size={16} />
                      ) : (
                        <HardDrive size={16} />
                      )
                    ) : file.isDir ? (
                      <Folder size={16} />
                    ) : (
                      <FileText size={16} />
                    )}
                  </span>
                  <span className="file-name-group">
                    <span className="file-name" title={file.path}>
                      {file.name}
                    </span>
                    {mount && (
                      <button
                        type="button"
                        className="mount-source"
                        data-mount-detail
                        aria-label={`查看 ${file.path} 挂载详情`}
                        title={mountSource(mount)}
                        onClick={() => showMount(mount)}
                      >
                        {mountSource(mount)}
                        {mountKind(mount) === "memory" ? " · 内存" : ""}
                      </button>
                    )}
                  </span>
                  {file.isSymlink && (
                    <Link
                      size={11}
                      className="file-link-mark"
                      aria-label="符号链接"
                    />
                  )}
                  {state.terminalCwd?.path === file.path && (
                    <span
                      className="file-cwd-mark"
                      title="终端当前目录"
                      aria-label="终端当前目录"
                    />
                  )}
                  {mount ? (
                    <button
                      type="button"
                      className={`mount-usage${capacity?.stale ? " stale" : ""}`}
                      data-mount-detail
                      aria-label={mountAccessibleLabel}
                      title={mountTitle}
                      aria-haspopup="dialog"
                      aria-expanded={mountDetail?.key === mount.key}
                      onClick={() => showMount(mount)}
                    >
                      {value?.status === "ready" ? (
                        <>
                          <span className="mount-capacity-total">
                            {mountTotal}
                          </span>
                          <span className="mount-capacity-percent">
                            {mountLabel}
                            {capacity?.stale ? " · 上次" : ""}
                          </span>
                          {percent != null && (
                            <span className="mount-meter" aria-hidden="true">
                              <span style={{ width: `${percent}%` }} />
                            </span>
                          )}
                        </>
                      ) : (
                        <span className="mount-capacity-status">
                          {mountLabel}
                        </span>
                      )}
                    </button>
                  ) : (
                    <span className="file-size">
                      {file.isDir ? "—" : formatSize(file.size)}
                    </span>
                  )}
                </div>
                {isExpanded && node?.error && (
                  <div
                    className="file-tree-status file-tree-error"
                    style={{ paddingLeft: 26 + depth * 15 }}
                  >
                    <span>{node.error}</span>
                    <button
                      className="quiet"
                      onClick={() => void model.refresh(file.path)}
                    >
                      重试
                    </button>
                  </div>
                )}
                {isExpanded &&
                  !node?.error &&
                  node?.directory?.entries.length === 0 && (
                    <div
                      className="file-tree-status"
                      style={{ paddingLeft: 26 + depth * 15 }}
                    >
                      目录为空
                    </div>
                  )}
              </Fragment>
            );
          })
        )}
      </div>
      {dropPath && (
        <div className="file-drop-hint" role="status">
          {dropKind === "localFiles"
            ? "上传到"
            : dropKind === "move"
              ? "移动到"
              : "复制到"}{" "}
          {serverName} · {dropPath}
        </div>
      )}
      {(error || state.error) && (
        <p className="panel-error" role="alert">
          {error || state.error}
          {selectedFile && (
            <button
              className="quiet"
              disabled={busy}
              onClick={() => beginDownload(selectedFile)}
            >
              下载此项目
            </button>
          )}
        </p>
      )}
      {pathNotice && (
        <p
          className={`file-path-notice${pathNotice.error ? " failed" : ""}`}
          role={pathNotice.error ? "alert" : "status"}
        >
          {pathNotice.message}
        </p>
      )}
      {(mounts.state.error || mountNotice) && (
        <div className="mount-panel-notice" role="status">
          <span>
            {mounts.state.error
              ? `挂载信息暂不可用：${mounts.state.error}`
              : mountNotice}
          </span>
          <button
            className="quiet"
            disabled={mounts.state.loading}
            onClick={() => void mounts.model.refresh(true)}
          >
            重试
          </button>
        </div>
      )}
      <footer className="panel-footer">
        {loading
          ? "正在读取目录…"
          : busy
            ? "正在处理…"
            : selection.length > 1
              ? `${selection.length} 个项目已选中`
              : selected
                ? selected === "/"
                  ? "/"
                  : selected.split("/").pop()
                : currentDirectory
                  ? currentDirectory.entries.length + " 个项目 · 双击编辑文件"
                  : "SFTP"}
      </footer>
      {menu && (
        <div
          className="context-menu file-context-menu"
          ref={contextMenu}
          role="menu"
          aria-label={menu.create ? "新建" : "文件操作"}
          onKeyDown={menuKey}
          style={{ left: Math.max(5, menu.x), top: Math.max(5, menu.y) }}
        >
          {menu.create ? (
            <>
              <button
                role="menuitem"
                disabled={busy}
                onClick={() => create("directory", menu.file.path)}
              >
                <FolderPlus size={14} />
                新建文件夹
              </button>
              <button
                role="menuitem"
                disabled={busy}
                onClick={() => create("file", menu.file.path)}
              >
                <FilePlus2 size={14} />
                新建文件
              </button>
            </>
          ) : (
            <>
              <button
                role="menuitem"
                onClick={() => void copyAbsolutePath(menu.file.path)}
              >
                <ClipboardCopy size={14} />
                {menu.blank ? "复制当前目录绝对路径" : "复制绝对路径"}
              </button>
              {(() => {
                const mount = mountForFile(
                  menu.file,
                  mounts.state.table?.mounts ?? [],
                );
                return mount ? (
                  <button role="menuitem" onClick={() => showMount(mount)}>
                    <Info size={14} />
                    查看挂载详情
                  </button>
                ) : null;
              })()}
              {!menu.blank && (
                <>
                  <button
                    role="menuitem"
                    disabled={busy || menu.file.path === "/"}
                    onClick={() => void clipboardCopy("copy", menu.file)}
                  >
                    <Copy size={14} />
                    复制
                  </button>
                  <button
                    role="menuitem"
                    disabled={busy || menu.file.path === "/"}
                    onClick={() => void clipboardCopy("move", menu.file)}
                  >
                    <Scissors size={14} />
                    剪切
                  </button>
                </>
              )}
              {menu.file.isDir && (
                <button
                  role="menuitem"
                  disabled={busy || !pasteAllowed}
                  onClick={() => void paste(menu.file.path)}
                >
                  <ClipboardPaste size={14} />
                  粘贴到此处
                </button>
              )}
              {menu.file.isDir && (
                <>
                  <button
                    role="menuitem"
                    disabled={busy}
                    onClick={() => create("directory", menu.file.path)}
                  >
                    <FolderPlus size={14} />
                    新建文件夹
                  </button>
                  <button
                    role="menuitem"
                    disabled={busy}
                    onClick={() => create("file", menu.file.path)}
                  >
                    <FilePlus2 size={14} />
                    新建文件
                  </button>
                  <button
                    role="menuitem"
                    disabled={busy}
                    onClick={() => void pickUpload(menu.file.path)}
                  >
                    <Upload size={14} />
                    上传文件到此处
                  </button>
                </>
              )}
              {!menu.file.isDir && (
                <button
                  role="menuitem"
                  disabled={busy}
                  onClick={() => void edit(menu.file)}
                >
                  <Pencil size={14} />
                  编辑文本文件
                </button>
              )}
              {!menu.blank && menu.file.isDir && (
                <button
                  role="menuitem"
                  onClick={() => {
                    model.select(menu.file.path, true);
                    void model.expand(menu.file.path);
                  }}
                >
                  <Folder size={14} />
                  打开文件夹
                </button>
              )}
              {!menu.blank && (
                <>
                  <button
                    role="menuitem"
                    disabled={busy}
                    onClick={() => beginDownload(menu.file)}
                  >
                    <Download size={14} />
                    下载{menu.file.isDir ? "文件夹" : "文件"}
                  </button>
                  <button
                    role="menuitem"
                    disabled={busy || menu.file.path === "/"}
                    onClick={() => copyToServer(menu.file)}
                  >
                    <ArrowRightLeft size={14} />
                    复制到另一台服务器…
                  </button>
                </>
              )}
              {!menu.blank && (
                <button
                  role="menuitem"
                  className="danger file-delete-menu-item"
                  disabled={!canDelete(deletePaths(menu.file))}
                  onClick={() => void requestDelete(menu.file)}
                >
                  <Trash2 size={14} />
                  {deletePaths(menu.file).length > 1
                    ? `删除选中的 ${deletePaths(menu.file).length} 项…`
                    : "删除…"}
                </button>
              )}
            </>
          )}
        </div>
      )}
      {deleteDraft && (
        <DeleteFilesDialog
          draft={deleteDraft}
          onClose={() => {
            setDeleteDraft(null);
            rowElements.current.get(model.getState().selected)?.focus();
          }}
          onStarted={() => {
            setDeleteDraft(null);
            rowElements.current.get(model.getState().selected)?.focus();
          }}
        />
      )}
      {mountDetail && detailMount && (
        <MountDetailsPopover
          mount={detailMount}
          usage={mounts.state.usage[detailMount.key]}
          anchor={mountDetail.anchor}
          onClose={() => setMountDetail(null)}
          onRefresh={() => void mounts.model.refresh(true)}
        />
      )}
      {copyPaths && sessionId && (
        <CopyTargetDialog
          sessionId={sessionId}
          paths={copyPaths}
          onClose={() => setCopyPaths(null)}
          onPrepared={(draft) => {
            setCopyPaths(null);
            setOperation({ kind: draft.kind, draft });
          }}
        />
      )}
      {operation && (
        <FileOperationDialog
          operation={operation}
          onClose={() => setOperation(null)}
          onCreated={(path, directory) => {
            const owner = sessionId;
            void model.refresh(remoteParent(path)).then(() => {
              if (sessionRef.current === owner) {
                void model.expand(remoteParent(path));
                model.select(path, directory);
                setFocusedPath(path);
              }
            });
          }}
        />
      )}
      <dialog
        ref={downloadDialog}
        className="download-dialog"
        aria-labelledby={downloadTitle}
        onCancel={(event) => {
          event.preventDefault();
          closeDownload();
        }}
      >
        <div className="dialog-head">
          <h2 id={downloadTitle}>下载{download?.isDir ? "文件夹" : "文件"}</h2>
          <button
            className="icon-button"
            aria-label="取消下载"
            onClick={closeDownload}
          >
            <X size={17} />
          </button>
        </div>
        <p className="download-source">{download?.path}</p>
        <label>
          本地已有同名文件时
          <select
            aria-label="同名文件处理"
            value={conflict}
            onChange={(event) =>
              setConflict(event.target.value as DownloadConflict)
            }
          >
            <option value="rename">保留两份，自动重命名</option>
            <option value="skip">跳过已有文件</option>
            <option value="overwrite">覆盖已有文件</option>
          </select>
        </label>
        {download?.isDir && (
          <p className="muted download-hint">
            保留目录结构和隐藏文件。符号链接会跳过并在下载结果中说明。
          </p>
        )}
        <div className="dialog-actions">
          <button onClick={closeDownload}>取消</button>
          <button className="primary" onClick={() => void startDownload()}>
            <Download size={15} />
            选择保存位置
          </button>
        </div>
      </dialog>
    </section>
  );
}
