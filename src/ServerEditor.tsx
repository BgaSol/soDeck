import { useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  Laptop,
  Server as ServerIcon,
  X,
} from "lucide-react";
import { addTag, addressOf, errorOf } from "./types";
import type { Server, SaveServerRequest, PasswordChange } from "./types";
interface Props {
  initial: Server;
  tags: string[];
  servers: Server[];
  onSave: (server: SaveServerRequest) => Promise<void>;
  onDelete?: (server: Server) => void;
  onClose: () => void;
}
export function ServerEditor({
  initial,
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
  const dirty =
    JSON.stringify(initial) !== JSON.stringify(draft) ||
    !!tag.trim() ||
    passwordChange.type !== "keep";
  const close = () => {
    if (busy) return;
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
  }, []);
  function update<K extends keyof Server>(key: K, value: Server[K]) {
    setDraft((valueDraft) => ({ ...valueDraft, [key]: value }));
    setError("");
  }
  function append() {
    if (tag.trim()) {
      update("tags", addTag(draft.tags, tag, tags));
      setTag("");
    }
  }
  async function submit() {
    if (busy || confirmDiscard) return;
    const saved = {
      ...draft,
      passwordChange,
      tags: addTag(draft.tags, tag, tags),
    };
    if (
      !saved.name.trim() ||
      !saved.host.trim() ||
      !Number.isInteger(saved.port) ||
      saved.port < 1 ||
      saved.port > 65535
    ) {
      setError("请填写名称、地址及 1–65535 的端口。");
      return;
    }
    if (saved.authType === "key" && !saved.privateKey.trim()) {
      setError("请选择私钥认证所使用的本地文件路径。");
      return;
    }
    if (draft.jumpServerId !== null && !jump) {
      setError("请选择可用的跳板服务器。");
      return;
    }
    setBusy(true);
    try {
      await onSave(saved);
    } catch (error) {
      setError(errorOf(error).message);
      setBusy(false);
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
      <header className="dialog-head">
        <div>
          <h2 id="server-editor-title">
            {initial.id ? "编辑服务器" : "新增服务器"}
          </h2>
          <p className="muted editor-subtitle">
            设置服务器地址、连接路径与认证方式。
          </p>
        </div>
        <button
          className="icon-button"
          onClick={close}
          aria-label="关闭配置窗口"
          disabled={busy}
        >
          <X size={18} />
        </button>
      </header>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
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
                value={draft.name}
                maxLength={120}
                onChange={(e) => update("name", e.target.value)}
                placeholder="例如 Web-01"
                required
              />
            </label>
            <div className="form-columns">
              <label>
                地址
                <input
                  value={draft.host}
                  onChange={(e) => update("host", e.target.value)}
                  placeholder="IP 或域名"
                  required
                />
              </label>
              <label>
                端口
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={draft.port || ""}
                  onChange={(e) => update("port", Number(e.target.value))}
                  required
                />
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
                  aria-describedby="jump-help"
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
                  value={draft.username}
                  onChange={(e) => update("username", e.target.value)}
                  placeholder="例如 deploy"
                />
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
                  {draft.hasSavedPassword && (
                    <span
                      className={`password-state${passwordChange.type === "clear" ? " pending" : ""}`}
                      role="status"
                    >
                      {passwordChange.type === "clear" ? (
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
                    setPasswordChange(
                      e.target.value
                        ? { type: "set", value: e.target.value }
                        : { type: "keep" },
                    )
                  }
                  placeholder={
                    draft.hasSavedPassword && passwordChange.type === "keep"
                      ? "已保存密码，输入新密码以替换"
                      : "留空则在连接时输入"
                  }
                />
                <div className="password-field-footer">
                  <small className="field-help" id="server-password-help">
                    {passwordChange.type === "clear"
                      ? "下次连接时重新输入。"
                      : "密码保存在本机，连接时自动使用。"}
                  </small>
                  {draft.hasSavedPassword && (
                    <button
                      type="button"
                      className="quiet password-action"
                      onClick={() =>
                        setPasswordChange(
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
              </div>
            )}
            {draft.authType === "key" && (
              <label>
                本地私钥路径
                <input
                  value={draft.privateKey}
                  onChange={(e) => update("privateKey", e.target.value)}
                  placeholder="私钥文件的完整路径"
                />
                <small className="muted">密钥口令在连接时输入。</small>
              </label>
            )}
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
        {error && (
          <p className="error editor-save-error" role="alert">
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
              className="primary"
              disabled={busy || invalidJump}
              type="submit"
            >
              {busy ? "保存中…" : "保存"}
            </button>
          </footer>
        )}
      </form>
    </dialog>
  );
}
