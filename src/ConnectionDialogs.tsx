import { useEffect, useRef, useState } from "react";
import { ShieldCheck, X } from "lucide-react";
import { addressOf, errorOf } from "./types";
import type { AppError, Credentials, Server } from "./types";

export function LoginDialog({
  server,
  onSubmit,
  onClose,
}: {
  server: Server;
  onSubmit: (credentials: Credentials) => void | Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [password, setPassword] = useState(
      server.authType === "password" ? server.password : "",
    ),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="login-dialog"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header className="dialog-head">
        <h2>连接 {server.name}</h2>
        <button
          className="icon-button"
          onClick={onClose}
          aria-label="关闭"
          disabled={busy}
        >
          <X size={18} />
        </button>
      </header>
      <form
        onSubmit={async (event) => {
          event.preventDefault();
          if (busy) return;
          setBusy(true);
          setError("");
          try {
            await onSubmit({
              password: server.authType === "password" ? password : "",
              passphrase: server.authType === "key" ? password : "",
            });
          } catch (error) {
            setError(errorOf(error).message);
            setBusy(false);
          }
        }}
      >
        <p className="muted">
          {server.username}@{addressOf(server)}
        </p>
        <label>
          {server.authType === "key" ? "私钥口令（如无可留空）" : "密码"}
          <input
            autoFocus
            type="password"
            autoComplete="off"
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        <small className="muted">
          {server.authType === "password"
            ? "密码保存在本机数据库中，后续连接自动使用。"
            : "私钥口令仅在当前运行期间使用。"}
        </small>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          <button type="button" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="primary" type="submit" disabled={busy}>
            {busy ? "保存中…" : "连接"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
export function FingerprintDialog({
  error,
  onTrust,
  onClose,
}: {
  error: AppError;
  onTrust: () => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog ref={dialog} className="login-dialog" onCancel={onClose}>
      <header className="dialog-head">
        <h2>
          <ShieldCheck size={20} />
          核对服务器指纹
        </h2>
      </header>
      <p>{error.message}</p>
      {error.previous && (
        <label>
          原指纹<code>{error.previous}</code>
        </label>
      )}
      <label>
        当前指纹<code>{error.fingerprint}</code>
      </label>
      <p className="muted">请与服务器管理员或可信渠道提供的指纹核对。</p>
      <footer className="dialog-actions">
        <button onClick={onClose}>取消</button>
        <button className="primary" onClick={onTrust}>
          确认指纹并连接
        </button>
      </footer>
    </dialog>
  );
}
