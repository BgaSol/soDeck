import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Copy,
  FolderOpen,
  ArrowUpRight,
  MoreHorizontal,
  Play,
  RefreshCw,
  Square,
} from "lucide-react";
import { forwardApi } from "../portForwardApi";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  dockerActionNames,
  healthName,
  operationBlocks,
  isContainerAction,
  type ContainerAction,
  type DockerPreview,
  type DockerOperationRequest,
  type DockerActionPayload,
} from "../dockerApi";
import { containerStateNames } from "../dockerPresentation";
import { errorOf } from "../types";
import { PortForwardMenu } from "../PortForwardMenu";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
import { ContainerSettingsDialog } from "./ContainerSettingsDialog";
import { DockerTransferDialog } from "./DockerTransferDialog";
import { ContainerDiagnostics } from "./ContainerDiagnostics";

function date(value: string) {
  if (!value || value.startsWith("0001-")) return "—";
  const d = new Date(value);
  return Number.isFinite(+d) ? d.toLocaleString() : value;
}
export function ContainerManagement({
  containerId,
  serverId,
  onRevealPath,
  groups,
  onGroupsChange,
  name,
  engineGeneration,
  endpoint,
  enabled,
  active,
  fallback,
  children,
  requestedAction,
  onActionConsumed,
}: {
  containerId: string;
  serverId?: string;
  onRevealPath?: (path: string) => void;
  groups?: { ports: boolean; storage: boolean };
  onGroupsChange?: (value: { ports: boolean; storage: boolean }) => void;
  name: string;
  engineGeneration: number;
  endpoint: string;
  enabled: boolean;
  active: boolean;
  fallback: ReactNode;
  children: ReactNode;
  requestedAction?: ContainerAction | null;
  onActionConsumed?: () => void;
}) {
  const { store, state } = useDockerManagement();
  const engine =
    state.engine?.engineGeneration === engineGeneration &&
    state.engine.endpoint === endpoint
      ? state.engine
      : null;
  const detailState =
    engine && state.detail?.containerId === containerId ? state.detail : null;
  const detail = detailState?.value;
  const tasks = state.operations.filter(
    (task) =>
      task.engine.endpoint === endpoint && task.targetId === containerId,
  );
  const locked =
    tasks.some(operationBlocks) ||
    state.operations.some(
      (t) =>
        t.engine.endpoint === endpoint &&
        operationBlocks(t) &&
        !isContainerAction(t.action),
    );
  const [transfer, setTransfer] = useState<"copyIn" | "copyOut" | null>(null);
  const [submitting, setSubmitting] = useState(false),
    [pendingId, setPendingId] = useState<string | null>(null);
  const [linkBusy, setLinkBusy] = useState(false);
  const [error, setError] = useState(""),
    [copied, setCopied] = useState(false);
  const [preview, setPreview] = useState<DockerPreview | null>(null);
  const [settings, setSettings] = useState<"rename" | "restartPolicy" | null>(
    null,
  );
  const request = useRef<DockerOperationRequest | null>(null),
    inFlight = useRef(false);
  const scope = `${engine?.token}/${containerId}/${active}`;
  const currentScope = useRef(scope);
  currentScope.current = scope;
  useEffect(() => {
    setPreview(null);
    setSettings(null);
    setTransfer(null);
    setError("");
    setPendingId(null);
    request.current = null;
  }, [scope]);
  useEffect(() => {
    if (pendingId && tasks.some((task) => task.id === pendingId))
      setPendingId(null);
    if (
      request.current &&
      tasks.some((task) => task.requestId === request.current!.requestId)
    ) {
      request.current = null;
      setError("");
    }
  }, [tasks, pendingId]);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(timer);
  }, [copied]);
  const canManage =
    enabled &&
    !!engine &&
    !!detail &&
    (!detailState?.stale ||
      !detailState.error ||
      detailState.errorKind === "timeout") &&
    !detailState?.missing &&
    !locked &&
    !submitting &&
    !pendingId &&
    !request.current;
  async function submit(
    action: DockerActionPayload,
    confirmation: DockerPreview | null = null,
  ) {
    if (!engine) return;
    const targetScope = scope;
    request.current ??= {
      requestId: crypto.randomUUID(),
      engineToken: engine.token,
      targetId: containerId,
      action,
      previewId: confirmation?.id ?? null,
    };
    let id: string;
    try {
      id = await dockerApi.submit(store.sessionId, request.current);
    } catch (e) {
      const failure = errorOf(e);
      if (
        failure.kind === "confirmationRequired" ||
        failure.kind === "conflict"
      )
        void store.refresh().catch(() => {});
      if (
        [
          "conflict",
          "validation",
          "busy",
          "permission",
          "limit",
          "confirmationRequired",
          "disconnected",
          "unavailable",
          "cancelled",
        ].includes(failure.kind)
      )
        request.current = null;
      if (currentScope.current === targetScope) setError(failure.message);
      throw e;
    }
    if (currentScope.current === targetScope) {
      setPendingId(id);
      request.current = null;
    }
  }
  async function act(action: ContainerAction) {
    if (!canManage || inFlight.current || !engine || !detail) return;
    if (action === "rename" || action === "restartPolicy") {
      setSettings(action);
      return;
    }
    inFlight.current = true;
    setSubmitting(true);
    setError("");
    const targetScope = scope;
    // A new user intent receives its own id; retries after an IPC transport error
    // keep the previous id in the explicit retry control below.
    request.current = null;
    try {
      if (
        action === "remove" ||
        action === "kill" ||
        (detail.autoRemove && (action === "stop" || action === "restart"))
      ) {
        const result = await dockerApi.prepare(
          store.sessionId,
          engine.token,
          containerId,
          { type: action },
        );
        if (currentScope.current === targetScope) setPreview(result);
      } else await submit({ type: action });
    } catch (e) {
      if (currentScope.current === targetScope) setError(errorOf(e).message);
    } finally {
      inFlight.current = false;
      if (currentScope.current === targetScope) setSubmitting(false);
    }
  }
  const runtime = detail?.state;
  async function hostLink(request: Parameters<typeof dockerApi.hostLink>[3]) {
    if (!engine || linkBusy) return;
    const origin = scope;
    setLinkBusy(true);
    setError("");
    try {
      const link = await dockerApi.hostLink(
        store.sessionId,
        engine.token,
        containerId,
        request,
      );
      if (currentScope.current !== origin) return;
      if (link.path) onRevealPath?.(link.path);
      else if (serverId && link.host && link.port)
        await forwardApi.open(serverId, {
          name: link.name,
          targetHost: link.host,
          targetPort: link.port,
        });
    } catch (e) {
      if (currentScope.current === origin) setError(errorOf(e).message);
    } finally {
      setLinkBusy(false);
    }
  }
  useEffect(() => {
    if (!requestedAction) return;
    if (
      !enabled ||
      error ||
      (detailState?.error && detailState.errorKind !== "timeout")
    ) {
      onActionConsumed?.();
      return;
    }
    if (canManage) {
      onActionConsumed?.();
      void act(requestedAction);
    }
  }, [
    requestedAction,
    canManage,
    enabled,
    error,
    detailState?.error,
    detailState?.errorKind,
  ]);
  const actions: ContainerAction[] =
    runtime === "running"
      ? ["stop", "restart"]
      : runtime === "restarting"
        ? ["stop"]
        : runtime === "created" || runtime === "exited"
          ? ["start"]
          : runtime === "paused"
            ? ["unpause"]
            : [];
  return (
    <div className="docker-container-management">
      {detail ? (
        <div className="docker-runtime">
          <span className={`docker-dot state-${detail.state}`} />
          <span>{containerStateNames[detail.state] ?? detail.state}</span>
          <span
            className={`docker-health-label health-${detail.health ?? "none"}`}
          >
            {healthName(detail.health)}
          </span>
        </div>
      ) : (
        fallback
      )}
      <div className="docker-management-toolbar">
        {actions.map((action) => (
          <button
            key={action}
            className={action === "start" ? "primary" : ""}
            disabled={!canManage}
            onClick={() => void act(action)}
            aria-label={`${dockerActionNames[action]} ${name}`}
          >
            {action === "start" ? (
              <Play size={14} />
            ) : action === "stop" ? (
              <Square size={13} />
            ) : (
              <RefreshCw size={14} />
            )}
            {dockerActionNames[action]}
          </button>
        ))}
        {children}
        {detail && (
          <PortForwardMenu
            label={`${name} 更多操作`}
            disabled={!canManage}
            items={[
              {
                label: "导出容器文件（tar）…",
                run: () => setTransfer("copyOut"),
              },
              {
                label: "复制本机文件到容器…",
                run: () => setTransfer("copyIn"),
              },
              ...(detail.state === "running"
                ? [{ label: "暂停", run: () => void act("pause") }]
                : []),
              ...(!["dead", "removing"].includes(detail.state)
                ? [{ label: "重命名…", run: () => setSettings("rename") }]
                : []),
              ...(!detail.autoRemove &&
              !["dead", "removing"].includes(detail.state)
                ? [
                    {
                      label: "重启策略…",
                      run: () => setSettings("restartPolicy"),
                    },
                  ]
                : []),
              ...(["running", "restarting", "paused"].includes(detail.state)
                ? [
                    {
                      label: "强制停止…",
                      danger: true,
                      run: () => void act("kill"),
                    },
                  ]
                : []),
              ...(["created", "exited", "dead"].includes(detail.state)
                ? [
                    {
                      label: "删除容器…",
                      danger: true,
                      run: () => void act("remove"),
                    },
                  ]
                : []),
            ]}
          >
            <MoreHorizontal size={16} />
          </PortForwardMenu>
        )}
      </div>
      {(submitting || pendingId) && (
        <p className="resource-help" role="status">
          {preview ? "等待确认" : "正在确认容器状态…"}
        </p>
      )}
      {error && (
        <p className="resource-error" role="alert">
          {error}
        </p>
      )}
      {error && request.current && (
        <button
          disabled={submitting}
          onClick={() => {
            const action = request.current!.action;
            setSubmitting(true);
            void submit(action)
              .catch(() => {})
              .finally(() => setSubmitting(false));
          }}
        >
          核对提交结果
        </button>
      )}
      {state.error && (
        <p className="resource-help" role="status">
          {state.error}
        </p>
      )}
      {detailState?.error && (
        <p className="docker-notice" role="status">
          {detailState.error}
          {detail && (
            <>
              {" "}
              · 保留上次详情（{new Date(detail.updatedAt).toLocaleTimeString()}
              ）
            </>
          )}
          {enabled &&
            detailState.errorKind === "timeout" &&
            "。正在自动重试，操作前会重新确认容器状态。"}{" "}
          <button
            className="resource-link"
            disabled={detailState.loading}
            onClick={() =>
              void store.refresh().catch((e) => setError(errorOf(e).message))
            }
          >
            立即重试
          </button>
        </p>
      )}
      {!detail && !detailState?.error && engine && (
        <p className="resource-help" role="status">
          正在读取容器详情…
        </p>
      )}
      {detail && (
        <>
          {detail.stats && (
            <div className="docker-stats" aria-label="容器资源">
              <div>
                <span>CPU</span>
                <strong>
                  {detail.stats.cpuPercent == null
                    ? "—"
                    : `${detail.stats.cpuPercent.toFixed(1)}%`}
                </strong>
              </div>
              <div>
                <span>内存</span>
                <strong>{detail.stats.memory}</strong>
              </div>
              <div>
                <span>网络累计 接收 / 发送</span>
                <strong>{detail.stats.networkIo}</strong>
              </div>
              <div>
                <span>磁盘累计 读 / 写</span>
                <strong>{detail.stats.blockIo}</strong>
              </div>
            </div>
          )}
          {detail.statsLoading && (
            <p className="resource-help">资源统计更新中…</p>
          )}
          {!detail.statsLoading && detail.statsError && (
            <p className="resource-help">
              资源采样暂不可用{detail.stats ? "，保留上次数据" : ""}：
              {detail.statsError}
            </p>
          )}
          {(detail.oomKilled ||
            detail.error ||
            (detail.exitCode !== null &&
              detail.exitCode !== 0 &&
              ["exited", "dead"].includes(detail.state))) && (
            <div className="docker-diagnostic">
              <strong>
                {detail.oomKilled ? "容器因内存不足被终止" : "容器退出异常"}
              </strong>
              <span>
                退出码 {detail.exitCode ?? "未知"}
                {detail.error && ` · ${detail.error}`}
              </span>
            </div>
          )}
          <dl className="docker-facts docker-overview">
            <dt>容器 ID</dt>
            <dd className="docker-id-with-copy">
              <span className="docker-id" title={detail.id}>
                {detail.id}
              </span>
              <button
                className="icon-button"
                aria-label={copied ? "容器 ID 已复制" : "复制容器 ID"}
                onClick={() => {
                  void navigator.clipboard.writeText(detail.id).then(
                    () => setCopied(true),
                    (e) => setError(errorOf(e).message),
                  );
                }}
              >
                <Copy size={13} />
              </button>
            </dd>
            <dt>镜像</dt>
            <dd>{detail.image}</dd>
            <dt>重启策略</dt>
            <dd>
              {detail.restartPolicy || "no"}
              {detail.restartMaximum > 0 &&
                ` · 最多 ${detail.restartMaximum} 次`}
            </dd>
            <dt>重启次数</dt>
            <dd>{detail.restartCount}</dd>
            <dt>创建时间</dt>
            <dd>{date(detail.created)}</dd>
            <dt>最近启动</dt>
            <dd>{date(detail.startedAt)}</dd>
            <dt>最近退出</dt>
            <dd>{date(detail.finishedAt)}</dd>
            {detail.autoRemove && (
              <>
                <dt>自动删除</dt>
                <dd>退出后移除容器及匿名卷</dd>
              </>
            )}
          </dl>
          <details
            className="docker-detail-group"
            open={groups?.ports ?? true}
            onToggle={(e) => {
              if (groups?.ports !== e.currentTarget.open)
                onGroupsChange?.({
                  ports: e.currentTarget.open,
                  storage: groups?.storage ?? false,
                });
            }}
          >
            <summary>
              端口与网络{" "}
              <span>
                {detail.ports.length} 个端口 · {detail.networks.length} 个网络
              </span>
            </summary>
            {!detail.ports.length && !detail.networks.length && (
              <p className="resource-help">没有端口或网络信息。</p>
            )}
            {detail.ports.map((port, index) => (
              <div
                className="docker-detail-item"
                key={`${port.containerPort}/${index}`}
              >
                <code>
                  {port.hostPort
                    ? `${port.hostIp?.includes(":") ? `[${port.hostIp}]` : port.hostIp || "0.0.0.0"}:${port.hostPort} → ${port.containerPort}`
                    : port.containerPort}
                </code>
                <span>{port.hostPort ? "已发布到宿主" : "未发布到宿主"}</span>
                <div className="docker-inline-tools">
                  <button
                    className="resource-link"
                    onClick={() =>
                      void navigator.clipboard
                        .writeText(
                          `${port.hostIp || "0.0.0.0"}:${port.hostPort || "—"} → ${port.containerPort}`,
                        )
                        .catch((e) => setError(errorOf(e).message))
                    }
                  >
                    <Copy size={12} />
                    复制
                  </button>
                  {serverId &&
                    port.hostPort &&
                    port.containerPort.endsWith("/tcp") && (
                      <button
                        className="resource-link"
                        disabled={
                          !enabled ||
                          linkBusy ||
                          !endpoint.startsWith("unix:///")
                        }
                        title={
                          endpoint.startsWith("unix:///")
                            ? "核对宿主后打开转发草稿"
                            : "外部引擎无法确认属于当前 SSH 主机"
                        }
                        onClick={() =>
                          void hostLink({
                            type: "port",
                            containerPort: port.containerPort,
                            hostIp: port.hostIp || "",
                            hostPort: port.hostPort!,
                          })
                        }
                      >
                        <ArrowUpRight size={12} />
                        转发端口
                      </button>
                    )}
                </div>
              </div>
            ))}
            {detail.networks.map((network) => (
              <div className="docker-detail-item" key={network.name}>
                <strong>{network.name}</strong>
                <code>
                  {[network.address, network.ipv6]
                    .filter(Boolean)
                    .join(" · ") || "未分配地址"}
                </code>
              </div>
            ))}
          </details>
          <details
            className="docker-detail-group"
            open={groups?.storage ?? false}
            onToggle={(e) => {
              if (groups?.storage !== e.currentTarget.open)
                onGroupsChange?.({
                  ports: groups?.ports ?? true,
                  storage: e.currentTarget.open,
                });
            }}
          >
            <summary>
              存储 <span>{detail.mounts.length} 个挂载</span>
            </summary>
            {!detail.mounts.length && (
              <p className="resource-help">
                没有挂载。容器可写层会随容器删除。
              </p>
            )}
            {detail.mounts.map((mount, index) => (
              <div
                className="docker-detail-item"
                key={`${mount.destination}/${index}`}
              >
                <strong>
                  {mount.kind === "volume"
                    ? mount.name || "Volume"
                    : mount.kind === "bind"
                      ? "Bind Mount"
                      : mount.kind}{" "}
                  · {mount.readOnly ? "只读" : "读写"}
                </strong>
                <code>{mount.source || mount.name || "—"}</code>
                <code>→ {mount.destination}</code>
                <div className="docker-inline-tools">
                  <button
                    className="resource-link"
                    onClick={() =>
                      void navigator.clipboard
                        .writeText(mount.source || mount.name || "")
                        .catch((e) => setError(errorOf(e).message))
                    }
                  >
                    <Copy size={12} />
                    复制
                  </button>
                  {mount.kind === "bind" && onRevealPath && (
                    <button
                      className="resource-link"
                      disabled={
                        !enabled || linkBusy || !endpoint.startsWith("unix:///")
                      }
                      title={
                        endpoint.startsWith("unix:///")
                          ? "核对宿主后在文件面板定位"
                          : "外部引擎路径不属于当前 SSH 主机"
                      }
                      onClick={() =>
                        void hostLink({
                          type: "mount",
                          destination: mount.destination,
                        })
                      }
                    >
                      <FolderOpen size={12} />
                      在文件中显示
                    </button>
                  )}
                </div>
              </div>
            ))}
          </details>
          {engine && active && (
            <>
              <ContainerDiagnostics
                key={`${scope}/config`}
                sessionId={store.sessionId}
                token={engine.token}
                containerId={containerId}
                section="configuration"
              />
              <ContainerDiagnostics
                key={`${scope}/processes`}
                sessionId={store.sessionId}
                token={engine.token}
                containerId={containerId}
                section="processes"
              />
            </>
          )}
          <div className="docker-detail-freshness">
            <span>
              {detailState?.stale ? "上次数据" : "详情更新于"}{" "}
              {new Date(detail.updatedAt).toLocaleTimeString()}
            </span>
            <button
              className="resource-link"
              disabled={detailState?.loading}
              onClick={() =>
                void store.refresh().catch((e) => setError(errorOf(e).message))
              }
            >
              刷新
            </button>
          </div>
        </>
      )}
      {transfer && active && (
        <DockerTransferDialog
          kind={transfer}
          targetId={containerId}
          name={name}
          onClose={() => setTransfer(null)}
        />
      )}
      {preview && (
        <DockerConfirmDialog
          key={preview.id}
          preview={preview}
          onClose={() => {
            setPreview(null);
          }}
          onConfirm={() => submit(preview.action, preview)}
        />
      )}
      {settings && detail && (
        <ContainerSettingsDialog
          mode={settings}
          detail={detail}
          onSubmit={submit}
          onClose={() => setSettings(null)}
        />
      )}
    </div>
  );
}
