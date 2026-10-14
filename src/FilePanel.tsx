import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowUp,
  Download,
  Folder,
  FileText,
  Pencil,
  RefreshCw,
  X,
} from "lucide-react";
import { api } from "./api";
import { fileApi, notifyDownloadStarted } from "./fileApi";
import type { DownloadConflict } from "./fileApi";
import { errorOf, formatSize } from "./types";
import type { Directory, FileEntry } from "./types";
import "./files.css";

export interface FilePanelSnapshot {
  sessionId: string | null;
  directory: Directory | null;
  path: string;
  selected: string;
}

export function FilePanel({
  sessionId,
  serverName,
  active = true,
  initialState,
  onStateChange,
}: {
  sessionId: string | null;
  serverName: string;
  active?: boolean;
  initialState?: FilePanelSnapshot;
  onStateChange?: (state: FilePanelSnapshot) => void;
}) {
  const snapshot =
    initialState?.sessionId === sessionId ? initialState : undefined;
  const [directory, setDirectory] = useState<Directory | null>(
    snapshot?.directory ?? null,
  );
  const [path, setPath] = useState(snapshot?.path ?? "");
  const [selected, setSelected] = useState(snapshot?.selected ?? "");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<{
    file: FileEntry;
    x: number;
    y: number;
  } | null>(null);
  const [download, setDownload] = useState<FileEntry | null>(null);
  const [conflict, setConflict] = useState<DownloadConflict>("rename");
  const downloadDialog = useRef<HTMLDialogElement>(null);
  const generation = useRef(0);
  const previousSession = useRef(sessionId);
  const initialized = useRef(!!snapshot?.directory);
  const report = useRef(onStateChange);
  report.current = onStateChange;

  const load = useCallback(
    async (next?: string) => {
      if (!sessionId) return;
      const token = ++generation.current;
      setLoading(true);
      setError("");
      try {
        const result = await api.directory(sessionId, next);
        if (token !== generation.current) return;
        setDirectory(result);
        setPath(result.path);
        setSelected("");
      } catch (error) {
        if (token === generation.current) setError(errorOf(error).message);
      } finally {
        if (token === generation.current) setLoading(false);
      }
    },
    [sessionId],
  );

  useEffect(() => {
    if (previousSession.current !== sessionId) {
      generation.current++;
      previousSession.current = sessionId;
      setDirectory(null);
      setPath("");
      setSelected("");
      setError("");
      setLoading(false);
      setMenu(null);
      setDownload(null);
      initialized.current = false;
    }
    if (sessionId && active && !initialized.current) {
      initialized.current = true;
      void load();
    }
  }, [sessionId, active, load]);

  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );
  useEffect(() => {
    report.current?.({ sessionId, directory, path, selected });
  }, [sessionId, directory, path, selected]);
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

  const selectedFile = directory?.entries.find(
    (file) => file.path === selected,
  );
  return (
    <section className="file-panel" aria-label="远程文件">
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
              disabled={!sessionId || loading}
              onClick={() => void load(directory?.path)}
            >
              <RefreshCw size={15} className={loading ? "spinning" : ""} />
            </button>
          </div>
        </div>
        <span className="panel-server">{serverName || "未选择服务器"}</span>
        <div className="path-row">
          <button
            className="icon-button"
            aria-label="上一级目录"
            disabled={!sessionId || loading || directory?.path === "/"}
            onClick={() =>
              void load(
                directory?.path.split("/").slice(0, -1).join("/") || "/",
              )
            }
          >
            <ArrowUp size={16} />
          </button>
          <input
            aria-label="远程目录路径"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            disabled={!sessionId || loading}
            placeholder="远程目录"
            onKeyDown={(event) => {
              if (event.key === "Enter") void load(path);
            }}
          />
        </div>
      </header>
      <div className="file-columns">
        <span>名称</span>
        <span>大小</span>
      </div>
      <div className="file-list">
        {!sessionId ? (
          <div className="panel-empty">
            <Folder size={28} aria-hidden="true" />
            <strong>连接服务器后浏览文件</strong>
            <p>目录与文件将显示在这里</p>
          </div>
        ) : (
          directory?.entries.map((file) => (
            <button
              className={
                "file-row" + (selected === file.path ? " selected" : "")
              }
              key={file.path}
              aria-pressed={selected === file.path}
              onClick={() => {
                if (file.isDir) void load(file.path);
                else setSelected(file.path);
              }}
              onDoubleClick={() => {
                if (!file.isDir) void edit(file);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  if (file.isDir) void load(file.path);
                  else void edit(file);
                }
                if (
                  event.key === "ContextMenu" ||
                  (event.key === "F10" && event.shiftKey)
                ) {
                  event.preventDefault();
                  const rect = event.currentTarget.getBoundingClientRect();
                  setSelected(file.path);
                  setMenu({
                    file,
                    x: Math.min(rect.x + 20, window.innerWidth - 195),
                    y: Math.min(rect.bottom, window.innerHeight - 105),
                  });
                }
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                setSelected(file.path);
                setMenu({
                  file,
                  x: Math.min(event.clientX, window.innerWidth - 195),
                  y: Math.min(event.clientY, window.innerHeight - 105),
                });
              }}
              disabled={loading}
            >
              <span className="file-icon">
                {file.isDir ? <Folder size={16} /> : <FileText size={16} />}
              </span>
              <span className="file-name" title={file.name}>
                {file.name}
              </span>
              <span className="file-size">
                {file.isDir ? "—" : formatSize(file.size)}
              </span>
            </button>
          ))
        )}
        {sessionId && directory?.entries.length === 0 && (
          <div className="panel-empty">目录为空</div>
        )}
      </div>
      {error && (
        <p className="panel-error" role="alert">
          {error}
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
              ? selected.split("/").pop()
              : directory
                ? directory.entries.length + " 个项目 · 双击编辑文件"
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
            <button role="menuitem" onClick={() => void load(menu.file.path)}>
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
