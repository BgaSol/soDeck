import { useEffect, useRef, useState } from "react";
import { X, FilePlus2, FolderPlus, Upload, ArrowRightLeft } from "lucide-react";
import { errorOf } from "./types";
import { transferApi, notifyTransferStarted } from "./transferApi";
import type { TransferAuth } from "./transferApi";
import { TransferAuthFields } from "./TransferAuthFields";
import type { ConflictPolicy, TransferDraft } from "./transferApi";

export type FileOperation =
  | { kind: "file" | "directory"; sessionId: string; path: string }
  | { kind: "upload" | "direct"; draft: TransferDraft };
export function FileOperationDialog({
  operation,
  onClose,
  onCreated,
}: {
  operation: FileOperation;
  onClose: () => void;
  onCreated: (path: string, directory: boolean) => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [policy, setPolicy] = useState<ConflictPolicy>("rename");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [auth, setAuth] = useState<TransferAuth>({
    mode:
      "draft" in operation && operation.draft.savedPasswordAvailable
        ? "savedPassword"
        : "password",
  });
  const [verifySha256, setVerifySha256] = useState(false);
  const [draft, setDraft] = useState(
    "draft" in operation ? operation.draft : null,
  );
  const [host, setHost] = useState(draft?.directEndpoint?.host ?? "");
  const [port, setPort] = useState(String(draft?.directEndpoint?.port ?? 22));
  const [editingEndpoint, setEditingEndpoint] = useState(
    draft?.endpointRequired ?? false,
  );
  const creating = operation.kind === "file" || operation.kind === "directory";
  const destination = draft
    ? draft.targetPath
    : "path" in operation
      ? operation.path
      : "";
  const title =
    operation.kind === "file"
      ? "新建文件"
      : operation.kind === "directory"
        ? "新建文件夹"
        : operation.kind === "upload"
          ? "上传到服务器"
          : "复制到服务器";
  useEffect(() => {
    ref.current?.showModal();
    input.current?.focus();
  }, []);
  async function submit() {
    if (busy) return;
    setError("");
    if (
      creating &&
      (!name ||
        name === "." ||
        name === ".." ||
        name.includes("/") ||
        name.includes("\0"))
    ) {
      setError("请输入有效名称，不能包含 / 或使用 . 和 ..。");
      return;
    }
    setBusy(true);
    try {
      if ("sessionId" in operation) {
        const path = await transferApi.create(
          operation.sessionId,
          operation.path,
          name,
          operation.kind,
        );
        onCreated(path, operation.kind === "directory");
      } else if (draft) {
        if (editingEndpoint) {
          const directPort = Number(port);
          if (
            !host.trim() ||
            !Number.isInteger(directPort) ||
            directPort < 1 ||
            directPort > 65535
          ) {
            setError("请填写真实地址，以及 1–65535 之间的端口。");
            setBusy(false);
            return;
          }
          const updated = await transferApi.setEndpoint(
            draft.id,
            host.trim(),
            directPort,
          );
          setDraft(updated);
          setEditingEndpoint(false);
          setBusy(false);
          return;
        }
        await transferApi.start(
          draft.id,
          policy,
          operation.kind === "direct" ? auth : undefined,
          verifySha256,
        );
        setAuth({ mode: "password" });
        notifyTransferStarted();
      }
      onClose();
    } catch (error) {
      setError(errorOf(error).message);
      setBusy(false);
    }
  }
  const Icon =
    operation.kind === "file"
      ? FilePlus2
      : operation.kind === "directory"
        ? FolderPlus
        : operation.kind === "upload"
          ? Upload
          : ArrowRightLeft;
  return (
    <dialog
      ref={ref}
      className="file-operation-dialog"
      aria-labelledby="file-operation-title"
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
          <h2 id="file-operation-title">
            <Icon size={18} />
            {title}
          </h2>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭文件操作"
            disabled={busy}
            onClick={onClose}
          >
            <X size={17} />
          </button>
        </header>
        <div className="file-operation-fields">
          {draft && (
            <div className="operation-destination">
              <span>
                来源 · {draft.sourceName} · {draft.count} 个项目
              </span>
              {draft.sourcePaths.map((path) => (
                <strong key={path} title={path}>
                  {path}
                </strong>
              ))}
              {draft.count > draft.sourcePaths.length && (
                <span>
                  另有 {draft.count - draft.sourcePaths.length} 个项目
                </span>
              )}
            </div>
          )}
          <div className="operation-destination">
            <span>
              目标目录
              {draft ? ` · ${draft.targetName}` : ""}
            </span>
            <strong title={destination}>{destination}</strong>
          </div>
          {operation.kind === "direct" && draft && (
            <section className="direct-endpoint" aria-label="本次直连目标">
              <div className="direct-endpoint-heading">
                <strong>本次直连目标</strong>
                {!editingEndpoint && (
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() => setEditingEndpoint(true)}
                  >
                    修改地址
                  </button>
                )}
              </div>
              {editingEndpoint ? (
                <>
                  <p className="muted">
                    填写源服务器能够访问的目标地址，保存后确认本次连接。
                  </p>
                  <div className="direct-endpoint-inputs">
                    <label>
                      真实地址
                      <input
                        aria-label="本次直连真实地址"
                        value={host}
                        disabled={busy}
                        onChange={(e) => setHost(e.target.value)}
                        autoComplete="off"
                        spellCheck={false}
                        placeholder="例如 10.0.0.20"
                      />
                    </label>
                    <label>
                      端口
                      <input
                        aria-label="本次直连端口"
                        value={port}
                        type="number"
                        min={1}
                        max={65535}
                        disabled={busy}
                        onChange={(e) => setPort(e.target.value)}
                      />
                    </label>
                  </div>
                  {draft.directEndpoint && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy}
                      onClick={() => {
                        setEditingEndpoint(false);
                        setHost(draft.directEndpoint!.host);
                        setPort(String(draft.directEndpoint!.port));
                        setError("");
                      }}
                    >
                      保留当前地址
                    </button>
                  )}
                </>
              ) : (
                draft.directEndpoint && (
                  <>
                    <code>
                      {draft.directEndpoint.username}@
                      {draft.directEndpoint.host.includes(":")
                        ? `[${draft.directEndpoint.host}]`
                        : draft.directEndpoint.host}
                      :{draft.directEndpoint.port}
                    </code>
                    <p className="muted">
                      本次任务使用此地址和当前会话已验证的主机身份。
                    </p>
                  </>
                )
              )}
            </section>
          )}
          {operation.kind === "direct" && (
            <TransferAuthFields
              value={auth}
              onChange={setAuth}
              disabled={busy}
              saved={draft?.savedPasswordAvailable ?? false}
            />
          )}
          {!creating && (
            <label>
              <input
                type="checkbox"
                checked={verifySha256}
                onChange={(e) => setVerifySha256(e.target.checked)}
              />
              逐文件 SHA-256 复核（额外读取，远端需有 sha256sum）
            </label>
          )}
          {creating ? (
            <label>
              名称
              <input
                ref={input}
                aria-label="新项目名称"
                value={name}
                disabled={busy}
                onChange={(e) => {
                  setName(e.target.value);
                  setError("");
                }}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
          ) : (
            <>
              <label>
                目标已有同名项目时
                <select
                  aria-label="传输同名处理"
                  value={policy}
                  disabled={busy}
                  onChange={(e) => setPolicy(e.target.value as ConflictPolicy)}
                >
                  <option value="rename">保留两份，自动重命名</option>
                  <option value="skip">跳过同名文件，合并目录</option>
                  <option value="overwrite">覆盖文件，合并目录</option>
                </select>
              </label>
              <p className="muted">
                文件夹保留层级、隐藏文件和空目录。符号链接与特殊文件将跳过并显示在结果中。
              </p>
              {policy === "overwrite" && (
                <p className="operation-warning">
                  开始后将替换目标中的同名文件；目录中的其他内容保留。
                </p>
              )}
              {operation.kind === "direct" && (
                <p className="muted">
                  源服务器直接连接目标服务器。源文件保留，无法直连时任务报错。
                  按上方所选方式完成密码或密钥认证。
                </p>
              )}
            </>
          )}
          {error && (
            <p className="panel-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="dialog-actions">
          <button type="button" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="primary" disabled={busy || (creating && !name)}>
            {busy
              ? "正在处理…"
              : creating
                ? "创建"
                : editingEndpoint
                  ? "保存地址并确认"
                  : policy === "overwrite"
                    ? "确认覆盖并开始"
                    : "开始传输"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
