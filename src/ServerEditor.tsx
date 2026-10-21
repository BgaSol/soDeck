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
import { updateDuplicateHost } from "./serverConfiguration";
import type {
  Server,
  SaveServerRequest,
  PasswordChange,
  DuplicatePasswordPolicy,
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
function validateFields(draft: Server, hasJump: boolean, connect: boolean) {
  const errors: Partial<Record<EditorField, string>> = {};
  if (!draft.name.trim()) errors.name = "请填写服务器名称。";
  else if (draft.name.length > 120) errors.name = "名称最多可输入 120 个字符。";
  if (!draft.host.trim()) errors.host = "请填写 IP 地址或域名。";
  if (!Number.isInteger(draft.port) || draft.port < 1 || draft.port > 65535)
    errors.port = "请输入 1–65535 的整数端口。";
  if (draft.jumpServerId !== null && !hasJump)
    errors.jumpServerId = "请选择可用的跳板服务器，或切换为直接连接。";
  if (connect && !draft.username.trim())
    errors.username = "请填写 SSH 用户名后再连接。";
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
  duplicateSource?: Server | null;
  connectDisabled?: boolean;
  onSave: (
    server: SaveServerRequest,
    connect?: boolean,
    policy?: DuplicatePasswordPolicy,
    asNew?: boolean,
  ) => Promise<void>;
  onDelete?: (server: Server) => void;
  onClose: () => void;
}
export function ServerEditor({
  initial,
  duplicateSource,
  connectDisabled = false,
  tags,
  servers,
  onSave,
  onDelete,
  onClose,
}: Props) {
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
    !duplicateSource &&
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
  const initiallyInherit =
    !!duplicateSource?.hasSavedPassword &&
    duplicateSource.authType === "password";
  const [inheritPassword, setInheritPassword] = useState(initiallyInherit);
  const [createInstead, setCreateInstead] = useState(false);
  const [sourceMissing, setSourceMissing] = useState(false);
  const source = createInstead ? null : duplicateSource;
  const submitting = useRef(false);
  const directEdited = useRef(false);
  const [directReset, setDirectReset] = useState(false);
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
  const [validateForConnection, setValidateForConnection] = useState(false);
  const [focusRequest, setFocusRequest] = useState<{
    field: EditorField;
  } | null>(null);
  const fieldErrors = validateFields(draft, !!jump, validateForConnection);
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
    passwordChange.type !== "keep" ||
    inheritPassword !== initiallyInherit ||
    createInstead;
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
    dialog.current?.showModal();
    nameInput.current?.focus();
    if (duplicateSource) nameInput.current?.select();
  }, []);
  function update<K extends keyof Server>(key: K, value: Server[K]) {
    if (key === "directHost" || key === "directPort")
      directEdited.current = true;
    if (key === "host" && source) {
      setDraft((current) =>
        updateDuplicateHost(
          current,
          value as string,
          source,
          directEdited.current,
        ),
      );
      if (
        source.directHost &&
        !directEdited.current &&
        String(value).trim() !== source.host.trim()
      )
        setDirectReset(true);
    } else setDraft((valueDraft) => ({ ...valueDraft, [key]: value }));
    if (key === "authType" && value !== "password" && source) {
      setInheritPassword(false);
      setPasswordChange({ type: "keep" });
    }
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
  async function submit(connect = false) {
    if (
      submitting.current ||
      busy ||
      confirmDiscard ||
      (connect && connectDisabled)
    )
      return;
    const saved = {
      ...draft,
      passwordChange,
      tags: addTag(draft.tags, tag, tags),
    };
    setValidateForConnection(connect);
    setCheckedFields(
      Object.fromEntries(fieldOrder.map((field) => [field, true])),
    );
    setError("");
    const errors = validateFields(saved, !!jump, connect);
    const firstError = fieldOrder.find((field) => errors[field]);
    if (firstError) {
      setFocusRequest({ field: firstError });
      return;
    }
    submitting.current = true;
    invalidatePasswordCopy();
    setBusy(true);
    try {
      const policy: DuplicatePasswordPolicy =
        draft.authType !== "password"
          ? { type: "clear" }
          : passwordChange.type === "set"
            ? passwordChange
            : source && inheritPassword
              ? { type: "inherit" }
              : { type: "clear" };
      await onSave(saved, connect, policy, createInstead);
    } catch (error) {
      const failure = errorOf(error);
      setError(failure.message);
      setSourceMissing(failure.kind === "sourceMissing");
      setBusy(false);
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
        title={
          source ? "复制服务器配置" : initial.id ? "编辑服务器" : "新增服务器"
        }
        subtitle={
          source
            ? `来自 ${source.name} · 保存为独立配置，不修改来源服务器。`
            : "设置服务器地址、连接路径与认证方式。"
        }
        closeLabel="关闭配置窗口"
        onClose={close}
        disabled={busy}
      />
      <form
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void submit(
            (event.nativeEvent as SubmitEvent).submitter?.getAttribute(
              "value",
            ) === "connect",
          );
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
              disabled={usedAsJump}
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
                此服务器已被用作跳板，需保持直接连接。
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
                {source?.authType === "password" && source.hasSavedPassword && (
                  <label className="copy-password-option">
                    <input
                      type="checkbox"
                      checked={inheritPassword && passwordChange.type !== "set"}
                      onChange={(event) => {
                        setInheritPassword(event.target.checked);
                        setPasswordChange({ type: "keep" });
                      }}
                    />
                    复用来源已保存的登录密码
                  </label>
                )}
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
                    source && inheritPassword && passwordChange.type === "keep"
                      ? "复用来源密码，输入新密码可替换"
                      : draft.hasSavedPassword &&
                          !passwordCopyUnavailable &&
                          passwordChange.type === "keep"
                        ? "已保存密码，输入新密码以替换"
                        : "留空则在连接时输入"
                  }
                />
                <div className="password-field-footer">
                  <small className="field-help" id="server-password-help">
                    {passwordCopyUnavailable
                      ? "请重新打开编辑窗口以获取最新配置。"
                      : showPasswordCopy && passwordChange.type === "set"
                        ? "复制的是已保存密码，新密码保存后才生效。"
                        : showPasswordCopy && passwordChange.type === "clear"
                          ? "保存后清空，当前仍可复制已保存密码。"
                          : source &&
                              inheritPassword &&
                              passwordChange.type === "keep"
                            ? "保存时由本机继承密码；之后修改来源密码不会影响副本。"
                            : passwordChange.type === "clear"
                              ? "下次连接时重新输入。"
                              : source && passwordChange.type === "keep"
                                ? "未保存密码，连接时输入。"
                                : "密码保存在本机，连接时自动使用。"}
                  </small>
                  {draft.hasSavedPassword && (
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
                        ? "保留已保存密码"
                        : "清空已保存密码"}
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
                  if (e.key === "Enter") {
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
        {directReset && (
          <p
            className="field-help editor-feedback editor-feedback-info"
            role="status"
          >
            SSH 地址已变更，已清空继承的直传地址，请按需重新填写。
          </p>
        )}
        {sourceMissing && (
          <div className="editor-feedback editor-feedback-info">
            <button
              type="button"
              onClick={() => {
                setCreateInstead(true);
                setInheritPassword(false);
                setSourceMissing(false);
                setError("");
              }}
            >
              保留草稿，改为普通新建
            </button>
          </div>
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
          <footer className="dialog-actions">
            {onDelete && (
              <button
                type="button"
                className="danger quiet push-right"
                disabled={busy}
                onClick={() => onDelete(initial)}
              >
                删除
              </button>
            )}
            <button type="button" onClick={close} disabled={busy}>
              取消
            </button>
            <button
              className={initial.id ? "primary" : ""}
              disabled={busy || invalidJump}
              type="submit"
            >
              {busy ? "保存中…" : source ? "保存副本" : "保存"}
            </button>
            {!initial.id && (
              <button
                className="primary"
                type="submit"
                value="connect"
                disabled={busy || invalidJump || connectDisabled}
              >
                保存并连接
              </button>
            )}
          </footer>
        )}
      </form>
    </dialog>
  );
}
