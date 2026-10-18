import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  Layers,
  Plus,
  RefreshCw,
  MoreHorizontal,
  Play,
  Square,
  FileText,
} from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  type ComposeAssociation,
  type ComposeMode,
  type ComposeOverview,
  type ComposeProject,
} from "../dockerApi";
import { containerStateNames } from "../dockerPresentation";
import { api } from "../api";
import { errorOf } from "../types";
import { PortForwardMenu } from "../PortForwardMenu";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
import { DockerPathPicker } from "./DockerPathPicker";
import { useDockerActions } from "./useDockerActions";

function AssociationDialog({
  initial,
  onClose,
  onSaved,
}: {
  initial: ComposeAssociation;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { store, state } = useDockerManagement();
  const dialog = useRef<HTMLDialogElement>(null),
    flight = useRef(false);
  const [value, setValue] = useState(initial),
    [files, setFiles] = useState(initial.files.join("\n")),
    [env, setEnv] = useState(initial.envFiles.join("\n")),
    [profiles, setProfiles] = useState(initial.profiles.join(", "));
  const [picker, setPicker] = useState<"files" | "env" | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  const lines = (text: string) =>
    text
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
  return (
    <dialog
      ref={dialog}
      className="login-dialog docker-confirm-dialog docker-form-dialog"
      aria-label="关联 Compose 配置"
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
            .saveAssociation(store.sessionId, state.engine.token, {
              ...value,
              files: lines(files),
              envFiles: lines(env),
              profiles: profiles.split(/[,，\s]+/).filter(Boolean),
            })
            .then(() => {
              onSaved();
              onClose();
            })
            .catch((e) => setError(errorOf(e).message))
            .finally(() => {
              flight.current = false;
              setBusy(false);
            });
        }}
      >
        <header className="dialog-head">
          <h2>关联 Compose 配置</h2>
        </header>
        <p className="resource-help">
          保存远端路径和项目设置，之后仍需预览并确认应用配置。
        </p>
        <label className="docker-form-field">
          项目名称
          <input
            autoFocus
            required
            value={value.project}
            pattern="[a-z0-9][a-z0-9_-]*"
            onChange={(e) =>
              setValue((v) => ({ ...v, project: e.target.value }))
            }
          />
        </label>
        <label className="docker-form-field">
          远端工作目录
          <input
            required
            value={value.directory}
            placeholder="/opt/app"
            onChange={(e) =>
              setValue((v) => ({ ...v, directory: e.target.value }))
            }
          />
        </label>
        <label className="docker-form-field">
          Compose 文件
          <textarea
            required
            rows={3}
            value={files}
            placeholder="/opt/app/compose.yaml"
            onChange={(e) => setFiles(e.target.value)}
          />
          <small>每行一个绝对路径，按此顺序合并配置。</small>
        </label>
        <button
          type="button"
          className="resource-link"
          onClick={() => setPicker("files")}
        >
          从远端文件中选择…
        </button>
        <details className="docker-detail-group">
          <summary>
            更多设置<span>环境文件与 Profiles</span>
          </summary>
          <label className="docker-form-field">
            环境文件
            <textarea
              rows={2}
              value={env}
              onChange={(e) => setEnv(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="resource-link"
            onClick={() => setPicker("env")}
          >
            选择环境文件…
          </button>
          <label className="docker-form-field">
            Profiles
            <input
              value={profiles}
              onChange={(e) => setProfiles(e.target.value)}
              placeholder="多个名称以逗号分隔"
            />
          </label>
        </details>
        {picker && (
          <DockerPathPicker
            sessionId={store.sessionId}
            initialPath={value.directory}
            onClose={() => setPicker(null)}
            onPick={(path) => {
              if (picker === "files")
                setFiles((v) => [...lines(v), path].join("\n"));
              else setEnv((v) => [...lines(v), path].join("\n"));
              setPicker(null);
            }}
          />
        )}
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
            {busy ? "正在核对路径…" : "保存关联"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
export function ComposeManagement({
  active,
  enabled,
  onContainer,
}: {
  active: boolean;
  enabled: boolean;
  onContainer: (id: string, name: string) => void;
}) {
  const { store, state } = useDockerManagement();
  const [data, setData] = useState<ComposeOverview | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [revision, setRevision] = useState(0),
    [selected, setSelected] = useState(""),
    [editing, setEditing] = useState<ComposeAssociation | null>(null),
    [query, setQuery] = useState("");
  const scope = `${state.engine?.token}/${active}/${selected}`;
  const actions = useDockerActions(scope, active && enabled);
  const finished = state.operations
    .map((t) => `${t.id}/${t.finishedAt}`)
    .join(",");
  useEffect(() => {
    setEditing(null);
  }, [scope]);
  useEffect(() => {
    if (!enabled || !active || !state.engine) return;
    let alive = true;
    setBusy(true);
    setError("");
    void dockerApi
      .projects(store.sessionId, state.engine.token)
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
  }, [store, state.engine?.token, state.catalogRevision, active, enabled, revision, finished]);
  const names = [
    ...new Set([
      ...(data?.projects.map((p) => p.name) ?? []),
      ...(data?.associations
        .filter((a) => a.endpoint === state.engine?.endpoint)
        .map((a) => a.project) ?? []),
    ]),
  ].sort();
  const project = data?.projects.find((p) => p.name === selected);
  const association = data?.associations.find(
    (a) => a.project === selected && a.endpoint === state.engine?.endpoint,
  );
  const associated =
    association && association.daemonId === state.engine?.daemonId;
  const canAct =
    active &&
    enabled &&
    data?.available &&
    associated &&
    !busy &&
    !error &&
    !actions.busy &&
    !actions.locked &&
    !actions.uncertain;
  function edit(p?: ComposeProject, a?: ComposeAssociation) {
    setEditing(
      a ?? {
        id: "",
        revision: 0,
        serverId: "",
        endpoint: "",
        daemonId: null,
        project: p?.name ?? selected,
        directory: p?.directory ?? "",
        files: p?.files ?? [],
        envFiles: [],
        profiles: [],
      },
    );
  }
  function act(mode: ComposeMode) {
    if (!canAct || !association) return;
    void actions
      .act(selected, {
        type: "compose",
        mode,
        associationId: association.id,
        revision: association.revision,
      })
      .catch(() => {});
  }
  return (
    <section className="docker-secondary-page" aria-label="Compose 项目">
      <div className="docker-page-toolbar">
        {selected ? (
          <button className="resource-link" onClick={() => setSelected("")}>
            <ArrowLeft size={14} />
            项目
          </button>
        ) : (
          <strong>Compose 项目</strong>
        )}
        <span />
        <button
          className="icon-button"
          aria-label="刷新项目"
          disabled={busy || !enabled}
          onClick={() => setRevision((v) => v + 1)}
        >
          <RefreshCw size={14} />
        </button>
        {!selected && (
          <button
            className="resource-link"
            disabled={!enabled}
            onClick={() => edit()}
          >
            <Plus size={13} />
            关联配置
          </button>
        )}
      </div>
      {busy && (
        <p className="resource-help" role="status">
          正在读取项目…
        </p>
      )}
      {(error || actions.error) && (
        <p className="resource-error" role="alert">
          {error || actions.error}
        </p>
      )}
      {data?.message && <p className="docker-notice">{data.message}</p>}
      {actions.uncertain && (
        <button
          disabled={actions.busy}
          onClick={() => void actions.retry().catch(() => {})}
        >
          核对提交结果
        </button>
      )}
      {selected ? (
        <>
          <div className="docker-page-title">
            <Layers size={18} />
            <h3>{selected}</h3>
          </div>
          <div className="docker-management-toolbar">
            <button disabled={!canAct} onClick={() => act("start")}>
              <Play size={13} />
              启动
            </button>
            <button disabled={!canAct} onClick={() => act("stop")}>
              <Square size={13} />
              停止
            </button>
            <button disabled={!canAct} onClick={() => act("restart")}>
              <RefreshCw size={13} />
              重启
            </button>
            <PortForwardMenu
              label="项目更多操作"
              disabled={!enabled || actions.busy}
              items={[
                {
                  label: "编辑配置关联…",
                  run: () => edit(project, association),
                },
                ...(canAct
                  ? [
                      {
                        label: "停止并移除项目…",
                        danger: true,
                        run: () => act("down"),
                      },
                    ]
                  : []),
                ...(association && !actions.locked
                  ? [
                      {
                        label: "解除本地关联",
                        run: () => {
                          void dockerApi
                            .removeAssociation(
                              store.sessionId,
                              association.id,
                              association.revision,
                            )
                            .then(() => setRevision((v) => v + 1))
                            .catch((e) => setError(errorOf(e).message));
                        },
                      },
                    ]
                  : []),
              ]}
            >
              <MoreHorizontal size={15} />
            </PortForwardMenu>
          </div>
          {!associated && (
            <p className="docker-notice">
              {association
                ? "引擎身份已变化，请重新核对并保存关联。"
                : "先关联配置文件，再管理此项目。"}
              <button
                className="resource-link"
                onClick={() => edit(project, association)}
              >
                关联配置
              </button>
            </p>
          )}
          {association && (
            <>
              <dl className="docker-facts">
                <dt>工作目录</dt>
                <dd>{association.directory}</dd>
                <dt>配置文件</dt>
                <dd>
                  {association.files.map((f) => (
                    <div key={f}>{f}</div>
                  ))}
                </dd>
                {association.profiles.length > 0 && (
                  <>
                    <dt>Profiles</dt>
                    <dd>{association.profiles.join(", ")}</dd>
                  </>
                )}
              </dl>
              <div className="docker-management-toolbar">
                <button disabled={!canAct} onClick={() => act("pull")}>
                  更新镜像…
                </button>
                <button
                  className="primary"
                  disabled={!canAct}
                  onClick={() => act("apply")}
                >
                  应用配置…
                </button>
              </div>
              <p className="resource-help">
                拉取完成后重新预览并应用配置。重启不会应用配置变更。解除关联仅删除本地设置。
              </p>
            </>
          )}
          <details className="docker-detail-group" open>
            <summary>
              服务与副本<span>{project?.containers.length ?? 0} 个容器</span>
            </summary>
            {project?.containers.map((c) => (
              <div className="docker-service-row" key={c.id}>
                <button
                  className="resource-link"
                  onClick={() => onContainer(c.id, c.name)}
                >
                  <strong>{c.service || c.name}</strong>
                  <small>
                    {c.name} · {containerStateNames[c.state] ?? c.state}
                  </small>
                </button>
                <button
                  className="icon-button"
                  aria-label={`查看 ${c.name} 日志`}
                  disabled={!enabled}
                  onClick={() =>
                    void api
                      .openContainerLogs(
                        store.sessionId,
                        {
                          id: c.id,
                          name: c.name,
                          state: c.state,
                          status: c.state,
                          image: "",
                        },
                        state.engine!.endpoint,
                      )
                      .catch((e) => setError(errorOf(e).message))
                  }
                >
                  <FileText size={14} />
                </button>
              </div>
            ))}
            {!project?.containers.length && (
              <p className="resource-help">
                当前没有项目容器。应用配置后会重新发现。
              </p>
            )}
          </details>
        </>
      ) : (
        <>
          <label className="docker-form-field">
            <input
              aria-label="搜索项目"
              placeholder="搜索项目"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          {names
            .filter((name) => name.toLowerCase().includes(query.toLowerCase()))
            .map((name) => {
              const p = data?.projects.find((p) => p.name === name);
              return (
                <button
                  className="docker-project-row"
                  key={name}
                  onClick={() => setSelected(name)}
                >
                  <Layers size={16} />
                  <span>
                    <strong>{name}</strong>
                    <small>
                      {p?.containers.length ?? 0} 个容器 ·{" "}
                      {data?.associations.some(
                        (a) =>
                          a.project === name &&
                          a.endpoint === state.engine?.endpoint,
                      )
                        ? "已关联配置"
                        : "待关联配置"}
                    </small>
                  </span>
                </button>
              );
            })}
          {!names.length && !busy && (
            <p className="resource-help">
              尚未发现项目。可关联远端 Compose 文件，再预览并应用配置。
            </p>
          )}
        </>
      )}
      {editing && (
        <AssociationDialog
          initial={editing}
          onClose={() => setEditing(null)}
          onSaved={() => setRevision((v) => v + 1)}
        />
      )}
      {actions.preview && (
        <DockerConfirmDialog
          preview={actions.preview}
          onClose={() => actions.setPreview(null)}
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
