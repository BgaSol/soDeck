import {
  Fragment,
  useEffect,
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
} from "lucide-react";
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
import "./files.css";

export type FilePanelSnapshot = RemoteFileTreeSnapshot;

export function FilePanel({
  sessionId,
  serverName,
  active = true,
  initialState,
  onStateChange,
  terminalCwd,
}: {
  sessionId: string | null;
  serverName: string;
  active?: boolean;
  initialState?: FilePanelSnapshot;
  onStateChange?: (state: FilePanelSnapshot) => void;
  terminalCwd?: TerminalDirectory;
}) {
  const { model, state } = useRemoteFileTree(
    sessionId,
    active,
    initialState,
    terminalCwd,
  );
  const { selected } = state;
  const [path, setPath] = useState(state.currentPath);
  const [focusedPath, setFocusedPath] = useState(selected || "/");
  const list = useRef<HTMLDivElement>(null);
  const rowElements = useRef(new Map<string, HTMLButtonElement>());
  const rows = useMemo(() => visibleFileRows(state), [state]);
  const expanded = useMemo(() => new Set(state.expanded), [state.expanded]);
  const loading =
    state.revealing ||
    Object.values(state.directories).some((node) => node.loading);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<{
    file: FileEntry;
    x: number;
    y: number;
  } | null>(null);
  const [download, setDownload] = useState<FileEntry | null>(null);
  const [conflict, setConflict] = useState<DownloadConflict>("rename");
  const downloadDialog = useRef<HTMLDialogElement>(null);
  const previousSession = useRef(sessionId);
  const scrollOwner = useRef(model);
  const restoredScroll = useRef(false);
  const lastReveal = useRef(0);
  const report = useRef(onStateChange);
  report.current = onStateChange;

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
  }, [sessionId]);
  useEffect(() => {
    report.current?.(model.snapshot());
  }, [model, state]);
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

  function focusRow(next: string) {
    setFocusedPath(next);
    rowElements.current.get(next)?.focus({ preventScroll: true });
    rowElements.current.get(next)?.scrollIntoView({ block: "nearest" });
  }
  function choose(file: FileEntry) {
    model.select(file.path, file.isDir);
    if (file.isDir) {
      if (expanded.has(file.path)) model.collapse(file.path);
      else void model.expand(file.path);
    }
  }
  useEffect(() => {
    if (!menu) return;
    const dismiss = () => setMenu(null);
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismiss();
    };
    window.addEventListener("click", dismiss);
    window.addEventListener("blur", dismiss);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("click", dismiss);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("keydown", key);
    };
  }, [menu]);
  useEffect(() => {
    if (download) downloadDialog.current?.showModal();
    else downloadDialog.current?.close();
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
    <section className="file-panel" aria-label="远程文件" data-session-id={sessionId ?? undefined}>
      <header className="panel-head">
        <div className="panel-title">
          <h2>
            <Folder size={16} aria-hidden="true" />
            远程文件
          </h2>
          <div className="file-toolbar">
            <button
              className="icon-button"
              aria-label="编辑选中文件"
              title="编辑文本文件（UTF-8，最大 1 MB）"
              disabled={
                !sessionId || !selectedFile || selectedFile.isDir || busy
              }
              onClick={() => selectedFile && void edit(selectedFile)}
            >
              <Pencil size={15} />
            </button>
            <button
              className="icon-button"
              aria-label="下载选中项目"
              title="下载文件或文件夹"
              disabled={!sessionId || !selectedFile || busy}
              onClick={() => selectedFile && setDownload(selectedFile)}
            >
              <Download size={15} />
            </button>
            <button
              className="icon-button"
              aria-label="刷新目录"
              disabled={!sessionId || refreshLoading}
              onClick={() => void model.refresh(refreshPath)}
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
        <span>大小</span>
      </div>
      <div
        className="file-list remote-file-tree"
        ref={list}
        role={sessionId ? "tree" : undefined}
        aria-label={sessionId ? "远程文件树" : undefined}
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
            return (
              <Fragment key={file.path}>
                <button
                  ref={(element) => {
                    if (element) rowElements.current.set(file.path, element);
                    else rowElements.current.delete(file.path);
                  }}
                  className={
                    "file-row file-tree-row" +
                    (selected === file.path ? " selected" : "") +
                    (state.terminalCwd?.path === file.path
                      ? " terminal-directory"
                      : "")
                  }
                  style={{ paddingLeft: 5 + depth * 15 }}
                  role="treeitem"
                  data-path={file.path}
                  aria-label={file.name}
                  aria-level={depth + 1}
                  aria-posinset={position}
                  aria-setsize={siblings}
                  aria-selected={selected === file.path}
                  aria-expanded={file.isDir ? isExpanded : undefined}
                  aria-busy={node?.loading || undefined}
                  tabIndex={focusedPath === file.path ? 0 : -1}
                  onFocus={() => setFocusedPath(file.path)}
                  onClick={() => choose(file)}
                  onDoubleClick={() => {
                    if (!file.isDir) void edit(file);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                      event.preventDefault();
                      focusRow(
                        rows[
                          Math.min(
                            rows.length - 1,
                            Math.max(
                              0,
                              index + (event.key === "ArrowDown" ? 1 : -1),
                            ),
                          )
                        ].file.path,
                      );
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
                      else choose(file);
                    } else if (
                      event.key === "ContextMenu" ||
                      (event.key === "F10" && event.shiftKey)
                    ) {
                      event.preventDefault();
                      const rect = event.currentTarget.getBoundingClientRect();
                      model.select(file.path, file.isDir);
                      setMenu({
                        file,
                        x: Math.min(rect.x + 20, window.innerWidth - 195),
                        y: Math.min(rect.bottom, window.innerHeight - 105),
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
                    model.select(file.path, file.isDir);
                    setMenu({
                      file,
                      x: Math.min(event.clientX, window.innerWidth - 195),
                      y: Math.min(event.clientY, window.innerHeight - 105),
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
                    {file.isDir ? <Folder size={16} /> : <FileText size={16} />}
                  </span>
                  <span className="file-name" title={file.path}>
                    {file.name}
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
                  <span className="file-size">
                    {file.isDir ? "—" : formatSize(file.size)}
                  </span>
                </button>
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
      {(error || state.error) && (
        <p className="panel-error" role="alert">
          {error || state.error}
          {selectedFile && (
            <button
              className="quiet"
              disabled={busy}
              onClick={() => setDownload(selectedFile)}
            >
              下载此项目
            </button>
          )}
        </p>
      )}
      <footer className="panel-footer">
        {loading
          ? "正在读取目录…"
          : busy
            ? "正在打开…"
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
          className="server-context-menu file-context-menu"
          role="menu"
          aria-label="文件操作"
          style={{ left: Math.max(5, menu.x), top: Math.max(5, menu.y) }}
        >
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
          {menu.file.isDir && (
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
          <button
            role="menuitem"
            disabled={busy}
            onClick={() => setDownload(menu.file)}
          >
            <Download size={14} />
            下载{menu.file.isDir ? "文件夹" : "文件"}
          </button>
        </div>
      )}
      <dialog
        ref={downloadDialog}
        className="download-dialog"
        onCancel={() => setDownload(null)}
      >
        <div className="dialog-head">
          <h2>下载{download?.isDir ? "文件夹" : "文件"}</h2>
          <button
            className="icon-button"
            aria-label="取消下载"
            onClick={() => setDownload(null)}
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
          <button onClick={() => setDownload(null)}>取消</button>
          <button className="primary" onClick={() => void startDownload()}>
            <Download size={15} />
            选择保存位置
          </button>
        </div>
      </dialog>
    </section>
  );
}
