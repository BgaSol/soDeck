import { createPortal } from "react-dom";
import { useWorkspaceDialog } from "./WorkspaceInteractionContext";
import { useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";
import { fileApi, type DeletePreview, type DeleteTask } from "./fileApi";
import { notifyFileTaskStarted } from "./fileTaskApi";
import { errorOf } from "./types";

export function DeleteFilesDialog({
  draft,
  onClose,
  onStarted,
}: {
  draft: DeletePreview;
  onClose: () => void;
  onStarted: (task: DeleteTask) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const pending = useRef(false);
  const alive = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useWorkspaceDialog(dialog, "files");
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const close = () => {
    if (!pending.current) onClose();
  };
  async function start() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      const task = await fileApi.startDelete(draft.id);
      // Once accepted, the task belongs to the application, independent of this UI.
      notifyFileTaskStarted();
      if (alive.current) onStarted(task);
    } catch (e) {
      if (alive.current) setError(errorOf(e).message);
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  }
  return createPortal(
    <dialog
      ref={dialog}
      className="delete-files-dialog"
      aria-labelledby="delete-files-title"
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <DialogHeader
        id="delete-files-title"
        title={`永久删除 ${draft.entries.length} 个项目？`}
        subtitle={draft.serverName}
        closeLabel="关闭删除窗口"
        onClose={close}
        disabled={busy}
      />
      <div className="delete-files-body">
        <p>
          将从服务器永久删除以下项目。文件夹中的全部内容也会删除，符号链接只删除链接本身。此操作无法撤销。
        </p>
        {draft.parentTaskId && (
          <p className="muted">
            这是重新发起的删除。目录范围包含当前剩余内容和后来新增的内容，请重新确认。
          </p>
        )}
        <ul className="delete-entry-list" aria-label="删除项目">
          {draft.entries.map((entry) => (
            <li key={entry.path}>
              <div className="delete-entry-heading">
                <span className="delete-entry-path">{entry.path}</span>
                <span className="muted">
                  {entry.kind === "directory"
                    ? "文件夹及其内容"
                    : entry.kind === "symlink"
                      ? "符号链接"
                      : entry.kind === "missing"
                        ? "已不存在"
                        : "文件"}
                </span>
              </div>
            </li>
          ))}
        </ul>
        <p className="muted">
          确认后将在后台执行，可在右下角“文件任务”查看进度和结果。
        </p>
        {error && (
          <p className="panel-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="dialog-actions">
        <button ref={cancel} data-initial-focus disabled={busy} onClick={close}>
          取消
        </button>
        <button
          className="delete-confirm"
          disabled={busy}
          onClick={() => void start()}
        >
          {busy ? "正在开始…" : "永久删除"}
        </button>
      </footer>
    </dialog>,
    document.body,
  );
}
