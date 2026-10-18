import { useEffect, useRef, useState } from "react";
import {
  dockerApi,
  transferActionNames,
  type DockerTransferKind,
  type DockerOperationRequest,
  type DockerPreview,
} from "../dockerApi";
import { useDockerManagement } from "../DockerManagementProvider";
import { errorOf } from "../types";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
export function DockerTransferDialog({
  kind,
  targetId,
  name,
  onClose,
}: {
  kind: DockerTransferKind;
  targetId: string;
  name: string;
  onClose: () => void;
}) {
  const { store, state } = useDockerManagement();
  const dialog = useRef<HTMLDialogElement>(null),
    flight = useRef(false),
    request = useRef<DockerOperationRequest | null>(null);
  const [path, setPath] = useState("/tmp"),
    [conflict, setConflict] = useState("rename"),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [preview, setPreview] = useState<DockerPreview | null>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    if (!preview) dialog.current?.showModal();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [preview]);
  const upload = kind === "imageLoad" || kind === "copyIn";
  if (preview)
    return (
      <DockerConfirmDialog
        preview={preview}
        onClose={() => setPreview(null)}
        onConfirm={async () => {
          if (!request.current) return;
          await dockerApi.submit(store.sessionId, request.current);
          onClose();
        }}
      />
    );
  return (
    <dialog
      ref={dialog}
      className="login-dialog docker-confirm-dialog"
      aria-label={transferActionNames[kind]}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (flight.current || !state.engine) return;
          flight.current = true;
          setBusy(true);
          setError("");
          void dockerApi
            .prepareTransfer(
              store.sessionId,
              state.engine.token,
              kind,
              targetId,
              kind === "copyIn" || kind === "copyOut" ? path : "",
              conflict,
            )
            .then((value) => {
              if (value) {
                request.current = {
                  requestId: crypto.randomUUID(),
                  engineToken: value.engine.token,
                  targetId: value.targetId,
                  action: value.action,
                  previewId: value.id,
                };
                setPreview(value);
              }
            })
            .catch((e) => setError(errorOf(e).message))
            .finally(() => {
              flight.current = false;
              setBusy(false);
            });
        }}
      >
        <header className="dialog-head">
          <h2>{transferActionNames[kind]}</h2>
        </header>
        <p className="docker-confirm-object">{name}</p>
        {(kind === "copyIn" || kind === "copyOut") && (
          <label className="docker-form-field">
            {kind === "copyIn" ? "容器目标目录" : "容器内文件或目录"}
            <input
              autoFocus
              required
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="/app/data"
            />
          </label>
        )}
        {!upload && (
          <label className="docker-form-field">
            本机目标重名时
            <select
              value={conflict}
              onChange={(e) => setConflict(e.target.value)}
            >
              <option value="rename">保留两者，自动重命名</option>
              <option value="skip">跳过已有文件</option>
              <option value="overwrite">预览后替换目标文件</option>
            </select>
          </label>
        )}
        <p className="resource-help">
          {kind === "imageLoad"
            ? "选择本机镜像归档，送往远端 Docker 引擎加载。归档内同名 Tag 可能改变现有引用。"
            : kind === "copyIn"
              ? "选择一个本机普通文件，复制到容器中的已有目录，保留文件名。同名容器文件可能被覆盖。当前支持小于 8 GiB 的单文件，文件名不超过 100 字节。"
              : "选择本机保存位置，以 tar 归档导出。不会在本机自动解包。"}
        </p>
        <p className="resource-help">
          进度会显示在文件任务面板。关闭详情后任务继续；中止不撤回远端已经发生的写入。
        </p>
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
            {busy
              ? "正在选择并核对…"
              : upload
                ? "选择本机文件…"
                : "选择保存位置…"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
