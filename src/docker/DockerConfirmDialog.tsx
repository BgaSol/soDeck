import { useEffect, useRef, useState } from "react";
import { actionName, type DockerPreview } from "../dockerApi";
import { errorOf } from "../types";
export function DockerConfirmDialog({
  preview,
  onConfirm,
  onClose,
}: {
  preview: DockerPreview;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null),
    cancel = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [expired, setExpired] = useState(Date.now() >= preview.expiresAt);
  const pending = useRef(false);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    cancel.current?.focus();
    const timer = window.setTimeout(
      () => setExpired(true),
      Math.max(0, preview.expiresAt - Date.now()),
    );
    return () => {
      clearTimeout(timer);
      if (previous?.isConnected) previous.focus();
    };
  }, [preview]);
  async function confirm() {
    if (pending.current || expired) return;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      await onConfirm();
      onClose();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="login-dialog docker-confirm-dialog"
      aria-labelledby="docker-confirm-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id="docker-confirm-title">
          {actionName(preview.action)}
          {[
            "start",
            "stop",
            "restart",
            "remove",
            "pause",
            "unpause",
            "kill",
          ].includes(preview.action.type)
            ? "容器？"
            : "？"}
        </h2>
      </header>
      <p className="docker-confirm-object">{preview.targetName}</p>
      <dl className="docker-facts">
        <dt>目标引擎</dt>
        <dd>{preview.engine.endpoint}</dd>
        <dt>
          {preview.action.type === "create"
            ? "容器名称"
            : preview.action.type === "compose"
              ? "项目"
              : preview.action.type.startsWith("image")
                ? "镜像 ID"
                : [
                      "volumeCreate",
                      "volumeRemove",
                      "networkCreate",
                      "networkRemove",
                      "networkConnect",
                      "networkDisconnect",
                      "cachePrune",
                    ].includes(preview.action.type)
                  ? "目标"
                  : "容器 ID"}
        </dt>
        <dd className="docker-id">{preview.targetId}</dd>
      </dl>
      {preview.consequences.map((text) => (
        <p key={text}>{text}</p>
      ))}
      {expired && (
        <p className="resource-error" role="status">
          确认已过期，请关闭后重新预览。
        </p>
      )}
      {error && (
        <p className="resource-error" role="alert">
          {error}
        </p>
      )}
      <footer className="dialog-actions">
        <button ref={cancel} disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className={
            preview.action.type === "remove" ||
            preview.action.type === "kill" ||
            preview.action.type === "imageRemove" ||
            preview.action.type === "volumeRemove" ||
            preview.action.type === "networkRemove" ||
            preview.action.type === "networkDisconnect" ||
            preview.action.type === "cachePrune" ||
            (preview.action.type === "compose" &&
              preview.action.mode === "down")
              ? "danger"
              : "primary"
          }
          disabled={busy || expired}
          onClick={() => void confirm()}
        >
          {busy ? "正在提交…" : `确认${actionName(preview.action)}`}
        </button>
      </footer>
    </dialog>
  );
}
