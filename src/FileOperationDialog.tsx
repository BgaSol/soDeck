import { useEffect, useRef, useState } from "react";
import { X, FilePlus2, FolderPlus, Upload, ArrowRightLeft } from "lucide-react";
import { errorOf } from "./types";
import { transferApi, notifyTransferStarted } from "./transferApi";
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
  const creating = operation.kind === "file" || operation.kind === "directory";
  const destination =
    "draft" in operation ? operation.draft.targetPath : operation.path;
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
      } else {
        await transferApi.start(operation.draft.id, policy);
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
          {"draft" in operation && (
            <div className="operation-destination">
              <span>
                来源 · {operation.draft.sourceName} · {operation.draft.count}{" "}
                个项目
              </span>
              {operation.draft.sourcePaths.map((path) => (
                <strong key={path} title={path}>
                  {path}
                </strong>
              ))}
              {operation.draft.count > operation.draft.sourcePaths.length && (
                <span>
                  另有{" "}
                  {operation.draft.count - operation.draft.sourcePaths.length}{" "}
                  个项目
                </span>
              )}
            </div>
          )}
          <div className="operation-destination">
            <span>
              目标目录
              {"draft" in operation ? ` · ${operation.draft.targetName}` : ""}
            </span>
            <strong title={destination}>{destination}</strong>
          </div>
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
                  <option value="skip">跳过同名项目</option>
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
                  源服务器需已配置目标账号的免交互 SSH 认证。
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
                : policy === "overwrite"
                  ? "确认覆盖并开始"
                  : "开始传输"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
