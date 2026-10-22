import { CreateContainerDialog } from "./docker/CreateContainerDialog";
import { monitorLive } from "./resourceMonitoring";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowLeft,
  Boxes,
  Layers,
  MoreHorizontal,
  RefreshCw,
  ScrollText,
  Search,
  TerminalSquare,
  X,
} from "lucide-react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import { api } from "./api";
import {
  useDockerManagement,
  useDockerDetail,
} from "./DockerManagementProvider";
import { ResourceManagement } from "./docker/ResourceManagement";
import { useDockerActions } from "./docker/useDockerActions";
import { DockerConfirmDialog } from "./docker/DockerConfirmDialog";
import { ComposeManagement } from "./docker/ComposeManagement";
import { ImageManagement, ImagePullButton } from "./docker/ImageManagement";
import { ContainerManagement } from "./docker/ContainerManagement";
import { DockerOperations } from "./docker/DockerOperations";
import { operationActive, type ContainerAction } from "./dockerApi";
import { PortForwardMenu } from "./PortForwardMenu";
import "./docker/docker.css";
import { useMonitor, useDockerInterest } from "./MonitorProvider";
import { dockerStatusNames } from "./dockerPresentation";
import type { Metric } from "./resourceMonitoring";
import { dockerCanAttempt } from "./serverMonitoring";
import { metricMessage } from "./resourcePresentation";
import {
  containerStateNames,
  dockerKey,
  dockerMatches,
  normalizeDockerSnapshot,
  orderDockerItems,
  type DockerFilter,
  type DockerPage,
  type DockerPanelSnapshot,
  type DockerSort,
} from "./dockerPresentation";
import { errorOf, type Container, type DockerImage } from "./types";
export type { DockerPanelSnapshot } from "./dockerPresentation";
type Operation = "logs" | "terminal";
type ActionState = { busy: boolean; error: string };
export function DockerPanel({
  sessionId,
  serverName,
  serverId,
  onRevealPath,
  active = true,
  initialState,
  onStateChange,
}: {
  sessionId: string | null;
  serverName: string;
  serverId?: string;
  onRevealPath?: (path: string) => void;
  active?: boolean;
  initialState?: DockerPanelSnapshot;
  onStateChange?: (snapshot: DockerPanelSnapshot) => void;
}) {
  const { store, state } = useMonitor();
  const docker = state.docker;
  const management = useDockerManagement();
  const [view, setView] = useState(() => normalizeDockerSnapshot(initialState));
  const [actions, setActions] = useState<Record<string, ActionState>>({});
  const [sortRevision, setSortRevision] = useState(0);
  const [refreshError, setRefreshError] = useState("");
  const refreshFlight = useRef(false);
  const [requestedAction, setRequestedAction] =
    useState<ContainerAction | null>(null);
  const root = useRef<HTMLElement>(null),
    list = useRef<VirtuosoHandle>(null),
    scroller = useRef<HTMLElement | null>(null);
  const savedScroll = useRef(view.scrollTop),
    savedSelection = useRef<Container | DockerImage | null>(null);
  const pendingFocus = useRef<string | null>(null);
  const keyboardFocusFrame = useRef(0);
  const restoringScroll = useRef(false);
  const scrollRestoreVersion = useRef(0);
  const currentScroll = () => savedScroll.current;
  const revealSelection = useRef<string | null>(null);
  const id = useId();
  const status = docker.engine.value;
  const engineState =
    !monitorLive(state) && !sessionId
      ? "disconnected"
      : (status?.state ?? "detecting");
  const endpoint = status?.endpoint ?? "";
  const token = `${docker.engineGeneration}\0${sessionId ?? ""}\0${endpoint}`;
  const [bulk, setBulk] = useState(false),
    [checked, setChecked] = useState(new Set<string>());
  const [creating, setCreating] = useState<{ id: string; name: string } | null>(
    null,
  );
  const bulkActions = useDockerActions(
    `${token}/${active}/${view.section}`,
    !!sessionId && active && monitorLive(state) && dockerCanAttempt(docker),
  );
  useEffect(() => {
    setBulk(false);
    setChecked(new Set());
  }, [token, view.page, view.section]);
  const currentToken = useRef(token);
  currentToken.current = token;
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      cancelAnimationFrame(keyboardFocusFrame.current);
    };
  }, []);
  const previousEngine = useRef({
    generation: docker.engineGeneration,
    endpoint,
  });
  const restoredSelection = useRef(initialState?.selection);
  useLayoutEffect(() => {
    const previous = previousEngine.current;
    const restored =
      docker.engineGeneration > 0 ? restoredSelection.current : null;
    if (docker.engineGeneration > 0) restoredSelection.current = null;
    if (
      (restored?.engineGeneration != null &&
        restored.engineGeneration !== docker.engineGeneration) ||
      (restored?.endpoint != null &&
        endpoint &&
        restored.endpoint !== endpoint) ||
      (previous.generation > 0 &&
        previous.generation !== docker.engineGeneration) ||
      (previous.endpoint && endpoint && previous.endpoint !== endpoint)
    ) {
      savedSelection.current = null;
      setView((v) => ({ ...v, selection: null, detailOpen: false }));
      savedScroll.current = { containers: 0, images: 0 };
      pendingFocus.current = null;
      revealSelection.current = null;
    }
    previousEngine.current = { generation: docker.engineGeneration, endpoint };
  }, [docker.engineGeneration, endpoint]);
  useEffect(() => {
    setActions({});
    setRequestedAction(null);
  }, [token]);
  useDockerInterest({
    summary: active,
    containers:
      active && view.section === "objects" && view.page === "containers",
    images: active && view.section === "objects" && view.page === "images",
  });
  useEffect(() => {
    onStateChange?.({
      ...view,
      scrollTop: savedScroll.current,
    });
  }, [view, onStateChange]);
  const order = useRef<{
    scope: string;
    containers: string[];
    images: string[];
  }>({ scope: "", containers: [], images: [] });
  const items = useMemo(() => {
    const scope = `${docker.engineGeneration}/${view.sort}/${sortRevision}`;
    if (order.current.scope !== scope)
      order.current = { scope, containers: [], images: [] };
    const rows: (Container | DockerImage)[] =
      view.page === "containers"
        ? (docker.containers.value ?? [])
        : (docker.images.value ?? []);
    const result = orderDockerItems(rows, view.sort, order.current[view.page]);
    order.current[view.page] = result.map(dockerKey);
    return result;
  }, [
    docker.engineGeneration,
    docker.containers.value,
    docker.images.value,
    view.page,
    view.sort,
    sortRevision,
  ]);
  const query = view.queries[view.page];
  const filtered = useMemo(
    () =>
      items.filter((item) =>
        dockerMatches(
          item,
          query,
          view.page === "containers" ? view.filter : "all",
          docker.summary.value?.unhealthyIds,
        ),
      ),
    [items, query, view.page, view.filter, docker.summary.value?.unhealthyIds],
  );
  const selection = view.selection;
  const selectedKey = selection
    ? selection.kind === "containers"
      ? selection.id
      : `${selection.id}/${selection.name}`
    : "";
  const selectedRows =
    selection?.kind === "images"
      ? docker.images.value
      : docker.containers.value;
  const current = selectedRows?.find((item) => dockerKey(item) === selectedKey);
  if (current) savedSelection.current = current;
  const selected =
    current ??
    (savedSelection.current && dockerKey(savedSelection.current) === selectedKey
      ? savedSelection.current
      : null);
  const selectedMetric =
    selection?.kind === "images" ? docker.images : docker.containers;
  const removed = !!selection && !current && selectedMetric.status === "ready";
  const filteredOut =
    !!selection &&
    selection.kind === view.page &&
    !!current &&
    !filtered.some((item) => dockerKey(item) === selectedKey);
  const connected =
    !!sessionId && monitorLive(state) && dockerCanAttempt(docker);
  const canAct = connected && active && !!current && "state" in current;
  const detailBack = useRef<HTMLButtonElement>(null);
  const showDetails =
    view.section === "objects" && !!selection && view.detailOpen;
  useDockerDetail(
    selection?.kind === "containers" ? selection.id : null,
    active && showDetails,
  );
  useEffect(() => {
    if (active && showDetails) detailBack.current?.focus();
  }, [showDetails]);
  function backToList() {
    setView((v) => ({ ...v, detailOpen: false }));
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        root.current
          ?.querySelector<HTMLElement>('[aria-current="true"]')
          ?.focus({ preventScroll: true }),
      ),
    );
  }
  const metric = view.page === "containers" ? docker.containers : docker.images;
  const freshnessMessage = (value: Metric<unknown>) =>
    monitorLive(state) && value.status === "stale" && value.value != null
      ? "上次数据" +
        (value.updatedAt != null
          ? " · " + new Date(value.updatedAt).toLocaleTimeString()
          : "")
      : metricMessage(value, state);
  async function refreshDocker() {
    if (!sessionId || refreshFlight.current || docker.refreshing) return;
    refreshFlight.current = true;
    setRefreshError("");
    setSortRevision((v) => v + 1);
    const origin = token;
    try {
      await Promise.all([store.refresh("docker"), management.store.refresh()]);
    } catch (e) {
      if (currentToken.current === origin) setRefreshError(errorOf(e).message);
    } finally {
      refreshFlight.current = false;
    }
  }
  const hasFilter =
    !!query || (view.page === "containers" && view.filter !== "all");
  const operationKey = (containerId: string, operation: Operation) =>
    `${token}\0${containerId}\0${operation}`;
  async function act(container: Container, operation: Operation) {
    if (
      !canAct ||
      !sessionId ||
      (operation === "terminal" &&
        docker.containers.status === "ready" &&
        container.state !== "running")
    )
      return;
    const key = operationKey(container.id, operation),
      requestToken = token;
    if (actions[key]?.busy) return;
    setActions((a) => ({ ...a, [key]: { busy: true, error: "" } }));
    try {
      if (operation === "logs")
        await api.openContainerLogs(sessionId, container, endpoint);
      else await api.openContainer(sessionId, container, endpoint);
      if (alive.current && currentToken.current === requestToken)
        setActions((a) => ({ ...a, [key]: { busy: false, error: "" } }));
    } catch (error) {
      if (alive.current && currentToken.current === requestToken)
        setActions((a) => ({
          ...a,
          [key]: { busy: false, error: errorOf(error).message },
        }));
    }
  }
  function choose(item: Container | DockerImage) {
    rememberScroll();
    savedSelection.current = item;
    revealSelection.current = dockerKey(item);
    setView((v) => ({
      ...v,
      selection: {
        kind: view.page,
        id: item.id,
        name: item.name,
        engineGeneration: docker.engineGeneration,
        endpoint,
      },
      detailOpen: true,
    }));
  }
  function switchPage(page: DockerPage) {
    rememberScroll();
    setView((v) => ({ ...v, section: "objects", page, detailOpen: false }));
  }
  const receiveScroller = useCallback(
    (element: HTMLElement | Window | null) => {
      scroller.current = element instanceof HTMLElement ? element : null;
    },
    [],
  );
  useLayoutEffect(() => {
    restoringScroll.current = true;
    pendingFocus.current = null;
    cancelAnimationFrame(keyboardFocusFrame.current);
    if (!active || showDetails) return;
    const version = ++scrollRestoreVersion.current;
    const top = currentScroll()[view.page];
    let frame = requestAnimationFrame(() => {
      if (scrollRestoreVersion.current !== version) return;
      list.current?.scrollTo({ top, behavior: "auto" });
      frame = requestAnimationFrame(() => {
        restoringScroll.current = false;
      });
    });
    return () => cancelAnimationFrame(frame);
  }, [active, view.page, showDetails]);
  useEffect(() => {
    if (!active || showDetails || !scroller.current) return;
    let frame = 0;
    const reveal = () => {
      const index = filtered.findIndex(
        (item) => dockerKey(item) === revealSelection.current,
      );
      if (index < 0 || !scroller.current) return;
      const target = document.getElementById(`${id}-row-${index}`);
      if (!target) {
        list.current?.scrollIntoView({
          index,
          done: () => {
            frame = requestAnimationFrame(reveal);
          },
        });
        return;
      }
      const bounds = scroller.current.getBoundingClientRect(),
        row = target.getBoundingClientRect();
      if (!bounds.height || !row.height) return;
      // Measure the committed layout: Virtuoso's cached viewport can still have
      // the previous height while the detail pane is opening.
      if (row.bottom > bounds.bottom)
        scroller.current.scrollTop += row.bottom - bounds.bottom;
      else if (row.top < bounds.top)
        scroller.current.scrollTop -= bounds.top - row.top;
      const next = {
        ...currentScroll(),
        [view.page]: scroller.current.scrollTop,
      };
      savedScroll.current = next;
      revealSelection.current = null;
    };
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(reveal);
    });
    observer.observe(scroller.current);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [active, showDetails, selection, filtered, id]);
  function rememberScroll() {
    if (!active || showDetails || !scroller.current || restoringScroll.current)
      return;
    const next = {
      ...currentScroll(),
      [view.page]: scroller.current.scrollTop,
    };
    savedScroll.current = next;
    onStateChange?.({
      ...view,
      scrollTop: savedScroll.current,
    });
  }
  function focusRow(index: number) {
    ++scrollRestoreVersion.current;
    restoringScroll.current = false;
    cancelAnimationFrame(keyboardFocusFrame.current);
    const key = dockerKey(filtered[index]);
    pendingFocus.current = key;
    let remaining = 120;
    const reveal = () => {
      if (
        !alive.current ||
        pendingFocus.current !== key ||
        currentToken.current !== token
      )
        return;
      const element = document.getElementById(`${id}-row-${index}`);
      const viewport = scroller.current?.getBoundingClientRect();
      const row = element?.getBoundingClientRect();
      if (
        element &&
        row &&
        viewport &&
        row.height > 0 &&
        row.top >= viewport.top - 1 &&
        row.bottom <= viewport.bottom + 1
      ) {
        element.focus({ preventScroll: true });
        pendingFocus.current = null;
        rememberScroll();
        return;
      }
      // Variable row measurements can settle after the first scroll, especially
      // in WebKit. Keep the intent until the requested row is actually visible.
      if (element && row && viewport && scroller.current) {
        if (row.bottom > viewport.bottom)
          scroller.current.scrollTop += row.bottom - viewport.bottom;
        else if (row.top < viewport.top)
          scroller.current.scrollTop -= viewport.top - row.top;
      } else {
        list.current?.scrollToIndex({
          index,
          align: "start",
          behavior: "auto",
        });
      }
      if (--remaining > 0)
        keyboardFocusFrame.current = requestAnimationFrame(reveal);
      else pendingFocus.current = null;
    };
    reveal();
  }
  const summary = docker.summary.value;
  return (
    <section
      ref={root}
      className={`docker-panel docker-inspector${showDetails ? " showing-detail" : ""}`}
      aria-label="Docker 引擎与对象"
    >
      <header className="docker-heading">
        <div className="docker-engine-title">
          <Boxes size={17} />
          <strong>Docker</strong>
          <span className={`docker-state ${connected ? "running" : ""}`}>
            {connected && docker.engine.status === "stale"
              ? "状态待确认"
              : (dockerStatusNames[engineState] ?? engineState)}
          </span>
          <button
            className="icon-button"
            aria-label="刷新 Docker 状态"
            disabled={!sessionId || docker.refreshing}
            onClick={() => void refreshDocker()}
          >
            <RefreshCw
              size={14}
              className={docker.refreshing ? "spinning" : ""}
            />
          </button>
          <PortForwardMenu
            label="Docker 更多功能"
            items={[
              {
                label: "空间与资源",
                run: () =>
                  setView((v) => ({
                    ...v,
                    section: "resources",
                    detailOpen: false,
                  })),
              },
            ]}
          >
            <MoreHorizontal size={15} />
          </PortForwardMenu>
        </div>
        <div className="docker-endpoint" title={endpoint}>
          {endpoint || serverName || "等待引擎信息"}
        </div>
        {summary && (
          <div className="docker-summary-inline">
            {summary.total} 个容器 · {summary.byState.running ?? 0} 运行 ·{" "}
            {summary.byState.exited ?? 0} 停止
            {(summary.byState.restarting ?? 0) > 0 &&
              ` · ${summary.byState.restarting} 重启中`}
          </div>
        )}
        <div className="resource-help">{freshnessMessage(metric)}</div>
      </header>
      <div
        className="docker-controls"
        hidden={showDetails || view.section === "resources"}
      >
        <div
          className="docker-segments"
          role="group"
          aria-label="Docker 对象类型"
        >
          {(["containers", "images"] as const).map((page) => (
            <button
              key={page}
              aria-pressed={view.section === "objects" && view.page === page}
              onClick={() => switchPage(page)}
            >
              {page === "containers" ? "容器" : "镜像"}
              <span>
                {(page === "containers" ? docker.containers : docker.images)
                  .value?.length ?? "—"}
              </span>
            </button>
          ))}
          <button
            aria-pressed={view.section === "projects"}
            onClick={() =>
              setView((v) => ({ ...v, section: "projects", detailOpen: false }))
            }
          >
            项目
          </button>
        </div>
        <label className="docker-search" hidden={view.section !== "objects"}>
          <Search size={14} />
          <input
            aria-label={view.page === "containers" ? "搜索容器" : "搜索镜像"}
            placeholder={
              view.page === "containers"
                ? "搜索名称、镜像或 ID"
                : "搜索名称或 ID"
            }
            value={query}
            onChange={(e) =>
              setView((v) => ({
                ...v,
                queries: { ...v.queries, [v.page]: e.target.value },
              }))
            }
          />
          {query && (
            <button
              className="icon-button"
              aria-label="清除 Docker 搜索"
              onClick={() =>
                setView((v) => ({
                  ...v,
                  queries: { ...v.queries, [v.page]: "" },
                }))
              }
            >
              <X size={13} />
            </button>
          )}
        </label>
        <div className="docker-filters" hidden={view.section !== "objects"}>
          {view.page === "images" && (
            <ImagePullButton
              enabled={
                active &&
                !!sessionId &&
                !!management.state.engine &&
                metric.status === "ready"
              }
            />
          )}
          {view.page === "containers" && (
            <select
              aria-label="容器状态筛选"
              value={view.filter}
              onChange={(e) =>
                setView((v) => ({
                  ...v,
                  filter: e.target.value as DockerFilter,
                }))
              }
            >
              <option value="all">全部状态</option>
              <option value="running">运行中</option>
              <option value="attention">需要关注</option>
              <option value="stopped">已停止</option>
              <option value="other">其他状态</option>
            </select>
          )}
          <select
            aria-label="Docker 排序"
            value={view.sort}
            onChange={(e) =>
              setView((v) => ({ ...v, sort: e.target.value as DockerSort }))
            }
          >
            <option value="name">按名称排序</option>
            {view.page === "containers" && (
              <option value="attention">异常状态优先</option>
            )}
            {view.page === "images" && view.sort === "attention" && (
              <option value="attention">按名称排序</option>
            )}
          </select>
          {view.page === "containers" && (
            <button
              className="resource-link"
              onClick={() => {
                setBulk((v) => !v);
                setChecked(new Set());
              }}
            >
              {bulk ? "取消选择" : "批量"}
            </button>
          )}
          <span>{filtered.length} 项</span>
        </div>
      </div>
      {bulk && view.section === "objects" && !showDetails && (
        <div className="docker-bulk-toolbar">
          <span>已选 {checked.size} 项</span>
          {(["start", "stop", "restart", "removeContainer"] as const).map(
            (action) => (
              <button
                key={action}
                disabled={
                  !checked.size ||
                  bulkActions.busy ||
                  bulkActions.locked ||
                  !connected
                }
                onClick={() =>
                  void bulkActions
                    .act("selection", {
                      type: "batch",
                      entries: [...checked].map((targetId) => ({
                        targetId,
                        action,
                      })),
                    })
                    .catch(() => {})
                }
              >
                {
                  {
                    start: "启动",
                    stop: "停止",
                    restart: "重启",
                    removeContainer: "删除…",
                  }[action]
                }
              </button>
            ),
          )}
        </div>
      )}
      {bulkActions.error && (
        <p className="resource-error" role="alert">
          {bulkActions.error}
        </p>
      )}
      {bulkActions.uncertain && (
        <button
          disabled={bulkActions.busy}
          onClick={() => void bulkActions.retry().catch(() => {})}
        >
          核对提交结果
        </button>
      )}
      {(docker.engine.error ||
        (engineState !== "running" && status?.message) ||
        metric.error ||
        refreshError) && (
        <div className="docker-notice" role="status">
          <span>
            {refreshError ||
              docker.engine.error ||
              metric.error ||
              status?.message}
            {metric.value && (
              <>
                {" "}
                · 保留上次列表
                {metric.updatedAt != null && (
                  <>（{new Date(metric.updatedAt).toLocaleTimeString()}）</>
                )}
                。
              </>
            )}
            {connected && " 正在自动重试，操作前会重新确认容器状态。"}
          </span>{" "}
          <button
            className="resource-link"
            disabled={!sessionId || docker.refreshing}
            onClick={() => void refreshDocker()}
          >
            立即重试
          </button>
        </div>
      )}
      {view.section === "resources" && (
        <ResourceManagement
          key={token}
          active={active}
          enabled={connected}
          onBack={() => setView((v) => ({ ...v, section: "objects" }))}
          onContainer={(id, name) =>
            setView((v) => ({
              ...v,
              section: "objects",
              page: "containers",
              detailOpen: true,
              selection: {
                kind: "containers",
                id,
                name,
                engineGeneration: docker.engineGeneration,
                endpoint,
              },
            }))
          }
        />
      )}
      {view.section === "projects" && (
        <ComposeManagement
          key={token}
          active={active}
          enabled={connected}
          onContainer={(id, name) =>
            setView((v) => ({
              ...v,
              section: "objects",
              page: "containers",
              detailOpen: true,
              selection: {
                kind: "containers",
                id,
                name,
                engineGeneration: docker.engineGeneration,
                endpoint,
              },
            }))
          }
        />
      )}
      <div className="docker-content" hidden={view.section !== "objects"}>
        <div className="docker-list-pane" hidden={showDetails}>
          <div
            className="docker-list"
            role="list"
            aria-label={view.page === "containers" ? "容器列表" : "镜像列表"}
            onScrollCapture={rememberScroll}
          >
            {filtered.length ? (
              <Virtuoso
                key={view.page}
                ref={list}
                scrollerRef={receiveScroller}
                data={filtered}
                initialScrollTop={currentScroll()[view.page]}
                defaultItemHeight={58}
                increaseViewportBy={120}
                computeItemKey={(_, item) => dockerKey(item)}
                itemContent={(index, item) => {
                  const key = dockerKey(item),
                    isContainer = "state" in item;
                  const operation = management.state.operations.find(
                    (task) =>
                      task.engine.endpoint === endpoint &&
                      task.targetId === item.id &&
                      (operationActive(task) || task.phase === "unknown"),
                  );
                  const unhealthy =
                    docker.summary.value?.unhealthyIds?.includes(item.id);
                  return (
                    <div role="listitem" className="docker-list-item">
                      {bulk && isContainer && (
                        <input
                          type="checkbox"
                          className="docker-bulk-checkbox"
                          aria-label={`选择 ${item.name}`}
                          checked={checked.has(item.id)}
                          onChange={() =>
                            setChecked((v) => {
                              const next = new Set(v);
                              if (next.has(item.id)) next.delete(item.id);
                              else next.add(item.id);
                              return next;
                            })
                          }
                        />
                      )}
                      <button
                        id={`${id}-row-${index}`}
                        type="button"
                        aria-current={
                          selectedKey === key && selection?.kind === view.page
                        }
                        tabIndex={
                          selectedKey === key ||
                          (!filtered.some(
                            (row) => dockerKey(row) === selectedKey,
                          ) &&
                            index === 0)
                            ? 0
                            : -1
                        }
                        className="docker-object-row"
                        onClick={() => choose(item)}
                        onKeyDown={(e) => {
                          if (e.shiftKey && e.key === "F10") {
                            e.preventDefault();
                            e.currentTarget.parentElement
                              ?.querySelector<HTMLButtonElement>(
                                '[aria-haspopup="menu"]',
                              )
                              ?.click();
                            return;
                          }
                          if (
                            !["ArrowDown", "ArrowUp", "Home", "End"].includes(
                              e.key,
                            )
                          )
                            return;
                          e.preventDefault();
                          const next =
                            e.key === "Home"
                              ? 0
                              : e.key === "End"
                                ? filtered.length - 1
                                : Math.max(
                                    0,
                                    Math.min(
                                      filtered.length - 1,
                                      index + (e.key === "ArrowDown" ? 1 : -1),
                                    ),
                                  );
                          focusRow(next);
                        }}
                        title={`${item.name}\n${item.id}`}
                      >
                        {isContainer ? (
                          <span
                            className={`docker-dot state-${item.state}`}
                            aria-hidden="true"
                          />
                        ) : (
                          <Layers size={15} />
                        )}
                        <span className="docker-object-text">
                          <strong>{item.name || item.id.slice(0, 12)}</strong>
                          <span>
                            {isContainer ? item.image : item.id.slice(0, 12)}
                          </span>
                        </span>
                        <span
                          className={`docker-object-state ${isContainer ? `state-${item.state}` : ""}`}
                        >
                          {isContainer
                            ? operation
                              ? operation.phase === "unknown"
                                ? "待确认"
                                : "操作中"
                              : unhealthy
                                ? "不健康"
                                : (containerStateNames[item.state] ??
                                  item.state)
                            : item.size}
                        </span>
                      </button>
                      {isContainer && (
                        <PortForwardMenu
                          label={`${item.name} 列表操作`}
                          disabled={!connected || !!operation}
                          items={[
                            { label: "查看详情", run: () => choose(item) },
                            ...(["created", "exited"].includes(item.state)
                              ? [
                                  {
                                    label: "启动",
                                    run: () => {
                                      choose(item);
                                      setRequestedAction("start");
                                    },
                                  },
                                ]
                              : []),
                            ...(item.state === "running"
                              ? [
                                  {
                                    label: "停止",
                                    run: () => {
                                      choose(item);
                                      setRequestedAction("stop");
                                    },
                                  },
                                  {
                                    label: "重启",
                                    run: () => {
                                      choose(item);
                                      setRequestedAction("restart");
                                    },
                                  },
                                ]
                              : []),
                            ...(["created", "exited", "dead"].includes(
                              item.state,
                            )
                              ? [
                                  {
                                    label: "删除容器…",
                                    danger: true,
                                    run: () => {
                                      choose(item);
                                      setRequestedAction("remove");
                                    },
                                  },
                                ]
                              : []),
                          ]}
                        >
                          <MoreHorizontal size={15} />
                        </PortForwardMenu>
                      )}
                    </div>
                  );
                }}
              />
            ) : (
              <div className="docker-empty">
                <Boxes size={24} />
                <strong>
                  {hasFilter
                    ? "没有匹配的对象"
                    : metric.status === "ready"
                      ? view.page === "containers"
                        ? "还没有容器"
                        : "还没有镜像"
                      : metric.status === "collecting"
                        ? "正在读取列表…"
                        : "列表暂不可用"}
                </strong>
                <span>
                  {hasFilter
                    ? "试试其他关键词或清除筛选。"
                    : "此处显示当前 Docker 引擎中的对象。"}
                </span>
              </div>
            )}
          </div>
          {view.page === "containers" && summary && (
            <div className="docker-health">
              {summary.unhealthy == null
                ? "健康统计暂不可用"
                : summary.unhealthy > 0
                  ? `${summary.unhealthy} 个容器报告不健康`
                  : "未报告不健康容器"}
              {summary.healthError && (
                <details>
                  <summary>查看原因</summary>
                  <p>{summary.healthError}</p>
                </details>
              )}
            </div>
          )}
        </div>
        <section
          className={`docker-detail${selection ? " has-selection" : ""}`}
          aria-label="Docker 对象详情"
          hidden={!showDetails}
        >
          {showDetails && (
            <button
              ref={detailBack}
              className="resource-link docker-back"
              onClick={backToList}
            >
              <ArrowLeft size={14} />
              返回列表
            </button>
          )}
          {selection ? (
            <>
              <div className="docker-detail-title">
                <span>
                  {selection.kind === "containers" ? "容器详情" : "镜像详情"}
                </span>
                <strong>{current?.name ?? selection.name}</strong>
              </div>
              {removed && (
                <p className="docker-notice" role="status">
                  此对象已从引擎移除。保留身份信息供核对。
                </p>
              )}
              {!current && !removed && (
                <p className="resource-help">等待对象数据；暂时无法操作。</p>
              )}
              {filteredOut && (
                <p className="docker-notice">
                  所选对象被当前筛选隐藏。
                  <button
                    className="resource-link"
                    onClick={() =>
                      setView((v) => ({
                        ...v,
                        filter: "all",
                        queries: { ...v.queries, [v.page]: "" },
                      }))
                    }
                  >
                    清除筛选
                  </button>
                </p>
              )}
              <div className="resource-help">
                {freshnessMessage(selectedMetric)}
              </div>
              {selection.kind === "containers" ? (
                <ContainerManagement
                  key={`${token}/${selection.id}`}
                  groups={view.detailGroups[`${endpoint}/${selection.id}`]}
                  onGroupsChange={(groups) =>
                    setView((v) => ({
                      ...v,
                      detailGroups: Object.fromEntries(
                        [
                          ...Object.entries(v.detailGroups).filter(
                            ([key]) => key !== `${endpoint}/${selection.id}`,
                          ),
                          [`${endpoint}/${selection.id}`, groups],
                        ].slice(-30),
                      ),
                    }))
                  }
                  serverId={serverId}
                  onRevealPath={onRevealPath}
                  containerId={selection.id}
                  name={selection.name}
                  engineGeneration={docker.engineGeneration}
                  endpoint={endpoint}
                  enabled={canAct}
                  active={active && showDetails}
                  requestedAction={requestedAction}
                  onActionConsumed={() => setRequestedAction(null)}
                  fallback={
                    <dl className="docker-facts">
                      <dt>ID</dt>
                      <dd className="docker-id">{selection.id}</dd>
                      {selected && "state" in selected ? (
                        <>
                          <dt>镜像</dt>
                          <dd>{selected.image}</dd>
                          <dt>状态</dt>
                          <dd>
                            {containerStateNames[selected.state] ??
                              selected.state}{" "}
                            · {selected.status}
                          </dd>
                        </>
                      ) : (
                        selected && (
                          <>
                            <dt>大小</dt>
                            <dd>{selected.size}</dd>
                          </>
                        )
                      )}
                    </dl>
                  }
                >
                  <div className="docker-actions">
                    {(["logs", "terminal"] as const).map((operation) => {
                      const result =
                          actions[operationKey(selection.id, operation)],
                        allowed =
                          canAct &&
                          (operation === "logs" ||
                            docker.containers.status === "stale" ||
                            (current &&
                              "state" in current &&
                              current.state === "running"));
                      return (
                        <div key={operation}>
                          <button
                            disabled={!allowed || result?.busy}
                            onClick={() =>
                              current &&
                              "state" in current &&
                              void act(current, operation)
                            }
                            aria-label={
                              operation === "logs"
                                ? `查看 ${selection.name} 日志`
                                : `进入 ${selection.name} 终端`
                            }
                            title={
                              operation === "terminal" &&
                              current &&
                              "state" in current &&
                              current.state !== "running" &&
                              docker.containers.status === "ready"
                                ? "仅运行中的容器可打开终端"
                                : "在独立窗口打开"
                            }
                          >
                            {operation === "logs" ? (
                              <ScrollText size={14} />
                            ) : (
                              <TerminalSquare size={14} />
                            )}{" "}
                            {result?.busy
                              ? "正在打开…"
                              : operation === "logs"
                                ? "日志"
                                : "终端"}
                          </button>
                          {result?.error && (
                            <p className="resource-error" role="alert">
                              {result.error}
                            </p>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </ContainerManagement>
              ) : (
                <ImageManagement
                  onCreate={(id, name) => setCreating({ id, name })}
                  key={`${token}/${selection.id}`}
                  imageId={selection.id}
                  name={selection.name}
                  enabled={
                    connected && !!current && docker.images.status === "ready"
                  }
                  active={active && showDetails}
                  onContainer={(id, name) => {
                    setView((v) => ({
                      ...v,
                      page: "containers",
                      detailOpen: true,
                      selection: {
                        kind: "containers",
                        id,
                        name,
                        engineGeneration: docker.engineGeneration,
                        endpoint,
                      },
                    }));
                  }}
                />
              )}
            </>
          ) : (
            <div className="docker-detail-empty">
              <Layers size={22} />
              <span>选择对象查看详情</span>
            </div>
          )}
        </section>
      </div>
      <DockerOperations
        endpoint={endpoint}
        onContainer={(id, name) =>
          setView((v) => ({
            ...v,
            section: "objects",
            page: "containers",
            detailOpen: true,
            selection: {
              kind: "containers",
              id,
              name,
              engineGeneration: docker.engineGeneration,
              endpoint,
            },
          }))
        }
      />
      {creating && (
        <CreateContainerDialog
          imageId={creating.id}
          imageName={creating.name}
          onClose={() => setCreating(null)}
        />
      )}
      {bulkActions.preview && (
        <DockerConfirmDialog
          preview={bulkActions.preview}
          onRefresh={bulkActions.renewPreview}
          uncertain={bulkActions.uncertain}
          onRetry={bulkActions.retry}
          onClose={bulkActions.dismissPreview}
          onConfirm={() =>
            bulkActions.submit(
              bulkActions.preview!.targetId,
              bulkActions.preview!.action,
              bulkActions.preview,
            )
          }
        />
      )}
    </section>
  );
}
