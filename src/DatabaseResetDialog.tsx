import { useEffect, useRef, useState } from "react";
import { errorOf } from "./types";

export function DatabaseResetDialog({
  details,
  onReset,
  onClose,
}: {
  details: string;
  onReset: () => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  async function confirm() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      await onReset();
    } catch (error) {
      setError(errorOf(error).message);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="database-reset-dialog"
      aria-labelledby="database-reset-title"
      aria-describedby="database-reset-description"
      aria-busy={busy}
      onCancel={(event) => {
        event.preventDefault();
        if (!pending.current) onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id="database-reset-title">备份旧库并重建？</h2>
      </header>
      <p id="database-reset-description">
        将先备份当前旧数据库，再创建空数据库。服务器配置、保存的密码、标签和已确认的主机指纹将清空，旧数据保留在备份中。备份失败不会清空旧数据。
      </p>
      <p className="database-reset-details">{details}</p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <footer className="dialog-actions">
        <button autoFocus disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className="delete-confirm"
          disabled={busy}
          onClick={() => void confirm()}
        >
          {busy ? "正在备份并重建…" : "确认备份并重建"}
        </button>
      </footer>
    </dialog>
  );
}
