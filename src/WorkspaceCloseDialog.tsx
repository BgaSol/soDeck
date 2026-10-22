import { useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";
import { errorOf } from "./types";
export function WorkspaceCloseDialog({
  title,
  details,
  blockers = [],
  onClose,
  onConfirm,
}: {
  title: string;
  details: string;
  blockers?: string[];
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="confirm-dialog"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <DialogHeader
        id="workspace-close-title"
        closeLabel="取消关闭"
        title={title}
        onClose={onClose}
        disabled={busy}
      />
      <p>{details}</p>
      {blockers.map((s) => (
        <p key={s} role="alert">
          {s}
        </p>
      ))}
      {error && <p role="alert">{error}</p>}
      <footer className="dialog-actions">
        <button disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className="danger"
          disabled={busy || blockers.length > 0}
          onClick={async () => {
            setBusy(true);
            try {
              await onConfirm();
              onClose();
            } catch (e) {
              setError(errorOf(e).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "正在关闭…" : "确认关闭"}
        </button>
      </footer>
    </dialog>
  );
}
