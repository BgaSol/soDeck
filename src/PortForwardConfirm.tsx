import { useCallback, useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";

export interface ForwardConfirmation {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
}

function ConfirmDialog({
  request,
  finish,
}: {
  request: ForwardConfirmation;
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
        title={request.title}
        closeLabel="取消操作"
        onClose={() => finish(false)}
      />
      <p>{request.message}</p>
      <footer className="dialog-actions">
        <button autoFocus onClick={() => finish(false)}>
          取消
        </button>
        <button
          className={request.danger ? "primary forward-danger" : "primary"}
          onClick={() => finish(true)}
        >
          {request.confirmLabel}
        </button>
      </footer>
    </dialog>
  );
}

export function useForwardConfirm() {
  const [request, setRequest] = useState<ForwardConfirmation | null>(null);
  const pending = useRef<((value: boolean) => void) | null>(null);
  const confirm = useCallback((request: ForwardConfirmation) => {
    if (pending.current) return Promise.resolve(false);
    return new Promise<boolean>((resolve) => {
      pending.current = resolve;
      setRequest(request);
    });
  }, []);
  function finish(value: boolean) {
    const resolve = pending.current;
    pending.current = null;
    setRequest(null);
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
    dialog: request ? (
      <ConfirmDialog request={request} finish={finish} />
    ) : null,
  };
}
