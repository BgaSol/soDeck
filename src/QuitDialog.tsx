import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { errorOf } from "./types";

export function QuitDialog({
  details,
  onQuit,
  onClose,
}: {
  details: string;
  onQuit: () => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
    cancel.current?.focus();
  }, []);
  async function quit() {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await onQuit();
    } catch (error) {
      setError(errorOf(error).message);
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="login-dialog"
      aria-labelledby="quit-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id="quit-title">退出 SSH Workspace？</h2>
        <button
          className="icon-button"
          aria-label="取消退出"
          disabled={busy}
          onClick={onClose}
        >
          <X size={17} />
        </button>
      </header>
      <p>{details}。</p>
      <p className="muted">
        退出将关闭连接、停止传输并放弃未保存修改。结果未确认的任务保留记录，下次启动后可核对。
      </p>
      {error && (
        <p className="panel-error" role="alert">
          {error}
        </p>
      )}
      <footer className="dialog-actions">
        <button ref={cancel} disabled={busy} onClick={onClose}>
          继续使用
        </button>
        <button className="primary" disabled={busy} onClick={() => void quit()}>
          {busy ? "正在停止并退出…" : "确认退出"}
        </button>
      </footer>
    </dialog>
  );
}
