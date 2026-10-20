import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  Database,
  Network,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  type DockerResource,
  type DockerResources,
  type DockerBatchEntry,
} from "../dockerApi";
import { errorOf } from "../types";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
import { useDockerActions } from "./useDockerActions";
function InputDialog({
  title,
  label,
  onSubmit,
  onClose,
}: {
  title: string;
  label: string;
  onSubmit: (value: string) => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null),
    flight = useRef(false);
  const [value, setValue] = useState(""),
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
      aria-label={title}
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
          void onSubmit(value.trim())
            .then(onClose)
            .catch((e) => setError(errorOf(e).message))
            .finally(() => {
              flight.current = false;
              setBusy(false);
            });
        }}
      >
        <header className="dialog-head">
          <h2>{title}</h2>
        </header>
        <label className="docker-form-field">
          {label}
          <input
            autoFocus
            required
            value={value}
            onChange={(e) => setValue(e.target.value)}
            maxLength={255}
          />
        </label>
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
            {busy ? "正在核对…" : "预览"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
export function ResourceManagement({
  active,
  enabled,
  onBack,
  onContainer,
}: {
  active: boolean;
  enabled: boolean;
  onBack: () => void;
  onContainer: (id: string, name: string) => void;
}) {
  const { store, state } = useDockerManagement();
  const [data, setData] = useState<DockerResources | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [revision, setRevision] = useState(0),
    [tab, setTab] = useState<"usage" | "volumes" | "networks" | "cleanup">(
      "usage",
    ),
    [selected, setSelected] = useState<string | null>(null),
    [form, setForm] = useState<
      "volumeCreate" | "networkCreate" | "networkConnect" | null
    >(null),
    [choices, setChoices] = useState(new Set<string>()),
    [showVolumes, setShowVolumes] = useState(false);
  const scope = `${state.engine?.token}/${active}`;
  const actions = useDockerActions(scope, active && enabled);
  const finished = state.operations
    .map((t) => `${t.id}/${t.finishedAt}`)
    .join(",");
  useEffect(() => {
    setForm(null);
    setChoices(new Set());
    setSelected(null);
  }, [scope]);
  useEffect(() => {
    if (!active || !enabled || !state.engine) return;
    let alive = true;
    setBusy(true);
    setError("");
    void dockerApi
      .resources(store.sessionId, state.engine.token)
      .then(
        (d) => {
          if (alive) setData(d);
        },
        (e) => {
          if (alive) setError(errorOf(e).message);
        },
      )
      .finally(() => {
        if (alive) setBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [store, state.engine?.token, active, enabled, revision, finished]);
  const canAct =
    enabled &&
    !busy &&
    !error &&
    !actions.busy &&
    !actions.locked &&
    !actions.uncertain;
  const resources = tab === "volumes" ? data?.volumes : data?.networks;
  const current = resources?.find((r) => r.id === selected);
  const visibleCandidates =
    data?.candidates.filter((c) => showVolumes || c.kind !== "volume") ?? [];
  function toggle(id: string) {
    setChoices((v) => {
      const next = new Set(v);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }
  function clean() {
    const entries: DockerBatchEntry[] = visibleCandidates
      .filter((c) => choices.has(`${c.kind}/${c.id}`))
      .map((c) => ({
        targetId: c.id,
        action:
          c.kind === "container"
            ? "removeContainer"
            : c.kind === "image"
              ? "removeImage"
              : c.kind === "volume"
                ? "removeVolume"
                : "removeNetwork",
      }));
    void actions.act("selection", { type: "batch", entries }).catch(() => {});
  }
  function resource(r: DockerResource) {
    return (
      <>
        <div className="docker-page-toolbar">
          <button className="resource-link" onClick={() => setSelected(null)}>
            <ArrowLeft size={13} />
            返回
          </button>
          <span />
          <button
            className="resource-link"
            disabled={!canAct || r.protected || r.containers.length > 0}
            onClick={() =>
              void actions
                .act(r.id, {
                  type: r.kind === "volume" ? "volumeRemove" : "networkRemove",
                })
                .catch(() => {})
            }
          >
            <Trash2 size={13} />
            删除…
          </button>
        </div>
        <h3 className="docker-resource-name">{r.name}</h3>
        <dl className="docker-facts">
          <dt>驱动</dt>
          <dd>{r.driver}</dd>
          <dt>创建时间</dt>
          <dd>{r.created ? new Date(r.created).toLocaleString() : "未知"}</dd>
          {r.mountpoint && (
            <>
              <dt>宿主挂载点</dt>
              <dd>{r.mountpoint}</dd>
            </>
          )}
          <dt>ID</dt>
          <dd className="docker-id">{r.id}</dd>
        </dl>
        {r.protected && (
          <p className="resource-help">内置或 Ingress 网络保留，不提供删除。</p>
        )}
        <details className="docker-detail-group" open>
          <summary>
            使用此资源的容器<span>{r.containers.length}</span>
          </summary>
          {r.containers.map((c) => (
            <div className="docker-service-row" key={c.id}>
              <button
                className="resource-link"
                onClick={() => onContainer(c.id, c.name)}
              >
                {c.name}
              </button>
              {r.kind === "network" && !r.protected && (
                <button
                  className="resource-link"
                  disabled={!canAct}
                  onClick={() =>
                    void actions
                      .act(r.id, {
                        type: "networkDisconnect",
                        containerId: c.id,
                      })
                      .catch(() => {})
                  }
                >
                  断开…
                </button>
              )}
            </div>
          ))}
          {!r.containers.length && (
            <p className="resource-help">当前没有容器引用。</p>
          )}
        </details>
        {r.kind === "network" && !r.protected && (
          <button disabled={!canAct} onClick={() => setForm("networkConnect")}>
            连接容器…
          </button>
        )}
        <details className="docker-detail-group">
          <summary>配置信息</summary>
          <pre className="docker-raw-config" tabIndex={0}>
            {JSON.stringify(r.configuration, null, 2)}
          </pre>
        </details>
      </>
    );
  }
  return (
    <section className="docker-secondary-page" aria-label="空间与资源">
      <div className="docker-page-toolbar">
        <button className="resource-link" onClick={onBack}>
          <ArrowLeft size={14} />
          Docker
        </button>
        <strong>空间与资源</strong>
        <span />
        <button
          className="icon-button"
          aria-label="刷新空间与资源"
          disabled={busy || !enabled}
          onClick={() => setRevision((v) => v + 1)}
        >
          <RefreshCw size={14} />
        </button>
      </div>
      <div className="docker-segments" role="group" aria-label="资源类型">
        {(["usage", "volumes", "networks", "cleanup"] as const).map((t) => (
          <button
            key={t}
            aria-pressed={tab === t}
            onClick={() => {
              setTab(t);
              setSelected(null);
            }}
          >
            {
              {
                usage: "用量",
                volumes: "卷",
                networks: "网络",
                cleanup: "清理",
              }[t]
            }
          </button>
        ))}
      </div>
      {busy && (
        <p className="resource-help" role="status">
          正在读取用量和资源引用…
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
      {tab === "usage" && (
        <>
          <div className="docker-usage-list">
            {data?.usage.map((u, i) => (
              <div className="docker-usage-row" key={i}>
                <strong>
                  {(
                    {
                      Images: "镜像",
                      Containers: "容器",
                      "Local Volumes": "数据卷",
                      "Build Cache": "构建缓存",
                    } as Record<string, string>
                  )[u.Type] ?? u.Type}
                </strong>
                <span>{u.Size}</span>
                <small>
                  {u.TotalCount} 项 · {u.Active} 活动 · 可回收 {u.Reclaimable}
                </small>
              </div>
            ))}
          </div>
          <p className="resource-help">
            用量来自 Docker 引擎。镜像层可能共享，逐项大小不代表实际释放量。
          </p>
          <button onClick={() => setTab("cleanup")}>选择要清理的资源…</button>
        </>
      )}
      {(tab === "volumes" || tab === "networks") &&
        (current ? (
          resource(current)
        ) : (
          <>
            <div className="docker-page-toolbar">
              <span />
              <button
                className="resource-link"
                disabled={!canAct}
                onClick={() =>
                  setForm(tab === "volumes" ? "volumeCreate" : "networkCreate")
                }
              >
                <Plus size={13} />
                创建{tab === "volumes" ? "数据卷" : "网络"}
              </button>
            </div>
            {resources?.map((r) => (
              <button
                key={r.id}
                className="docker-project-row"
                onClick={() => setSelected(r.id)}
              >
                {r.kind === "volume" ? (
                  <Database size={16} />
                ) : (
                  <Network size={16} />
                )}
                <span>
                  <strong>{r.name}</strong>
                  <small>
                    {r.driver} · {r.containers.length} 个容器引用
                    {r.protected ? " · 内置" : ""}
                  </small>
                </span>
              </button>
            ))}
            {!resources?.length && !busy && (
              <p className="resource-help">没有可显示的资源。</p>
            )}
          </>
        ))}
      {tab === "cleanup" && (
        <>
          <p className="resource-help">
            勾选本次要删除的对象，预览时再次核对。不会自动增加新对象。
          </p>
          <label className="docker-check-field">
            <input
              type="checkbox"
              checked={showVolumes}
              onChange={(e) => {
                setShowVolumes(e.target.checked);
                if (!e.target.checked)
                  setChoices(
                    (v) =>
                      new Set([...v].filter((k) => !k.startsWith("volume/"))),
                  );
              }}
            />
            显示未被引用的数据卷（删除将丢失数据）
          </label>
          <div className="docker-cleanup-list">
            {visibleCandidates.map((c) => (
              <label key={`${c.kind}/${c.id}`}>
                <input
                  type="checkbox"
                  checked={choices.has(`${c.kind}/${c.id}`)}
                  onChange={() => toggle(`${c.kind}/${c.id}`)}
                />
                <span>
                  <strong>{c.name}</strong>
                  <small>
                    {
                      {
                        container: "已停止容器",
                        image: "悬空镜像",
                        volume: "数据卷",
                        network: "未被引用网络",
                      }[c.kind]
                    }
                  </small>
                </span>
              </label>
            ))}
          </div>
          <button
            className="danger"
            disabled={!canAct || !choices.size}
            onClick={clean}
          >
            预览删除所选 {choices.size} 项…
          </button>
          <details className="docker-detail-group">
            <summary>
              构建缓存<span>按引擎规则清理</span>
            </summary>
            <p className="resource-help">
              悬空构建缓存由引擎在执行时选择，不能精确列出候选，释放量未知。
            </p>
            <button
              disabled={!canAct}
              onClick={() =>
                void actions
                  .act("builder-cache", { type: "cachePrune" })
                  .catch(() => {})
              }
            >
              预览清理悬空缓存…
            </button>
          </details>
        </>
      )}
      {form && (
        <InputDialog
          title={
            form === "volumeCreate"
              ? "创建数据卷"
              : form === "networkCreate"
                ? "创建网络"
                : "连接容器"
          }
          label={form === "networkConnect" ? "完整容器 ID" : "名称"}
          onClose={() => setForm(null)}
          onSubmit={(value) =>
            actions.act(
              form === "networkConnect" ? current!.id : value,
              form === "networkConnect"
                ? { type: form, containerId: value }
                : { type: form },
            )
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
    </section>
  );
}
