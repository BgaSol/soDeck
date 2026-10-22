import { DialogHeader } from "./DialogHeader";
import "./server-editor.css";
import { useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  Copy,
  Laptop,
  Server as ServerIcon,
  X,
} from "lucide-react";
import { api } from "./api";
import { addTag, addressOf, errorOf } from "./types";
import {
  createServerName,
  needsDirectAddressReview,
} from "./serverConfiguration";
import type {
  Server,
  SaveServerRequest,
  PasswordChange,
  ServerSaveIntent,
} from "./types";
const fieldOrder = [
  "name",
  "host",
  "port",
  "jumpServerId",
  "username",
  "privateKey",
  "directPort",
] as const;
type EditorField = (typeof fieldOrder)[number];
type PasswordCopyState =
  | { phase: "idle" | "copying" | "copied" }
  | { phase: "failed"; unavailable: boolean; message: string };
const fieldLabels: Record<EditorField, string> = {
  name: "名称",
  host: "地址",
  port: "端口",
  jumpServerId: "跳板服务器",
  username: "用户名",
  privateKey: "本地私钥路径",
  directPort: "直连端口",
};
function validateFields(draft: Server, hasJump: boolean) {
  const errors: Partial<Record<EditorField, string>> = {};
  if (!draft.name.trim()) errors.name = "请填写服务器名称。";
  else if (draft.name.length > 120) errors.name = "名称最多可输入 120 个字符。";
  if (!draft.host.trim()) errors.host = "请填写 IP 地址或域名。";
  if (!Number.isInteger(draft.port) || draft.port < 1 || draft.port > 65535)
    errors.port = "请输入 1–65535 的整数端口。";
  if (draft.jumpServerId !== null && !hasJump)
    errors.jumpServerId = "请选择可用的跳板服务器，或切换为直接连接。";
  if (draft.authType === "key" && !draft.privateKey.trim())
    errors.privateKey = "请填写本地私钥文件的完整路径。";
  if (
    !Number.isInteger(draft.directPort) ||
    draft.directPort < 1 ||
    draft.directPort > 65535
  )
    errors.directPort = "请输入 1–65535 的整数端口。";
  return errors;
}
interface Props {
  initial: Server;
  tags: string[];
  servers: Server[];
  selectName?: boolean;
  updateBlocked?: boolean;
  onSave: (
    server: SaveServerRequest,
    intent: ServerSaveIntent,
    autoName: boolean,
  ) => Promise<void>;
  onClose: () => void;
}
export function ServerEditor({
  initial,
  selectName = false,
  updateBlocked = false,
  tags,
  servers,
  onSave,
  onClose,
}: Props) {
  const opener = useRef(
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );
  const [draft, setDraft] = useState(() => structuredClone(initial)),
    [tag, setTag] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const [passwordChange, setPasswordChange] = useState<PasswordChange>({
    type: "keep",
  });
  const [passwordCopy, setPasswordCopy] = useState<PasswordCopyState>({
    phase: "idle",
  });
  const copyGeneration = useRef(0);
  const copyPending = useRef<symbol | null>(null);
  const copyMounted = useRef(false);
  const showPasswordCopy =
    !!initial.id &&
    initial.authType === "password" &&
    initial.hasSavedPassword &&
    draft.authType === "password";
  const passwordCopyUnavailable =
    passwordCopy.phase === "failed" && passwordCopy.unavailable;
  useEffect(() => {
    copyMounted.current = true;
    setPasswordCopy({ phase: "idle" });
    return () => {
      copyMounted.current = false;
      copyGeneration.current++;
      copyPending.current = null;
    };
  }, [initial.id]);
  function invalidatePasswordCopy() {
    copyGeneration.current++;
    setPasswordCopy((current) =>
      current.phase === "copying" ||
      (current.phase === "failed" && current.unavailable)
        ? current
        : { phase: "idle" },
    );
  }
  function changePassword(value: PasswordChange) {
    invalidatePasswordCopy();
    setPasswordChange(value);
  }
  const [sourceMissing, setSourceMissing] = useState(false);
  const [savingIntent, setSavingIntent] = useState<ServerSaveIntent | null>(
    null,
  );
  const submitting = useRef(false);
  const newName = createServerName(draft, initial, servers);
  const candidates = servers.filter(
    (server) => server.id !== draft.id && !server.jumpServerId,
  );
  const jump = candidates.find((server) => server.id === draft.jumpServerId);
  const lastJump = useRef(initial.jumpServerId);
  const invalidJump = draft.jumpServerId !== null && !jump;
  const usedAsJump = servers.some(
    (server) => server.jumpServerId === draft.id && !!draft.id,
  );
  const dialog = useRef<HTMLDialogElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const continueButton = useRef<HTMLButtonElement>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [checkedFields, setCheckedFields] = useState<
    Partial<Record<EditorField, boolean>>
  >({});
  const [focusRequest, setFocusRequest] = useState<{
    field: EditorField;
  } | null>(null);
  const fieldErrors = validateFields(draft, !!jump);
  function fieldControl(field: EditorField, hint?: string) {
    const errorId =
      checkedFields[field] && fieldErrors[field]
        ? `server-${field}-error`
        : undefined;
    return {
      "data-server-field": field,
      "aria-label": fieldLabels[field],
      "aria-invalid": errorId ? true : undefined,
      "aria-describedby":
        [hint, errorId].filter(Boolean).join(" ") || undefined,
      onBlur: () =>
        setCheckedFields((current) => ({ ...current, [field]: true })),
    };
  }
  function fieldError(field: EditorField) {
    return checkedFields[field] && fieldErrors[field] ? (
      <span className="field-error" id={`server-${field}-error`}>
        {fieldErrors[field]}
      </span>
    ) : null;
  }
  useEffect(() => {
    if (!focusRequest) return;
    const control = dialog.current?.querySelector<
      HTMLInputElement | HTMLSelectElement
    >(`[data-server-field="${focusRequest.field}"]`);
    if (!control) return;
    const target = control.disabled
      ? dialog.current?.querySelector<HTMLInputElement>(
          'input[name="connection-mode"][value="direct"]',
        )
      : control;
    target?.focus({ preventScroll: true });
    (target?.closest("label") ?? target)?.scrollIntoView({ block: "nearest" });
    dialog.current
      ?.querySelector(`#server-${focusRequest.field}-error`)
      ?.scrollIntoView({ block: "nearest" });
  }, [focusRequest]);
  const dirty =
    JSON.stringify(initial) !== JSON.stringify(draft) ||
    !!tag.trim() ||
    passwordChange.type !== "keep";
  const close = () => {
    if (busy) return;
    invalidatePasswordCopy();
    if (dirty) setConfirmDiscard(true);
    else onClose();
  };
  useEffect(() => {
    if (confirmDiscard) continueButton.current?.focus();
    else nameInput.current?.focus();
  }, [confirmDiscard]);
  function continueEditing() {
    setConfirmDiscard(false);
  }
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    nameInput.current?.focus();
    if (selectName) nameInput.current?.select();
    return () => {
      modal?.close();
      if (opener.current?.isConnected)
        opener.current.focus({ preventScroll: true });
    };
  }, []);
  function update<K extends keyof Server>(key: K, value: Server[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
    if (key === "authType") {
      invalidatePasswordCopy();
      setCheckedFields((current) => ({ ...current, privateKey: false }));
    }
    if (key === "jumpServerId" && value === null) {
      setCheckedFields((current) => ({ ...current, jumpServerId: false }));
    }
    setError("");
  }
  function append() {
    if (tag.trim()) {
      update("tags", addTag(draft.tags, tag, tags));
      setTag("");
    }
  }
  async function copySavedPassword() {
    if (
      !showPasswordCopy ||
      passwordCopyUnavailable ||
      copyPending.current ||
      busy ||
      submitting.current ||
      confirmDiscard
    )
      return;
    const request = Symbol();
    const generation = ++copyGeneration.current;
    copyPending.current = request;
    setPasswordCopy({ phase: "copying" });
    const current = () =>
      copyMounted.current && copyGeneration.current === generation;
    try {
      await api.copySavedServerPassword(initial.id);
      if (current()) setPasswordCopy({ phase: "copied" });
    } catch (error) {
      if (current()) {
        const failure = errorOf(error);
        const unavailable = ["notFound", "unavailable"].includes(failure.kind);
        setPasswordCopy({
          phase: "failed",
          unavailable,
          message: unavailable
            ? failure.message
            : failure.kind === "clipboard"
              ? "密码复制失败，请重试。"
              : "暂时无法复制已保存密码，请稍后重试。",
        });
      }
    } finally {
      if (copyPending.current === request) {
        copyPending.current = null;
        if (copyMounted.current)
          setPasswordCopy((state) =>
            state.phase === "copying" ? { phase: "idle" } : state,
          );
      }
    }
  }
  const updateRestriction = sourceMissing
    ? "原服务器已删除，无法保存修改。"
    : updateBlocked
      ? "原服务器正在连接，请完成或取消连接后再保存修改。"
      : usedAsJump && draft.jumpServerId !== null
        ? "原服务器正被用作跳板，保存修改需保持直接连接；当前配置可用于新建。"
        : "";
  async function submit(intent: ServerSaveIntent) {
    if (
      submitting.current ||
      busy ||
      confirmDiscard ||
      (intent === "update" && (!initial.id || updateRestriction))
    )
      return;
    const recoverWithoutSource =
      sourceMissing &&
      (draft.authType !== "password" || passwordChange.type !== "keep");
    if (sourceMissing && !recoverWithoutSource) {
      setError("原服务器已删除。请填写新密码，或明确选择不保存密码后再新建。");
      return;
    }
    const saved: SaveServerRequest = {
      ...draft,
      id: recoverWithoutSource ? "" : initial.id,
      name: intent === "create" ? newName : draft.name,
      passwordChange,
      tags: addTag(draft.tags, tag, tags),
    };
    setCheckedFields(
      Object.fromEntries(fieldOrder.map((field) => [field, true])),
    );
    setError("");
    const errors = validateFields(saved, !!jump);
    const firstError = fieldOrder.find((field) => errors[field]);
    if (firstError) {
      setFocusRequest({ field: firstError });
      return;
    }
    submitting.current = true;
    invalidatePasswordCopy();
    setBusy(true);
    setSavingIntent(intent);
    try {
      await onSave(
        saved,
        intent,
        !!initial.id && draft.name.trim() === initial.name.trim(),
      );
    } catch (error) {
      const failure = errorOf(error);
      setError(failure.message);
      if (["sourceMissing", "notFound"].includes(failure.kind))
        setSourceMissing(true);
      setBusy(false);
      setSavingIntent(null);
      submitting.current = false;
    }
  }
  return (
    <dialog
      ref={dialog}
      className="editor-dialog"
      aria-labelledby="server-editor-title"
      onCancel={(event) => {
        event.preventDefault();
        if (confirmDiscard) continueEditing();
        else close();
      }}
    >
      <DialogHeader
        id="server-editor-title"
        title={initial.id ? "服务器配置" : "新增服务器"}
        subtitle={
          initial.id
            ? `当前配置：${initial.name} · ${addressOf(initial)}`
            : "设置服务器地址、连接路径与认证方式。"
        }
        closeLabel="关闭配置窗口"
        onClose={close}
        disabled={busy}
      />
      <form
        noValidate
        onSubmit={(event) => event.preventDefault()}
        onKeyDown={(event) => {
          if (
            event.key === "Enter" &&
            !event.nativeEvent.isComposing &&
            event.target instanceof HTMLInputElement
          )
            event.preventDefault();
        }}
      >
        <div className="editor-fields">
          <fieldset
            className="server-form-section"
            disabled={busy || confirmDiscard}
          >
            <legend>服务器信息</legend>
            <label>
              <span className="field-label">
                名称 <small className="muted">必填</small>
              </span>
              <input
                ref={nameInput}
                autoFocus
                {...fieldControl("name")}
                value={draft.name}
                maxLength={120}
                onChange={(e) => update("name", e.target.value)}
                placeholder="例如 Web-01"
                required
              />
              {fieldError("name")}
              {initial.id && (
                <small className="field-help create-name-preview">
                  新建名称：{newName || "请填写名称"}
                </small>
              )}
            </label>
            <div className="form-columns">
              <label>
                地址
                <input
                  {...fieldControl("host")}
                  value={draft.host}
                  onChange={(e) => update("host", e.target.value)}
                  placeholder="IP 或域名"
                  required
                />
                {fieldError("host")}
              </label>
              <label>
                端口
                <input
                  type="number"
                  min={1}
                  max={65535}
                  {...fieldControl("port")}
                  value={draft.port || ""}
                  onChange={(e) => update("port", Number(e.target.value))}
                  required
                />
                {fieldError("port")}
              </label>
            </div>
          </fieldset>
          <fieldset
            className="server-form-section connection-settings"
            disabled={busy || confirmDiscard}
          >
            <legend>连接路径</legend>
            <fieldset
              className="connection-choice"
              aria-label="连接方式"
              aria-describedby={usedAsJump ? "jump-usage-help" : undefined}
            >
              <label>
                <input
                  type="radio"
                  name="connection-mode"
                  value="direct"
                  checked={draft.jumpServerId === null}
                  onChange={() => {
                    if (draft.jumpServerId)
                      lastJump.current = draft.jumpServerId;
                    update("jumpServerId", null);
                  }}
                />
                <span>
                  <Laptop size={16} aria-hidden="true" />
                  直接连接
                </span>
              </label>
              <label>
                <input
                  type="radio"
                  name="connection-mode"
                  value="jump"
                  checked={draft.jumpServerId !== null}
                  onChange={() =>
                    update("jumpServerId", lastJump.current || "select-jump")
                  }
                />
                <span>
                  <ServerIcon size={16} aria-hidden="true" />
                  通过跳板机
                </span>
              </label>
            </fieldset>
            {usedAsJump && (
              <p className="field-help" id="jump-usage-help">
                此服务器已被用作跳板；保存修改需保持直接连接，新建可选择其他路径。
              </p>
            )}
            {draft.jumpServerId !== null && (
              <div className="jump-settings">
                <label htmlFor="jump-server">跳板服务器</label>
                <select
                  id="jump-server"
                  required
                  value={jump ? draft.jumpServerId : ""}
                  disabled={!candidates.length}
                  {...fieldControl("jumpServerId", "jump-help")}
                  onChange={(e) => {
                    lastJump.current = e.target.value;
                    update("jumpServerId", e.target.value);
                  }}
                >
                  <option value="" disabled>
                    请选择跳板服务器
                  </option>
                  {candidates.map((server) => (
                    <option key={server.id} value={server.id}>
                      {server.name} · {addressOf(server)}
                    </option>
                  ))}
                </select>
                {fieldError("jumpServerId")}
                <p className="field-help" id="jump-help">
                  {!candidates.length
                    ? "还没有可用的跳板服务器。请先保存一台可直接连接的服务器。"
                    : "目标地址需能从跳板服务器访问。两台服务器分别认证。"}
                </p>
                <ol
                  className="route-preview"
                  aria-label={`连接路径：本机 → ${jump?.name || "选择跳板"} → ${draft.name || "目标服务器"}`}
                >
                  <li>
                    <Laptop size={17} aria-hidden="true" />
                    <strong>本机</strong>
                    <small>连接起点</small>
                  </li>
                  <li className={jump ? "" : "route-placeholder"}>
                    <ArrowRight
                      className="route-arrow"
                      size={14}
                      aria-hidden="true"
                    />
                    <ServerIcon size={17} aria-hidden="true" />
                    <strong title={jump?.name}>
                      {jump?.name || "选择跳板"}
                    </strong>
                    <small>跳板服务器</small>
                  </li>
                  <li>
                    <ArrowRight
                      className="route-arrow"
                      size={14}
                      aria-hidden="true"
                    />
                    <ServerIcon size={17} aria-hidden="true" />
                    <strong title={draft.name}>
                      {draft.name || "目标服务器"}
                    </strong>
                    <small>目标服务器</small>
                  </li>
                </ol>
              </div>
            )}
          </fieldset>
          <fieldset
            className="server-form-section"
            disabled={busy || confirmDiscard}
          >
            <legend>登录认证</legend>
            <div className="form-columns equal">
              <label>
                用户名
                <input
                  {...fieldControl("username")}
                  value={draft.username}
                  onChange={(e) => update("username", e.target.value)}
                  placeholder="例如 deploy"
                />
                {fieldError("username")}
              </label>
              <label>
                认证方式
                <select
                  value={draft.authType}
                  onChange={(e) =>
                    update("authType", e.target.value as Server["authType"])
                  }
                >
                  <option value="agent">SSH Agent</option>
                  <option value="password">密码</option>
                  <option value="key">私钥文件</option>
                </select>
              </label>
            </div>
            {draft.authType === "password" && (
              <div className="password-field">
                <div className="password-field-heading">
                  <label htmlFor="server-password">密码</label>
                  <div className="password-saved-actions">
                    {draft.hasSavedPassword && (
                      <span
                        className={`password-state${passwordChange.type === "clear" || passwordCopyUnavailable ? " pending" : ""}`}
                        role="status"
                      >
                        {passwordCopyUnavailable ? (
                          "已保存密码不可用"
                        ) : passwordChange.type === "clear" ? (
                          "保存后清空"
                        ) : passwordChange.type === "set" ? (
                          "保存后替换"
                        ) : (
                          <>
                            <Check size={13} aria-hidden="true" />
                            已保存
                          </>
                        )}
                      </span>
                    )}
                    {showPasswordCopy && (
                      <button
                        type="button"
                        className="quiet password-copy-button"
                        aria-label="复制已保存密码"
                        aria-describedby="server-password-help server-password-copy-feedback"
                        aria-busy={passwordCopy.phase === "copying"}
                        disabled={
                          passwordCopy.phase === "copying" ||
                          passwordCopyUnavailable
                        }
                        onClick={() => void copySavedPassword()}
                      >
                        <Copy size={14} aria-hidden="true" />
                        {passwordCopy.phase === "copying"
                          ? "复制中…"
                          : "复制已保存密码"}
                      </button>
                    )}
                  </div>
                </div>
                <input
                  id="server-password"
                  type="password"
                  aria-describedby="server-password-help"
                  autoComplete="off"
                  value={
                    passwordChange.type === "set" ? passwordChange.value : ""
                  }
                  onChange={(e) =>
                    changePassword(
                      e.target.value
                        ? { type: "set", value: e.target.value }
                        : { type: "keep" },
                    )
                  }
                  placeholder={
                    initial.hasSavedPassword && passwordChange.type === "keep"
                      ? "已保存密码，输入新密码以替换"
                      : "留空则在连接时输入"
                  }
                />
                <div className="password-field-footer">
                  <small className="field-help" id="server-password-help">
                    {passwordCopyUnavailable
                      ? "请重新打开配置窗口以获取最新状态。"
                      : showPasswordCopy && passwordChange.type !== "keep"
                        ? "复制的是当前已保存密码，本次密码设置保存后生效。"
                        : passwordChange.type === "clear"
                          ? "不保存密码，下次连接时输入。"
                          : passwordChange.type === "set"
                            ? "新密码将在保存后生效。"
                            : initial.hasSavedPassword
                              ? "保存修改保留原密码；新建服务器继承此密码。"
                              : "未保存密码，连接时输入。"}
                  </small>
                  {(initial.hasSavedPassword || sourceMissing) && (
                    <button
                      type="button"
                      className="quiet password-action"
                      onClick={() =>
                        changePassword(
                          passwordChange.type === "clear"
                            ? { type: "keep" }
                            : { type: "clear" },
                        )
                      }
                    >
                      {passwordChange.type === "clear"
                        ? "保留原密码"
                        : initial.hasSavedPassword
                          ? "清空已保存密码"
                          : "不保存密码"}
                    </button>
                  )}
                </div>
                {showPasswordCopy && (
                  <div
                    className="password-copy-feedback"
                    id="server-password-copy-feedback"
                  >
                    <span role="status" aria-atomic="true">
                      {passwordCopy.phase === "copying" &&
                        "正在复制已保存密码…"}
                      {passwordCopy.phase === "copied" && (
                        <>
                          <Check size={13} aria-hidden="true" />
                          密码已复制
                        </>
                      )}
                    </span>
                    <span
                      className="field-error"
                      role="alert"
                      aria-atomic="true"
                    >
                      {passwordCopy.phase === "failed" && passwordCopy.message}
                    </span>
                  </div>
                )}
              </div>
            )}
            {draft.authType === "key" && (
              <label>
                本地私钥路径
                <input
                  {...fieldControl("privateKey", "server-private-key-help")}
                  value={draft.privateKey}
                  onChange={(e) => update("privateKey", e.target.value)}
                  placeholder="私钥文件的完整路径"
                />
                {fieldError("privateKey")}
                <small className="muted" id="server-private-key-help">
                  密钥口令在连接时输入。
                </small>
              </label>
            )}
          </fieldset>
          <fieldset
            className="server-form-section"
            disabled={busy || confirmDiscard}
          >
            <legend>服务器间传输</legend>
            <p className="muted">供其他服务器直接连接此服务器时使用。</p>
            <div className="form-columns">
              <label>
                真实地址
                <input
                  aria-label="真实地址"
                  value={draft.directHost ?? ""}
                  onChange={(e) => update("directHost", e.target.value || null)}
                  placeholder="IP 或域名，可稍后填写"
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <label>
                直连端口
                <input
                  type="number"
                  min={1}
                  max={65535}
                  {...fieldControl("directPort")}
                  value={draft.directPort}
                  onChange={(e) => update("directPort", Number(e.target.value))}
                />
                {fieldError("directPort")}
              </label>
            </div>
            <button
              type="button"
              className="quiet"
              onClick={() => {
                update("directHost", draft.host || null);
                update("directPort", draft.port);
              }}
            >
              使用当前连接地址
            </button>
          </fieldset>
          <fieldset
            className="server-form-section server-tags-section"
            disabled={busy || confirmDiscard}
          >
            <legend>
              标签 <span className="muted">可选</span>
            </legend>
            <div className="edit-tags">
              {draft.tags.map((value) => (
                <span className="edit-chip" key={value}>
                  {value}
                  <button
                    type="button"
                    aria-label={`移除标签 ${value}`}
                    onClick={() =>
                      update(
                        "tags",
                        draft.tags.filter((t) => t !== value),
                      )
                    }
                  >
                    <X size={12} />
                  </button>
                </span>
              ))}
            </div>
            <div className="tag-add">
              <input
                value={tag}
                onChange={(e) => setTag(e.target.value)}
                list="existing-tags"
                aria-label="标签"
                placeholder="选择或输入标签"
                maxLength={40}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    append();
                  }
                }}
              />
              <button type="button" onClick={append}>
                添加
              </button>
            </div>
            <datalist id="existing-tags">
              {tags.map((value) => (
                <option key={value} value={value} />
              ))}
            </datalist>
          </fieldset>
        </div>
        {needsDirectAddressReview(draft, initial) && (
          <p
            className="field-help editor-feedback editor-feedback-info"
            role="status"
          >
            真实地址仍为来源地址，请确认。
          </p>
        )}
        {sourceMissing && (
          <p className="field-help editor-feedback">
            草稿已保留。请明确设置新密码或选择不保存密码，再新建服务器。
          </p>
        )}
        {updateRestriction && (
          <p
            id="server-update-restriction"
            className="field-help editor-feedback"
          >
            {updateRestriction}
          </p>
        )}
        {error && (
          <p
            className="error editor-feedback editor-feedback-error"
            role="alert"
          >
            {error}
          </p>
        )}
        {confirmDiscard ? (
          <footer className="dialog-actions editor-discard-actions">
            <p role="alert">有未保存的修改，是否放弃？</p>
            <button
              ref={continueButton}
              type="button"
              onClick={continueEditing}
            >
              继续编辑
            </button>
            <button type="button" className="discard-changes" onClick={onClose}>
              放弃修改
            </button>
          </footer>
        ) : (
          <footer className="dialog-actions editor-save-actions">
            {initial.id && (
              <p className="field-help">
                新建保存为独立配置；保存修改会更新当前服务器。
              </p>
            )}
            <button type="button" onClick={close} disabled={busy}>
              取消
            </button>
            <button
              className={initial.id ? "" : "primary"}
              disabled={busy || invalidJump}
              type="button"
              onClick={() => void submit("create")}
            >
              {savingIntent === "create" ? "新建中…" : "新建服务器"}
            </button>
            {initial.id && (
              <button
                className="primary"
                disabled={busy || invalidJump || !!updateRestriction}
                aria-describedby={
                  updateRestriction ? "server-update-restriction" : undefined
                }
                type="button"
                onClick={() => void submit("update")}
              >
                {savingIntent === "update" ? "保存中…" : "保存修改"}
              </button>
            )}
          </footer>
        )}
      </form>
    </dialog>
  );
}
