import {
  useEffect,
  useId,
  useRef,
  useState,
  type InputHTMLAttributes,
} from "react";
import { Plus, X } from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import { dockerApi } from "../dockerApi";
import { DockerConfirmationView } from "./DockerConfirmationView";
import { DockerDialogFrame } from "./DockerDialogFrame";
import { RestartPolicyFields } from "./RestartPolicyFields";
import { useDockerActions } from "./useDockerActions";
import {
  createInitialDraft,
  draftEquals,
  validateCreateDraft,
  type CreateContainerDraft,
} from "./createContainerDraft";
export function CreateContainerDialog({
  imageId,
  imageName,
  onClose,
}: {
  imageId: string;
  imageName: string;
  onClose: () => void;
}) {
  const { store, state } = useDockerManagement(),
    dialog = useRef<HTMLDialogElement>(null),
    prefix = useId();
  const baseline = useRef(createInitialDraft()),
    source = useRef(state.engine?.token),
    editorFocus = useRef<string>('[data-field="name"]'),
    editorScroll = useRef(0);
  const [groups, setGroups] = useState<Record<string, boolean>>({
    ports: true,
    environment: false,
    mounts: false,
    more: true,
  });
  const groupProps = (key: string) => ({
    "data-group": key,
    open: groups[key],
    onToggle: (e: React.SyntheticEvent<HTMLDetailsElement>) => {
      const open = e.currentTarget.open;
      setGroups((v) => (v[key] === open ? v : { ...v, [key]: open }));
    },
  });
  const [draft, setDraft] = useState<CreateContainerDraft>(createInitialDraft),
    [touched, setTouched] = useState<Record<string, boolean>>({}),
    [submitted, setSubmitted] = useState(false),
    [discard, setDiscard] = useState(false);
  const actions = useDockerActions(
    store.contextId + "/create/" + imageId,
    !!state.engine,
    onClose,
  );
  const preview = actions.preview,
    dirty = !draftEquals(draft, baseline.current),
    validation = validateCreateDraft(draft, imageId),
    issues = validation.ok ? [] : validation.issues;
  const engineChanged = source.current !== state.engine?.token;
  useEffect(() => {
    if (engineChanged && !actions.uncertain && actions.phase !== "submitting")
      actions.dismissPreview();
  }, [state.engine?.token]);
  const update = (patch: Partial<CreateContainerDraft>) =>
    setDraft((v) => ({ ...v, ...patch }));
  const touch = (key: string) => setTouched((v) => ({ ...v, [key]: true }));
  const issue = (key: string) =>
    submitted || touched[key]
      ? issues.find((i) => i.field === key)?.message
      : undefined;
  const errorId = (key: string) => prefix + "-" + key;
  function fieldError(key: string) {
    return issue(key) ? (
      <small id={errorId(key)} className="field-error" role="status">
        {issue(key)}
      </small>
    ) : null;
  }
  function attrs(key: string) {
    return {
      "data-field": key,
      "aria-invalid": !!issue(key),
      "aria-describedby": issue(key) ? errorId(key) : undefined,
      onBlur: () => touch(key),
    };
  }
  function input(
    label: string,
    key: string,
    value: string,
    onChange: (value: string) => void,
    extra: InputHTMLAttributes<HTMLInputElement> = {},
  ) {
    return (
      <label className="docker-form-field">
        {label}
        <input
          aria-label={label}
          {...extra}
          {...attrs(key)}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        {fieldError(key)}
      </label>
    );
  }
  function restoreEditor() {
    setDiscard(false);
    requestAnimationFrame(() => {
      const body =
        dialog.current?.querySelector<HTMLElement>(".docker-flow-body");
      if (body) body.scrollTop = editorScroll.current;
      dialog.current
        ?.querySelector<HTMLElement>(editorFocus.current)
        ?.focus({ preventScroll: true });
    });
  }
  function back() {
    actions.dismissPreview();
    restoreEditor();
  }
  function close() {
    if (actions.uncertain || actions.phase === "submitting") return;
    const key = (document.activeElement as HTMLElement)?.dataset.field;
    editorFocus.current = key
      ? '[data-field="' + key + '"]'
      : "[data-preview-create]";
    actions.dismissPreview();
    if (dirty) setDiscard(true);
    else onClose();
  }
  async function prepare() {
    if (actions.busy || !state.engine) return;
    setSubmitted(true);
    if (!validation.ok) {
      const target = dialog.current?.querySelector<HTMLElement>(
        '[data-field="' + validation.issues[0].field + '"]',
      );
      const group = target?.closest("details")?.dataset.group;
      if (group) setGroups((v) => ({ ...v, [group]: true }));
      requestAnimationFrame(() => {
        target?.scrollIntoView({ block: "center" });
        target?.focus({ preventScroll: true });
      });
      return;
    }
    const key = (document.activeElement as HTMLElement)?.dataset.field;
    editorFocus.current = key
      ? '[data-field="' + key + '"]'
      : "[data-preview-create]";
    source.current = state.engine.token;
    await actions
      .prepareWith(() =>
        dockerApi.prepareCreate(
          store.contextId,
          state.engine!.token,
          validation.specification,
        ),
      )
      .catch(() => {});
  }
  const focusKey = discard ? "discard" : preview ? "preview" : "edit";
  return (
    <DockerDialogFrame
      dialogRef={dialog}
      title={discard ? "放弃创建设置？" : preview ? "创建容器？" : "创建容器"}
      wide
      onClose={discard ? restoreEditor : close}
      busy={actions.phase === "submitting" || actions.uncertain}
      focusKey={focusKey}
      onSubmit={!preview && !discard ? () => void prepare() : undefined}
    >
      {discard ? (
        <>
          <div className="docker-flow-body">
            <p>尚未保存的容器设置将被丢弃。</p>
          </div>
          <footer className="dialog-actions">
            <button type="button" data-initial-focus onClick={restoreEditor}>
              继续编辑
            </button>
            <button type="button" className="danger" onClick={onClose}>
              放弃修改
            </button>
          </footer>
        </>
      ) : preview ? (
        <DockerConfirmationView
          preview={preview}
          onBack={back}
          backLabel="返回编辑"
          onRefresh={actions.renewPreview}
          uncertain={actions.uncertain}
          onRetry={actions.retry}
          externalError={actions.error}
          disabled={engineChanged}
          onConfirm={() =>
            actions.submit(preview.targetId, preview.action, preview)
          }
        />
      ) : null}
      {!preview && !discard && (
        <div className="docker-editor-content">
          <div
            className="docker-flow-body"
            onScroll={(e) => {
              editorScroll.current = e.currentTarget.scrollTop;
            }}
            onBlur={(e) => {
              const key = (e.target as HTMLElement).dataset.field;
              if (key) touch(key);
            }}
          >
            <p className="docker-confirm-object">{imageName}</p>
            {engineChanged && (
              <p className="docker-notice" role="status">
                引擎已变化，设置已保留。请重新预检并核对目标引擎。
              </p>
            )}
            {input("容器名称", "name", draft.name, (name) => update({ name }), {
              "data-initial-focus": !preview && !discard ? true : undefined,
              maxLength: 255,
            } as InputHTMLAttributes<HTMLInputElement>)}
            <details className="docker-detail-group" {...groupProps("ports")}>
              <summary>
                发布端口<span>{draft.ports.length}</span>
              </summary>
              {draft.ports.map((p, i) => (
                <div className="docker-create-row" key={p.rowId}>
                  {input(
                    "宿主地址",
                    "ports." + p.rowId + ".hostIp",
                    p.hostIp,
                    (hostIp) =>
                      update({
                        ports: draft.ports.map((v) =>
                          v.rowId === p.rowId ? { ...v, hostIp } : v,
                        ),
                      }),
                    { "aria-label": "宿主地址 " + (i + 1) },
                  )}
                  {input(
                    "宿主端口",
                    "ports." + p.rowId + ".hostPort",
                    p.hostPort,
                    (hostPort) =>
                      update({
                        ports: draft.ports.map((v) =>
                          v.rowId === p.rowId ? { ...v, hostPort } : v,
                        ),
                      }),
                    {
                      type: "number",
                      min: 1,
                      max: 65535,
                      "aria-label": "宿主端口 " + (i + 1),
                    },
                  )}
                  {input(
                    "容器端口",
                    "ports." + p.rowId + ".containerPort",
                    p.containerPort,
                    (containerPort) =>
                      update({
                        ports: draft.ports.map((v) =>
                          v.rowId === p.rowId ? { ...v, containerPort } : v,
                        ),
                      }),
                    {
                      type: "number",
                      min: 1,
                      max: 65535,
                      "aria-label": "容器端口 " + (i + 1),
                    },
                  )}
                  <label className="docker-form-field">
                    协议
                    <select
                      aria-label={"协议 " + (i + 1)}
                      value={p.protocol}
                      onChange={(e) =>
                        update({
                          ports: draft.ports.map((v) =>
                            v.rowId === p.rowId
                              ? { ...v, protocol: e.target.value }
                              : v,
                          ),
                        })
                      }
                    >
                      <option value="tcp">TCP</option>
                      <option value="udp">UDP</option>
                    </select>
                  </label>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={"删除端口 " + (i + 1)}
                    onClick={() =>
                      update({
                        ports: draft.ports.filter((v) => v.rowId !== p.rowId),
                      })
                    }
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
              <button
                type="button"
                data-field="ports"
                className="resource-link"
                onClick={() =>
                  update({
                    ports: [
                      ...draft.ports,
                      {
                        rowId: crypto.randomUUID(),
                        hostIp: "127.0.0.1",
                        hostPort: "",
                        containerPort: "",
                        protocol: "tcp",
                      },
                    ],
                  })
                }
              >
                <Plus size={12} />
                添加端口
              </button>
              {fieldError("ports")}
            </details>
            <details
              className="docker-detail-group"
              {...groupProps("environment")}
            >
              <summary>
                环境变量<span>仅保留在当前草稿</span>
              </summary>
              <label className="docker-form-field">
                每行 KEY=value
                <textarea
                  aria-label="每行 KEY=value"
                  {...attrs("environmentText")}
                  className="form-textarea"
                  autoComplete="off"
                  spellCheck={false}
                  rows={4}
                  value={draft.environmentText}
                  onChange={(e) => update({ environmentText: e.target.value })}
                />
                {fieldError("environmentText")}
              </label>
            </details>
            <details className="docker-detail-group" {...groupProps("mounts")}>
              <summary>
                挂载<span>{draft.mounts.length}</span>
              </summary>
              {draft.mounts.map((m, i) => (
                <div
                  className="docker-create-row docker-mount-editor"
                  key={m.rowId}
                >
                  <label className="docker-form-field">
                    类型
                    <select
                      value={m.kind}
                      onChange={(e) =>
                        update({
                          mounts: draft.mounts.map((v) =>
                            v.rowId === m.rowId
                              ? { ...v, kind: e.target.value }
                              : v,
                          ),
                        })
                      }
                    >
                      <option value="bind">Bind</option>
                      <option value="volume">Volume</option>
                    </select>
                  </label>
                  {input(
                    "来源",
                    "mounts." + m.rowId + ".source",
                    m.source,
                    (source) =>
                      update({
                        mounts: draft.mounts.map((v) =>
                          v.rowId === m.rowId ? { ...v, source } : v,
                        ),
                      }),
                  )}
                  {input(
                    "容器路径",
                    "mounts." + m.rowId + ".destination",
                    m.destination,
                    (destination) =>
                      update({
                        mounts: draft.mounts.map((v) =>
                          v.rowId === m.rowId ? { ...v, destination } : v,
                        ),
                      }),
                  )}
                  <label className="docker-check-field">
                    <input
                      type="checkbox"
                      checked={m.readOnly}
                      onChange={(e) =>
                        update({
                          mounts: draft.mounts.map((v) =>
                            v.rowId === m.rowId
                              ? { ...v, readOnly: e.target.checked }
                              : v,
                          ),
                        })
                      }
                    />
                    只读
                  </label>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={"删除挂载 " + (i + 1)}
                    onClick={() =>
                      update({
                        mounts: draft.mounts.filter((v) => v.rowId !== m.rowId),
                      })
                    }
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
              <button
                type="button"
                data-field="mounts"
                className="resource-link"
                onClick={() =>
                  update({
                    mounts: [
                      ...draft.mounts,
                      {
                        rowId: crypto.randomUUID(),
                        kind: "bind",
                        source: "",
                        destination: "",
                        readOnly: false,
                      },
                    ],
                  })
                }
              >
                <Plus size={12} />
                添加挂载
              </button>
              {fieldError("mounts")}
            </details>
            <details className="docker-detail-group" {...groupProps("more")}>
              <summary>
                更多设置<span>网络、重启与命令</span>
              </summary>
              {input(
                "已有网络名称",
                "network",
                draft.network,
                (network) => update({ network }),
                { placeholder: "默认 bridge" },
              )}
              <RestartPolicyFields
                policy={draft.restartPolicy}
                count={draft.maximumRetryCount}
                onPolicy={(restartPolicy) => update({ restartPolicy })}
                onCount={(maximumRetryCount) => update({ maximumRetryCount })}
                error={issue("maximumRetryCount")}
                onBlur={() => touch("maximumRetryCount")}
              />
              {input(
                "工作目录",
                "workingDirectory",
                draft.workingDirectory,
                (workingDirectory) => update({ workingDirectory }),
              )}
              <label className="docker-form-field">
                命令参数
                <textarea
                  aria-label="命令参数"
                  {...attrs("commandText")}
                  className="form-textarea"
                  rows={3}
                  value={draft.commandText}
                  onChange={(e) => update({ commandText: e.target.value })}
                />
                <small>每行一个参数，保持原样传递。</small>
                {fieldError("commandText")}
              </label>
            </details>
            <label className="docker-check-field">
              <input
                type="checkbox"
                checked={draft.start}
                onChange={(e) => update({ start: e.target.checked })}
              />
              创建后启动
            </label>
            {actions.error && (
              <p className="resource-error" role="alert">
                {actions.error}
              </p>
            )}
          </div>
          <footer className="dialog-actions">
            <button type="button" onClick={close}>
              取消
            </button>
            <button
              type="submit"
              data-preview-create
              className="primary"
              disabled={actions.busy || !state.engine}
            >
              {actions.busy ? "正在预检…" : "预览创建"}
            </button>
          </footer>
        </div>
      )}
    </DockerDialogFrame>
  );
}
