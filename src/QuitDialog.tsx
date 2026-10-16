import { useEffect, useRef, useState } from "react";
import { errorOf } from "./types";
import { APP_NAME } from "./brand";

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
        <h2 id="quit-title">退出 {APP_NAME}？</h2>
      </header>
      <p>{details}。</p>
      <p className="muted">
        退出将关闭连接、停止端口转发和文件任务，并按确认内容处理未保存修改。已经删除的内容无法恢复。任务记录会保留，下次启动后可查看、核对；删除任务和端口转发不会自动继续。
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
