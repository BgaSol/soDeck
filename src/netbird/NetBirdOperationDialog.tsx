import { createPortal } from "react-dom";
import { useContext, useLayoutEffect } from "react";
import {
  WorkspaceInteractionContext,
  canRestoreFocus,
} from "../WorkspaceInteractionContext";
import { useEffect, useId, useRef, useState } from "react";
import type { OperationRequest, Preview } from "./netbirdApi";
import { actionLabels, at } from "./netbirdPresentation";
import { errorOf } from "../types";
export function NetBirdOperationDialog({
  preview,
  visible = true,
  workspaceActive = true,
  serverName,
  onConfirm,
  onClose,
}: {
  preview: Preview;
  visible?: boolean;
  workspaceActive?: boolean;
  serverName: string;
  onConfirm: (request: OperationRequest) => Promise<void>;
  onClose: () => void;
}) {
  const id = useId(),
    dialog = useRef<HTMLDialogElement>(null),
    cancel = useRef<HTMLButtonElement>(null),
    pending = useRef(false);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [expired, setExpired] = useState(Date.now() >= preview.expiresAt),
    [attempted, setAttempted] = useState(false);
  const [request] = useState<OperationRequest>(() => ({
    requestId: crypto.randomUUID(),
    bindingToken: preview.bindingToken,
    action: preview.action,
    previewId: preview.id,
  }));
  const interaction = useContext(WorkspaceInteractionContext);
  const interactionRef = useRef(interaction);
  interactionRef.current = interaction;
  const presented = useRef(false);
  const previous = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    if (workspaceActive && (presented.current || visible)) {
      if (!dialog.current?.open) {
        if (!presented.current)
          previous.current = document.activeElement as HTMLElement | null;
        dialog.current?.showModal();
        cancel.current?.focus();
        presented.current = true;
      }
    } else dialog.current?.close();
  }, [workspaceActive, visible]);
  useEffect(() => {
    const element = dialog.current;
    const timer = setTimeout(
      () => setExpired(true),
      Math.max(0, preview.expiresAt - Date.now()),
    );
    return () => {
      clearTimeout(timer);
      element?.close();
      if (!presented.current) return;
      queueMicrotask(() => {
        if (document.querySelector("dialog[open]")) return;
        if (interactionRef.current)
          interactionRef.current.restoreFocus(previous.current, "services");
        else if (canRestoreFocus(previous.current)) previous.current.focus();
      });
    };
  }, [preview]);
  async function confirm() {
    if (
      !workspaceActive ||
      pending.current ||
      !preview.allowed ||
      (expired && !attempted)
    )
      return;
    pending.current = true;
    setBusy(true);
    setError("");
    setAttempted(true);
    try {
      await onConfirm(request);
      onClose();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  const path = preview.path.result;
  return createPortal(
    <dialog
      ref={dialog}
      className="login-dialog nb-confirm"
      aria-labelledby={id}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id={id}>{actionLabels[preview.action]}</h2>
      </header>
      <p className="nb-confirm-server">{serverName}</p>
      <div
        className={`nb-path-result nb-${path === "independent" ? "good" : "warning"}`}
      >
        <strong>
          {path === "independent"
            ? "当前 SSH 路径未发现 NetBird 依赖"
            : path === "dependent"
              ? "当前 SSH 依赖 NetBird"
              : "无法确认 SSH 路径"}
        </strong>
        {preview.path.reasons.map((reason) => (
          <p key={reason}>{reason}</p>
        ))}
        <p className="nb-secondary">检查于 {at(preview.path.checkedAt)}</p>
      </div>
      {preview.consequences.map((text) => (
        <p key={text}>{text}</p>
      ))}
      {!preview.allowed && (
        <p className="nb-warning">
          操作已阻止。请使用独立的公网、局域网或控制台路径，并检查客户端能力。
        </p>
      )}
      {expired && !attempted && (
        <p className="nb-warning" role="status">
          预检已过期，请关闭后重新检查。
        </p>
      )}
      {error && (
        <p className="nb-error" role="alert">
          {error}
          {attempted && "。可检查同一请求的提交结果，不会重复已接收的命令。"}
        </p>
      )}
      <footer className="dialog-actions">
        <button ref={cancel} disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className={
            preview.action === "down" || preview.action === "serviceRestart"
              ? "danger"
              : "primary"
          }
          disabled={busy || !preview.allowed || (expired && !attempted)}
          onClick={() => void confirm()}
        >
          {busy
            ? "正在提交…"
            : attempted
              ? "检查提交结果"
              : actionLabels[preview.action]}
        </button>
      </footer>
    </dialog>,
    document.body,
  );
}
