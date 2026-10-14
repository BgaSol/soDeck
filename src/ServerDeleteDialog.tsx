import { useEffect, useRef, useState } from "react";
import { addressOf, errorOf } from "./types";
import type { Server } from "./types";

export function ServerDeleteDialog({
  server,
  blocked,
  onDelete,
  onClose,
}: {
  server: Server;
  blocked: boolean;
  onDelete: (server: Server) => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  async function confirm() {
    if (busy || blocked) return;
    setBusy(true);
    setError("");
    try {
      await onDelete(server);
    } catch (error) {
      setError(errorOf(error).message);
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="server-delete-dialog"
      aria-labelledby="server-delete-title"
      aria-describedby="server-delete-description"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id="server-delete-title">删除服务器配置？</h2>
      </header>
      <div className="server-delete-target">
        <strong>{server.name}</strong>
        <span>{addressOf(server)}</span>
      </div>
      <p id="server-delete-description">
        将删除本地服务器配置、保存的密码和标签关联。远端服务器及文件不受影响。
      </p>
      {(blocked || error) && (
        <p className="error" role="alert">
          {blocked ? "请先关闭此服务器的所有会话标签页，再删除配置。" : error}
        </p>
      )}
      <footer className="dialog-actions">
        <button autoFocus disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className="delete-confirm"
          disabled={busy || blocked}
          onClick={() => void confirm()}
        >
          {busy ? "正在删除…" : "确认删除"}
        </button>
      </footer>
    </dialog>
  );
}
