import { useEffect, useRef, useState } from "react";
import { Download, Tag, Trash2, RefreshCw } from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import { dockerApi, type ImageDetail } from "../dockerApi";
import { containerStateNames } from "../dockerPresentation";
import { errorOf } from "../types";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
import { DockerTransferDialog } from "./DockerTransferDialog";
import { useDockerActions } from "./useDockerActions";

function ReferenceDialog({
  mode,
  initial = "",
  onSubmit,
  onClose,
}: {
  mode: "pull" | "tag";
  initial?: string;
  onSubmit: (reference: string) => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null),
    flight = useRef(false);
  const [reference, setReference] = useState(initial),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      className="login-dialog docker-confirm-dialog"
      aria-label={mode === "pull" ? "拉取镜像" : "添加 Tag"}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (flight.current) return;
          flight.current = true;
          setBusy(true);
          setError("");
          void onSubmit(reference.trim())
            .then(onClose)
            .catch((e) => setError(errorOf(e).message))
            .finally(() => {
              flight.current = false;
              setBusy(false);
            });
        }}
      >
        <header className="dialog-head">
          <h2>{mode === "pull" ? "拉取镜像" : "添加 Tag"}</h2>
        </header>
        <label className="docker-form-field">
          {mode === "pull" ? "镜像名称" : "目标引用"}
          <input
            autoFocus
            required
            maxLength={512}
            value={reference}
            onChange={(e) => setReference(e.target.value)}
            placeholder="registry.example.com/team/app:latest"
            spellCheck={false}
          />
        </label>
        <p className="resource-help">
          {mode === "pull"
            ? "在远端 Docker 引擎拉取，使用该服务器已有的仓库登录信息。任务会在后台继续。"
            : "为当前内容添加引用；已存在的 Tag 会在下一步显示替换范围。"}
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
          <button className="primary" disabled={busy || !reference.trim()}>
            {busy ? "正在提交…" : mode === "pull" ? "拉取" : "预览"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
export function ImagePullButton({ enabled }: { enabled: boolean }) {
  const { state } = useDockerManagement();
  const [open, setOpen] = useState(false),
    [importing, setImporting] = useState(false);
  const actions = useDockerActions(
    `${state.engine?.token}/pull/${enabled}`,
    enabled,
  );
  useEffect(() => {
    if (!enabled) {
      setOpen(false);
      setImporting(false);
    }
  }, [enabled]);
  return (
    <>
      <button
        className="resource-link"
        disabled={
          !enabled || actions.busy || actions.locked || actions.uncertain
        }
        onClick={() => setOpen(true)}
      >
        <Download size={13} /> 拉取镜像
      </button>
      <button
        className="resource-link"
        disabled={!enabled || actions.locked || actions.busy}
        onClick={() => setImporting(true)}
      >
        导入镜像…
      </button>
      {importing && (
        <DockerTransferDialog
          kind="imageLoad"
          targetId="image-import"
          name="远端 Docker 引擎"
          onClose={() => setImporting(false)}
        />
      )}
      {open && (
        <ReferenceDialog
          mode="pull"
          onClose={() => setOpen(false)}
          onSubmit={(reference) =>
            actions.act(reference, { type: "imagePull" }, false)
          }
        />
      )}
      {actions.error && (
        <p className="resource-error" role="alert">
          {actions.error}
        </p>
      )}
      {actions.uncertain && (
        <button
          disabled={actions.busy}
          onClick={() => void actions.retry().catch(() => {})}
        >
          核对提交结果
        </button>
      )}
    </>
  );
}
export function ImageManagement({
  imageId,
  name,
  enabled,
  active,
  onContainer,
  onCreate,
}: {
  imageId: string;
  name: string;
  enabled: boolean;
  active: boolean;
  onContainer: (id: string, name: string) => void;
  onCreate: (id: string, name: string) => void;
}) {
  const { store, state } = useDockerManagement();
  const [detail, setDetail] = useState<ImageDetail | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(false),
    [revision, setRevision] = useState(0),
    [tagging, setTagging] = useState(false);
  const [exporting, setExporting] = useState(false);
  const scope = `${state.engine?.token}/${imageId}/${active}`;
  const actions = useDockerActions(scope, enabled && active);
  const finished = state.operations
    .map((t) => `${t.id}/${t.finishedAt}`)
    .join(",");
  useEffect(() => {
    setTagging(false);
    setExporting(false);
  }, [scope]);
  useEffect(() => {
    if (!active || !enabled || !state.engine) return;
    let alive = true;
    setLoading(true);
    setError("");
    void dockerApi
      .image(store.contextId, state.engine.token, imageId)
      .then(
        (value) => {
          if (alive) setDetail(value);
        },
        (e) => {
          if (alive) setError(errorOf(e).message);
        },
      )
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [
    store,
    state.engine?.token,
    imageId,
    active,
    enabled,
    revision,
    finished,
  ]);
  const canAct =
    enabled &&
    active &&
    !!detail &&
    !loading &&
    !error &&
    !actions.busy &&
    !actions.locked &&
    !actions.uncertain;
  const reference =
    detail?.tags.includes(name) || detail?.digests.includes(name) ? name : null;
  return (
    <div className="docker-container-management">
      <div className="docker-management-toolbar">
        <button
          className="primary"
          disabled={!canAct}
          onClick={() => onCreate(imageId, name)}
        >
          创建容器…
        </button>
        <button disabled={!canAct} onClick={() => setTagging(true)}>
          <Tag size={14} />
          添加 Tag
        </button>
        <button
          disabled={!canAct}
          onClick={() =>
            void actions
              .act(imageId, { type: "imageRemove", reference })
              .catch(() => {})
          }
        >
          <Trash2 size={14} />
          {reference ? "移除引用" : "删除镜像"}
        </button>
      </div>
      {loading && (
        <p className="resource-help" role="status">
          正在核对镜像与容器引用…
        </p>
      )}
      {(error || actions.error) && (
        <p className="resource-error" role="alert">
          {error || actions.error}
        </p>
      )}
      {actions.uncertain && (
        <button
          disabled={actions.busy}
          onClick={() => void actions.retry().catch(() => {})}
        >
          核对提交结果
        </button>
      )}
      <dl className="docker-facts">
        <dt>镜像 ID</dt>
        <dd className="docker-id">{imageId}</dd>
        {detail && (
          <>
            <dt>平台</dt>
            <dd>{detail.platform}</dd>
            <dt>大小</dt>
            <dd>{(detail.size / 1024 / 1024).toFixed(1)} MiB</dd>
            <dt>创建时间</dt>
            <dd>{new Date(detail.created).toLocaleString()}</dd>
          </>
        )}
      </dl>
      {detail && (
        <>
          <details className="docker-detail-group" open>
            <summary>
              引用<span>{detail.tags.length} 个 Tag</span>
            </summary>
            {detail.tags.map((tag) => (
              <div
                className="docker-detail-item docker-reference-item"
                key={tag}
              >
                <code>{tag}</code>
                <button
                  className="icon-button"
                  aria-label={`移除引用 ${tag}`}
                  disabled={!canAct}
                  onClick={() =>
                    void actions
                      .act(imageId, { type: "imageRemove", reference: tag })
                      .catch(() => {})
                  }
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
            {detail.digests.map((digest) => (
              <div className="docker-detail-item" key={digest}>
                <code>{digest}</code>
              </div>
            ))}
            {!detail.tags.length && (
              <p className="resource-help">没有 Tag 引用</p>
            )}
          </details>
          <details className="docker-detail-group" open>
            <summary>
              使用此镜像的容器<span>{detail.containers.length}</span>
            </summary>
            {detail.containers.map((container) => (
              <button
                className="docker-reference-container"
                key={container.id}
                onClick={() => onContainer(container.id, container.name)}
              >
                <span>{container.name}</span>
                <small>
                  {containerStateNames[container.state] ?? container.state}
                </small>
              </button>
            ))}
            {!detail.containers.length && (
              <p className="resource-help">当前没有容器使用此内容。</p>
            )}
          </details>
        </>
      )}
      <div className="docker-detail-freshness">
        <span>包含已停止容器的使用关系</span>
        <button
          className="resource-link"
          disabled={!enabled || loading}
          onClick={() => setRevision((v) => v + 1)}
        >
          <RefreshCw size={12} />
          刷新
        </button>
      </div>
      <button
        className="resource-link"
        disabled={!canAct}
        onClick={() => setExporting(true)}
      >
        导出镜像（tar）…
      </button>
      {exporting && active && (
        <DockerTransferDialog
          kind="imageSave"
          targetId={imageId}
          name={name}
          onClose={() => setExporting(false)}
        />
      )}
      {tagging && (
        <ReferenceDialog
          mode="tag"
          onClose={() => setTagging(false)}
          onSubmit={(reference) =>
            actions.act(imageId, { type: "imageTag", reference })
          }
        />
      )}
      {actions.preview && (
        <DockerConfirmDialog
          preview={actions.preview}
          onRefresh={actions.renewPreview}
          uncertain={actions.uncertain}
          onRetry={actions.retry}
          onClose={actions.dismissPreview}
          onConfirm={() =>
            actions.submit(
              actions.preview!.targetId,
              actions.preview!.action,
              actions.preview,
            )
          }
        />
      )}
    </div>
  );
}
