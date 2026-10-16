import { useCallback, useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";

function ConfirmDialog({
  message,
  finish,
}: {
  message: string;
  finish: (answer: boolean) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="login-dialog"
      aria-labelledby="forward-confirm-title"
      onCancel={(e) => {
        e.preventDefault();
        finish(false);
      }}
    >
      <DialogHeader
        id="forward-confirm-title"
        title="确认操作"
        closeLabel="取消操作"
        onClose={() => finish(false)}
      />
      <p>{message}</p>
      <footer className="dialog-actions">
        <button autoFocus onClick={() => finish(false)}>
          取消
        </button>
        <button className="primary" onClick={() => finish(true)}>
          确认
        </button>
      </footer>
    </dialog>
  );
}

export function useForwardConfirm() {
  const [message, setMessage] = useState("");
  const pending = useRef<((value: boolean) => void) | null>(null);
  const confirm = useCallback((message: string) => {
    if (pending.current) return Promise.resolve(false);
    return new Promise<boolean>((resolve) => {
      pending.current = resolve;
      setMessage(message);
    });
  }, []);
  function finish(value: boolean) {
    const resolve = pending.current;
    pending.current = null;
    setMessage("");
    resolve?.(value);
  }
  useEffect(
    () => () => {
      pending.current?.(false);
      pending.current = null;
    },
    [],
  );
  return {
    confirm,
    dialog: message ? (
      <ConfirmDialog message={message} finish={finish} />
    ) : null,
  };
}
