import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { addTag, errorOf } from "./types";
import type { Server } from "./types";
interface Props {
  initial: Server;
  tags: string[];
  onSave: (server: Server) => Promise<void>;
  onDelete?: (server: Server) => Promise<void>;
  onClose: () => void;
}
export function ServerEditor({
  initial,
  tags,
  onSave,
  onDelete,
  onClose,
}: Props) {
  const [draft, setDraft] = useState(() => structuredClone(initial)),
    [tag, setTag] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const dirty =
    JSON.stringify(initial) !== JSON.stringify(draft) || !!tag.trim();
  const close = () => {
    if (!busy && (!dirty || window.confirm("有未保存的修改，确定放弃？")))
      onClose();
  };
  useEffect(() => {
    dialog.current?.showModal();
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
    const saved = { ...draft, tags: addTag(draft.tags, tag, tags) };
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
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
    >
      <header className="dialog-head">
        <div>
          <span className="eyebrow">服务器配置</span>
          <h2>{initial.id ? "编辑服务器" : "新增服务器"}</h2>
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
        <label>
          <span className="field-label">
            名称 <small className="muted">必填</small>
          </span>
          <input
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
          <label>
            密码
            <input
              type="password"
              aria-label="密码"
              aria-describedby="server-password-help"
              autoComplete="off"
              value={draft.password}
              onChange={(e) => update("password", e.target.value)}
              placeholder="留空则在连接时输入"
            />
            <small className="muted" id="server-password-help">
              保存在本机数据库中，连接时自动使用。
            </small>
          </label>
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
        <label>
          标签
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
        </label>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          {onDelete && (
            <button
              type="button"
              className="danger quiet push-right"
              disabled={busy}
              onClick={async () => {
                if (
                  !window.confirm(`删除 ${initial.name}（${initial.host}）？`)
                )
                  return;
                setBusy(true);
                try {
                  await onDelete(initial);
                } catch (error) {
                  setError(errorOf(error).message);
                  setBusy(false);
                }
              }}
            >
              删除
            </button>
          )}
          <button type="button" onClick={close} disabled={busy}>
            取消
          </button>
          <button className="primary" disabled={busy} type="submit">
            {busy ? "保存中…" : "保存"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
