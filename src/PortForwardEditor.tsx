import { useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { DialogHeader } from "./DialogHeader";
import {
  endpoint,
  forwardActive,
  type ForwardRule,
  type ForwardView,
} from "./portForwardApi";
import { PortForwardPath } from "./PortForwardPath";
import { sameSettings } from "./portForwardPresentation";
import {
  draftSettings,
  forwardDraft,
  updateTargetPort,
  validateForwardDraft,
  type DraftErrors,
  type ForwardDraft,
  type ForwardSaveIntent,
} from "./portForwardDraft";
import type { ForwardConfirmation } from "./PortForwardConfirm";
import { errorOf, type Server } from "./types";

export function PortForwardEditor({
  initial,
  servers,
  view,
  onSave,
  onClose,
  onDirty,
  confirm,
}: {
  initial: ForwardRule;
  servers: Server[];
  view?: ForwardView;
  onSave: (rule: ForwardRule, intent: ForwardSaveIntent) => Promise<void>;
  onClose: () => void;
  onDirty: (dirty: boolean) => void;
  confirm: (request: ForwardConfirmation) => Promise<boolean>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [baseline] = useState(() => forwardDraft(initial));
  const [draft, setDraft] = useState(baseline);
  const [touched, setTouched] = useState<
    Partial<Record<keyof DraftErrors, boolean>>
  >({});
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [start, setStart] = useState(true),
    [restart, setRestart] = useState(false);
  const dirty =
    JSON.stringify({ ...baseline, localPortEdited: undefined }) !==
    JSON.stringify({ ...draft, localPortEdited: undefined });
  const active = !!view && forwardActive(view);
  const networkChanged =
    active &&
    (!!view.pendingChanges ||
      !sameSettings(draftSettings(draft), view.runtime!.settings));
  const errors = validateForwardDraft(
    draft,
    servers.map((s) => s.id),
  );
  useEffect(() => {
    dialog.current?.showModal();
    dialog.current
      ?.querySelector<HTMLInputElement>("#forward-name")
      ?.focus({ preventScroll: true });
  }, []);
  useEffect(() => {
    onDirty(dirty);
  }, [dirty, onDirty]);
  const server = servers.find((s) => s.id === draft.serverId);
  const jump = servers.find((s) => s.id === server?.jumpServerId);
  const hops = [...(jump ? [jump] : []), ...(server ? [server] : [])];
  const submitLabel = !initial.id
    ? start
      ? "创建并启动"
      : "创建"
    : networkChanged && restart
      ? "保存并重启"
      : "保存";
  async function close() {
    if (
      !busy &&
      (!dirty ||
        (await confirm({
          title: "放弃修改？",
          message: "尚未保存的转发设置将被丢弃。",
          confirmLabel: "放弃修改",
          danger: true,
        })))
    )
      onClose();
  }
  function update<K extends keyof ForwardDraft>(
    key: K,
    value: ForwardDraft[K],
  ) {
    setDraft((d) => ({ ...d, [key]: value }));
  }
  function validation(key: keyof DraftErrors) {
    return {
      id: `forward-${key}`,
      "aria-invalid": !!(touched[key] && errors[key]),
      "aria-describedby":
        touched[key] && errors[key] ? `forward-${key}-error` : undefined,
      onBlur: () => setTouched((t) => ({ ...t, [key]: true })),
    };
  }
  function fieldError(key: keyof DraftErrors) {
    return touched[key] && errors[key] ? (
      <span id={`forward-${key}-error`} className="forward-field-error">
        {errors[key]}
      </span>
    ) : null;
  }
  async function save() {
    if (busy) return;
    const first = Object.keys(errors)[0];
    if (first) {
      setTouched({
        name: true,
        serverId: true,
        targetHost: true,
        targetPort: true,
        localPort: true,
      });
      dialog.current?.querySelector<HTMLElement>(`#forward-${first}`)?.focus();
      return;
    }
    setBusy(true);
    setError("");
    try {
      await onSave(
        {
          ...initial,
          name: draft.name.trim(),
          webScheme: draft.webScheme,
          settings: draftSettings(draft),
        },
        !initial.id && start
          ? "start"
          : networkChanged && restart
            ? "restart"
            : "save",
      );
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  const advancedSummary = [
    draft.listenAddress === "::1" ? "IPv6" : "IPv4",
    draft.autoReconnect ? "自动重连" : "不自动重连",
    draft.webScheme?.toUpperCase(),
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <dialog
      ref={dialog}
      className="forward-editor"
      aria-labelledby="forward-editor-title"
      onCancel={(e) => {
        e.preventDefault();
        void close();
      }}
    >
      <DialogHeader
        id="forward-editor-title"
        title={initial.id ? "编辑端口转发" : "新建端口转发"}
        closeLabel="关闭规则编辑"
        onClose={() => void close()}
        disabled={busy}
      />
      <form
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="forward-editor-scroll">
          <fieldset disabled={busy}>
            <section className="forward-form-section">
              <h3>基本信息</h3>
              <label htmlFor="forward-name">名称</label>
              <input
                {...validation("name")}
                autoFocus
                maxLength={128}
                value={draft.name}
                onChange={(e) => update("name", e.target.value)}
                placeholder="例如：开发数据库"
              />
              {fieldError("name")}
            </section>
            <section className="forward-form-section">
              <h3>远程服务</h3>
              <div className="forward-field">
                <label htmlFor="forward-serverId">SSH 服务器</label>
                <select
                  {...validation("serverId")}
                  value={draft.serverId}
                  onChange={(e) => update("serverId", e.target.value)}
                >
                  <option value="">请选择服务器</option>
                  {servers.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name} · {s.host}
                      {s.jumpServerId ? "（经跳板）" : ""}
                    </option>
                  ))}
                </select>
                {fieldError("serverId")}
              </div>
              <div className="forward-fields">
                <div className="forward-field">
                  <label htmlFor="forward-targetHost">目标地址</label>
                  <input
                    {...validation("targetHost")}
                    maxLength={253}
                    value={draft.targetHost}
                    onChange={(e) => update("targetHost", e.target.value)}
                    placeholder="主机名或 IP"
                  />
                  {fieldError("targetHost")}
                </div>
                <div className="forward-field">
                  <label htmlFor="forward-targetPort">目标端口</label>
                  <input
                    {...validation("targetPort")}
                    inputMode="numeric"
                    value={draft.targetPort}
                    onChange={(e) =>
                      setDraft((d) => updateTargetPort(d, e.target.value))
                    }
                    placeholder="1–65535"
                  />
                  {fieldError("targetPort")}
                </div>
              </div>
              <p className="field-help">
                由所选服务器访问；127.0.0.1 表示服务器自身。
              </p>
            </section>
            <section className="forward-form-section">
              <h3>本地访问</h3>
              <div className="forward-local-form">
                <div
                  className="forward-port-mode"
                  role="group"
                  aria-label="本地端口分配"
                >
                  <label>
                    <input
                      type="radio"
                      name="port-mode"
                      checked={draft.portMode === "fixed"}
                      onChange={() => update("portMode", "fixed")}
                    />
                    固定端口
                  </label>
                  <label>
                    <input
                      type="radio"
                      name="port-mode"
                      checked={draft.portMode === "auto"}
                      onChange={() => update("portMode", "auto")}
                    />
                    自动分配
                  </label>
                </div>
                {draft.portMode === "fixed" && (
                  <div className="forward-field forward-local-port">
                    <label htmlFor="forward-localPort">本地端口</label>
                    <input
                      {...validation("localPort")}
                      inputMode="numeric"
                      value={draft.localPort}
                      onChange={(e) =>
                        setDraft((d) => ({
                          ...d,
                          localPort: e.target.value,
                          localPortEdited: true,
                        }))
                      }
                      placeholder="与目标端口一致"
                    />
                    {fieldError("localPort")}
                  </div>
                )}
              </div>
              <p className="field-help">
                {draft.portMode === "auto"
                  ? "启动后分配可用端口，停止后再启动可能更换；同次运行重连保持端口。"
                  : "本机应用通过此端口连接远程服务，仅允许本机访问。"}
              </p>
            </section>
            <details className="forward-advanced">
              <summary>
                <ChevronRight size={14} />
                <strong>高级选项</strong>
                <span>{advancedSummary}</span>
              </summary>
              <div className="forward-advanced-fields">
                <div className="forward-fields">
                  <label className="forward-field">
                    本地地址
                    <select
                      value={draft.listenAddress}
                      onChange={(e) => update("listenAddress", e.target.value)}
                    >
                      <option value="127.0.0.1">127.0.0.1 · IPv4</option>
                      <option value="::1">::1 · IPv6</option>
                    </select>
                  </label>
                  <label className="forward-field">
                    网页打开方式
                    <select
                      value={draft.webScheme || ""}
                      onChange={(e) =>
                        update(
                          "webScheme",
                          (e.target.value || null) as ForwardRule["webScheme"],
                        )
                      }
                    >
                      <option value="">无</option>
                      <option value="http">HTTP</option>
                      <option value="https">HTTPS</option>
                    </select>
                  </label>
                </div>
                <label className="forward-check">
                  <input
                    type="checkbox"
                    checked={draft.autoReconnect}
                    onChange={(e) => update("autoReconnect", e.target.checked)}
                  />
                  断线自动重连
                </label>
              </div>
            </details>
            <PortForwardPath
              hops={hops}
              local={
                draft.portMode === "auto"
                  ? "自动分配端口"
                  : `${draft.listenAddress === "::1" ? "[::1]" : draft.listenAddress}:${draft.localPort || "端口"}`
              }
              target={
                draft.targetHost
                  ? `${endpoint(draft.targetHost.replace(/^\[|\]$/g, ""), null).replace(/:自动$/, "")}:${draft.targetPort || "端口"}`
                  : "目标服务"
              }
            />
            {networkChanged && (
              <section className="forward-apply-choice">
                <label htmlFor="forward-apply">应用更改</label>
                <select
                  id="forward-apply"
                  value={restart ? "restart" : "later"}
                  onChange={(e) => setRestart(e.target.value === "restart")}
                >
                  <option value="later">下次启动时生效</option>
                  <option value="restart">保存后重新启动</option>
                </select>
                <p className="field-help">
                  {restart
                    ? "重新启动会断开当前业务连接，并应用保存的配置。"
                    : "当前转发保持运行，名称和网页打开方式会立即更新。"}
                </p>
              </section>
            )}
          </fieldset>
          {error && (
            <p className="panel-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="dialog-actions forward-editor-footer">
          {!initial.id && (
            <label className="forward-check">
              <input
                type="checkbox"
                disabled={busy}
                checked={start}
                onChange={(e) => setStart(e.target.checked)}
              />
              创建后启动
            </label>
          )}
          <div className="forward-actions">
            <button type="button" disabled={busy} onClick={() => void close()}>
              取消
            </button>
            <button
              className="primary"
              type="submit"
              disabled={busy || !servers.length}
            >
              {busy ? "正在保存…" : submitLabel}
            </button>
          </div>
        </footer>
      </form>
    </dialog>
  );
}
