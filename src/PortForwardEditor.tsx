import { useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";
import { endpoint, type ForwardRule } from "./portForwardApi";
import type { Server } from "./types";
import { errorOf } from "./types";

export function PortForwardEditor({
  initial,
  servers,
  active,
  onSave,
  onClose,
  onDirty,
  confirm,
}: {
  initial: ForwardRule;
  servers: Server[];
  active: boolean;
  onSave: (rule: ForwardRule, start: boolean) => Promise<void>;
  onClose: () => void;
  onDirty: (dirty: boolean) => void;
  confirm: (message: string) => Promise<boolean>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [rule, setRule] = useState(() => structuredClone(initial));
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const dirty = JSON.stringify(initial) !== JSON.stringify(rule);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  useEffect(() => {
    onDirty(dirty);
  }, [dirty, onDirty]);
  const server = servers.find((s) => s.id === rule.settings.serverId);
  const jump = servers.find((s) => s.id === server?.jumpServerId);
  async function close() {
    if (!busy && (!dirty || (await confirm("放弃尚未保存的转发规则修改？"))))
      onClose();
  }
  function setting<K extends keyof ForwardRule["settings"]>(
    key: K,
    value: ForwardRule["settings"][K],
  ) {
    setRule((r) => ({ ...r, settings: { ...r.settings, [key]: value } }));
  }
  async function save(start: boolean) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await onSave(rule, start);
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="forward-editor"
      aria-labelledby="forward-editor-title"
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <DialogHeader
        id="forward-editor-title"
        title={initial.id ? "编辑端口转发" : "新建端口转发"}
        closeLabel="关闭规则编辑"
        onClose={close}
      />
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void save(false);
        }}
      >
        <fieldset disabled={busy}>
          <label>
            名称
            <input
              autoFocus
              required
              maxLength={128}
              value={rule.name}
              onChange={(e) => setRule({ ...rule, name: e.target.value })}
              placeholder="例如：开发数据库"
            />
          </label>
          <label>
            经由服务器
            <select
              required
              value={rule.settings.serverId}
              onChange={(e) => setting("serverId", e.target.value)}
            >
              <option value="">请选择服务器</option>
              {servers.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} · {s.host}
                  {s.jumpServerId ? "（经跳板）" : ""}
                </option>
              ))}
            </select>
          </label>
          <p className="field-help">
            本机 → {jump ? jump.name + " → " : ""}
            {server?.name || "SSH 服务器"}
          </p>
          <div className="forward-fields">
            <label>
              目标地址
              <input
                required
                maxLength={253}
                value={rule.settings.targetHost}
                onChange={(e) => setting("targetHost", e.target.value)}
                placeholder="db.internal 或 127.0.0.1"
              />
            </label>
            <label>
              目标端口
              <input
                required
                type="number"
                min={1}
                max={65535}
                value={rule.settings.targetPort || ""}
                onChange={(e) => setting("targetPort", Number(e.target.value))}
              />
            </label>
          </div>
          <p className="field-help">
            由经由服务器访问；127.0.0.1 / ::1 表示该服务器自身。请填写主机名或
            IP，不含协议和路径。
          </p>
          <div className="forward-fields">
            <label>
              本地地址
              <select
                value={rule.settings.listenAddress}
                onChange={(e) => setting("listenAddress", e.target.value)}
              >
                <option value="127.0.0.1">127.0.0.1 · IPv4</option>
                <option value="::1">::1 · IPv6</option>
              </select>
            </label>
            <label>
              本地端口
              <input
                type="number"
                min={1}
                max={65535}
                placeholder="留空：自动分配"
                value={rule.settings.listenPort ?? ""}
                onChange={(e) =>
                  setting(
                    "listenPort",
                    e.target.value ? Number(e.target.value) : null,
                  )
                }
              />
            </label>
          </div>
          <label className="forward-check">
            <input
              type="checkbox"
              checked={rule.settings.autoReconnect}
              onChange={(e) => setting("autoReconnect", e.target.checked)}
            />
            断线自动重连
          </label>
          <label>
            网页打开方式
            <select
              value={rule.webScheme || ""}
              onChange={(e) =>
                setRule({
                  ...rule,
                  webScheme: (e.target.value ||
                    null) as ForwardRule["webScheme"],
                })
              }
            >
              <option value="">无</option>
              <option value="http">HTTP</option>
              <option value="https">HTTPS</option>
            </select>
          </label>
          <p className="forward-route">
            {endpoint(rule.settings.listenAddress, rule.settings.listenPort)}
            {" → "}
            {server?.name || "SSH"}
            {" → "}
            {endpoint(
              rule.settings.targetHost || "目标",
              rule.settings.targetPort,
            )}
          </p>
          {active && (
            <p className="forward-notice">
              当前转发继续使用启动时的配置。保存后，可在详情中选择“停止并应用”。
            </p>
          )}
        </fieldset>
        {error && (
          <p className="panel-error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          <button type="button" disabled={busy} onClick={close}>
            取消
          </button>
          <button type="submit" disabled={busy || !servers.length}>
            {active ? "保存，下次启动生效" : "保存"}
          </button>
          {!active && (
            <button
              className="primary"
              disabled={busy || !servers.length}
              type="button"
              onClick={(e) => {
                if (e.currentTarget.form?.reportValidity()) void save(true);
              }}
            >
              保存并启动
            </button>
          )}
        </footer>
      </form>
    </dialog>
  );
}
