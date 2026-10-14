import { useEffect, useId, useRef, useState } from "react";
import { KeyRound, Server, ShieldCheck, ShieldAlert, X } from "lucide-react";
import { addressOf, errorOf } from "./types";
import type {
  Credentials,
  AuthChallenge,
  HostKeyChallenge,
  HopView,
} from "./types";

function HopSummary({ hop }: { hop: HopView }) {
  return (
    <div className="connection-hop">
      <span className="connection-hop-icon">
        <Server size={20} aria-hidden="true" />
      </span>
      <div className="connection-hop-details">
        <span className="connection-hop-label">
          {hop.role === "jump" ? "跳板服务器" : "目标服务器"}
          <span className="sr-only">：</span>
        </span>
        <strong>{hop.server.name}</strong>
        <span className="connection-hop-address">
          <span className="connection-hop-user">{hop.server.username}@</span>
          <wbr />
          {addressOf(hop.server)}
        </span>
      </div>
      <span className="connection-hop-step">
        {hop.role === "jump" || hop.index > 0
          ? `第 ${hop.index + 1} 跳`
          : "直接连接"}
      </span>
    </div>
  );
}

export function LoginDialog({
  challenge,
  disabled = false,
  onSubmit,
  onClose,
}: {
  challenge: AuthChallenge;
  disabled?: boolean;
  onSubmit: (credentials: Credentials) => void | Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const passwordInput = useRef<HTMLInputElement>(null);
  const titleId = useId(),
    helpId = useId();
  const pending = useRef(false);
  const server = challenge.hop.server;
  const [password, setPassword] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(challenge.message || "");
  useEffect(() => {
    dialog.current?.showModal();
    passwordInput.current?.focus();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="login-dialog connection-dialog"
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id={titleId}>
          <KeyRound size={20} aria-hidden="true" />
          {server.authType === "key" ? "私钥认证" : "密码认证"}
        </h2>
        <button className="icon-button" onClick={onClose} aria-label="取消连接">
          <X size={16} />
        </button>
      </header>
      <form
        onSubmit={async (event) => {
          event.preventDefault();
          if (pending.current || disabled) return;
          pending.current = true;
          setBusy(true);
          setError("");
          try {
            await onSubmit({
              password: server.authType === "password" ? password : "",
              passphrase: server.authType === "key" ? password : "",
            });
          } catch (error) {
            setError(errorOf(error).message);
          } finally {
            pending.current = false;
            setBusy(false);
          }
        }}
      >
        <HopSummary hop={challenge.hop} />
        <label>
          {server.authType === "key" ? "私钥口令（如无可留空）" : "密码"}
          <input
            ref={passwordInput}
            autoFocus
            type="password"
            autoComplete="off"
            value={password}
            disabled={busy}
            aria-describedby={helpId}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        <p className="field-help" id={helpId}>
          {server.authType === "password"
            ? "认证成功后保存在本机，后续连接自动使用。"
            : "私钥口令仅在当前运行期间使用。"}
        </p>
        {error && (
          <p className="error connection-feedback" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          <button type="button" onClick={onClose}>
            取消
          </button>
          <button className="primary" type="submit" disabled={busy || disabled}>
            {busy ? "认证中…" : "连接"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}

export function FingerprintDialog({
  challenge,
  disabled = false,
  onTrust,
  onClose,
}: {
  challenge: HostKeyChallenge;
  disabled?: boolean;
  onTrust: () => Promise<void>;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const titleId = useId(),
    descriptionId = useId();
  useEffect(() => {
    dialog.current?.showModal();
    cancelButton.current?.focus();
  }, []);
  const Icon = challenge.previous ? ShieldAlert : ShieldCheck;
  return (
    <dialog
      ref={dialog}
      className="login-dialog connection-dialog"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <header className="dialog-head">
        <h2 id={titleId}>
          <Icon size={20} aria-hidden="true" />
          {challenge.previous ? "服务器指纹已变化" : "核对服务器指纹"}
        </h2>
        <button className="icon-button" onClick={onClose} aria-label="取消连接">
          <X size={16} />
        </button>
      </header>
      <div className="connection-dialog-content">
        <HopSummary hop={challenge.hop} />
        <p
          className={
            challenge.previous
              ? "connection-feedback error"
              : "connection-explanation"
          }
          id={descriptionId}
        >
          {challenge.previous
            ? "当前指纹与已保存的记录不同。请确认服务器身份后再更新。"
            : "首次连接此服务器，请核对主机指纹后继续。"}
        </p>
        <dl className="fingerprint-list">
          {challenge.previous && (
            <div className="fingerprint-previous">
              <dt>已保存的指纹</dt>
              <dd>
                <code>{challenge.previous}</code>
              </dd>
            </div>
          )}
          <div>
            <dt>当前指纹</dt>
            <dd>
              <code>{challenge.fingerprint}</code>
            </dd>
          </div>
        </dl>
        <p className="field-help">请与服务器管理员或可信渠道提供的指纹核对。</p>
        {error && (
          <p className="error connection-feedback" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="dialog-actions">
        <button ref={cancelButton} autoFocus onClick={onClose}>
          取消
        </button>
        <button
          className="primary"
          disabled={busy || disabled}
          onClick={async () => {
            if (pending.current || disabled) return;
            pending.current = true;
            setBusy(true);
            try {
              await onTrust();
            } catch (error) {
              setError(errorOf(error).message);
            } finally {
              pending.current = false;
              setBusy(false);
            }
          }}
        >
          {busy
            ? "正在确认…"
            : challenge.previous
              ? "更新指纹并连接"
              : "确认指纹并连接"}
        </button>
      </footer>
    </dialog>
  );
}
