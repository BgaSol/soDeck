import { useState } from "react";
import {
  dockerApi,
  transferActionNames,
  type DockerTransferKind,
} from "../dockerApi";
import { useDockerManagement } from "../DockerManagementProvider";
import { DockerDialogFrame } from "./DockerDialogFrame";
import { DockerConfirmationView } from "./DockerConfirmationView";
import { useDockerActions } from "./useDockerActions";
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
  const [path, setPath] = useState("/tmp"),
    [conflict, setConflict] = useState("rename");
  const actions = useDockerActions(
    store.contextId + "/" + state.engine?.token + "/" + kind + "/" + targetId,
    !!state.engine,
    onClose,
  );
  const { preview, busy } = actions,
    upload = kind === "imageLoad" || kind === "copyIn";
  function prepare() {
    if (!state.engine) return;
    void actions
      .prepareWith(() =>
        dockerApi.prepareTransfer(
          store.contextId,
          state.engine!.token,
          kind,
          targetId,
          kind === "copyIn" || kind === "copyOut" ? path : "",
          conflict,
        ),
      )
      .catch(() => {});
  }
  return (
    <DockerDialogFrame
      title={transferActionNames[kind]}
      onClose={onClose}
      busy={actions.phase === "submitting" || actions.uncertain}
      focusKey={preview ? "preview" : "edit"}
      onSubmit={!preview ? prepare : undefined}
    >
      {preview ? (
        <DockerConfirmationView
          preview={preview}
          onConfirm={() =>
            actions.submit(preview.targetId, preview.action, preview)
          }
          onRefresh={actions.renewPreview}
          onBack={() => actions.dismissPreview()}
          backLabel="返回编辑"
          uncertain={actions.uncertain}
          onRetry={actions.retry}
          externalError={actions.error}
        />
      ) : (
        <>
          <div className="docker-flow-body">
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

            {actions.error && (
              <p className="resource-error" role="alert">
                {actions.error}
              </p>
            )}
          </div>{" "}
          <footer className="dialog-actions">
            <button
              type="button"
              disabled={actions.uncertain}
              onClick={onClose}
            >
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
        </>
      )}
    </DockerDialogFrame>
  );
}
