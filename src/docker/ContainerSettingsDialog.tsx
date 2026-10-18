import { useEffect, useRef, useState } from "react";
import type { ContainerDetail, DockerActionPayload } from "../dockerApi";
import { errorOf } from "../types";
export function ContainerSettingsDialog({
  mode,
  detail,
  onSubmit,
  onClose,
}: {
  mode: "rename" | "restartPolicy";
  detail: ContainerDetail;
  onSubmit: (action: DockerActionPayload) => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null),
    pending = useRef(false);
  const [name, setName] = useState(detail.name),
    [policy, setPolicy] = useState(detail.restartPolicy || "no"),
    [count, setCount] = useState(String(detail.restartMaximum));
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  async function submit() {
    if (pending.current) return;
    if (mode === "rename" && !/^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,254}$/.test(name)) {
      setError(
        "名称须以字母或数字开头，只能包含字母、数字、下划线、点和连字符。",
      );
      return;
    }
    const maximumRetryCount = policy === "on-failure" ? Number(count) : 0;
    if (
      !Number.isInteger(maximumRetryCount) ||
      maximumRetryCount < 0 ||
      maximumRetryCount > 100000
    ) {
      setError("重试次数须为 0–100000 的整数，0 表示不限次数。");
      return;
    }
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      await onSubmit(
        mode === "rename"
          ? { type: "rename", name }
          : { type: "restartPolicy", policy, maximumRetryCount },
      );
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
      aria-labelledby="docker-settings-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <header className="dialog-head">
          <h2 id="docker-settings-title">
            {mode === "rename" ? "重命名容器" : "重启策略"}
          </h2>
        </header>
        <p>{detail.name}</p>
        {mode === "rename" ? (
          <label className="docker-form-field">
            容器名称
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={255}
            />
          </label>
        ) : (
          <>
            <label className="docker-form-field">
              策略
              <select
                autoFocus
                value={policy}
                onChange={(e) => setPolicy(e.target.value)}
              >
                <option value="no">不自动重启</option>
                <option value="always">始终重启</option>
                <option value="unless-stopped">除非手动停止</option>
                <option value="on-failure">仅失败时重启</option>
              </select>
            </label>
            {policy === "on-failure" && (
              <label className="docker-form-field">
                最大重试次数
                <input
                  type="number"
                  min="0"
                  max="100000"
                  value={count}
                  onChange={(e) => setCount(e.target.value)}
                />
                <small>0 表示不限次数</small>
              </label>
            )}
            <p className="resource-help">
              由 Compose
              管理的容器应同时修改项目配置，否则下次重建可能覆盖此设置。
            </p>
          </>
        )}
        {error && (
          <p className="resource-error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          <button type="button" disabled={busy} onClick={onClose}>
            取消
          </button>
          <button className="primary" disabled={busy}>
            {busy ? "正在提交…" : "保存"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
