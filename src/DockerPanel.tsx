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
  RefreshCw,
  ScrollText,
  Search,
  TerminalSquare,
  X,
} from "lucide-react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import { api } from "./api";
import { useMonitor, useDockerInterest } from "./MonitorProvider";
import { dockerStatusNames } from "./dockerPresentation";
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
  active = true,
  initialState,
  onStateChange,
}: {
  sessionId: string | null;
  serverName: string;
  active?: boolean;
  initialState?: DockerPanelSnapshot;
  onStateChange?: (snapshot: DockerPanelSnapshot) => void;
}) {
  const { store, state } = useMonitor();
  const docker = state.docker;
  const [view, setView] = useState(() => normalizeDockerSnapshot(initialState));
  const [actions, setActions] = useState<Record<string, ActionState>>({});
  const [sortRevision, setSortRevision] = useState(0);
  const root = useRef<HTMLElement>(null),
    list = useRef<VirtuosoHandle>(null),
    scroller = useRef<HTMLElement | null>(null);
  const savedScroll = useRef(view.scrollTop),
    savedSelection = useRef<Container | DockerImage | null>(null);
  const pendingFocus = useRef<string | null>(null);
  const restoringScroll = useRef(false);
  const currentScroll = () => savedScroll.current;
  const revealSelection = useRef<string | null>(null);
  const id = useId();
  const status = docker.engine.value;
  const engineState =
    !state.connected && !sessionId
      ? "disconnected"
      : (status?.state ?? "detecting");
  const endpoint = status?.endpoint ?? "";
  const token = `${docker.engineGeneration}\0${sessionId ?? ""}\0${endpoint}`;
  const currentToken = useRef(token);
  currentToken.current = token;
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
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
  }, [token]);
  useDockerInterest({
    summary: active,
    containers: active && view.page === "containers",
    images: active && view.page === "images",
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
        ),
      ),
    [items, query, view.page, view.filter],
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
    !!sessionId &&
    state.connected &&
    engineState === "running" &&
    docker.engine.status === "ready";
  const canAct =
    connected &&
    docker.containers.status === "ready" &&
    !!current &&
    "state" in current;
  const detailBack = useRef<HTMLButtonElement>(null);
  const showDetails = !!selection && view.detailOpen;
  useEffect(() => {
    if (active && showDetails) detailBack.current?.focus();
  }, [active, showDetails]);
  function backToList() {
    setView((v) => ({ ...v, detailOpen: false }));
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        root.current
          ?.querySelector<HTMLElement>('[aria-selected="true"]')
          ?.focus({ preventScroll: true }),
      ),
    );
  }
  const metric = view.page === "containers" ? docker.containers : docker.images;
  const hasFilter =
    !!query || (view.page === "containers" && view.filter !== "all");
  const operationKey = (containerId: string, operation: Operation) =>
    `${token}\0${containerId}\0${operation}`;
  async function act(container: Container, operation: Operation) {
    if (
      !canAct ||
      !sessionId ||
      (operation === "terminal" && container.state !== "running")
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
    setView((v) => ({ ...v, page, detailOpen: false }));
  }
  const receiveScroller = useCallback(
    (element: HTMLElement | Window | null) => {
      scroller.current = element instanceof HTMLElement ? element : null;
    },
    [],
  );
  useLayoutEffect(() => {
    restoringScroll.current = true;
    if (!active || showDetails) return;
    const top = currentScroll()[view.page];
    let frame = requestAnimationFrame(() => {
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
            {dockerStatusNames[engineState] ?? engineState}
          </span>
          <button
            className="icon-button"
            aria-label="刷新 Docker 状态"
            disabled={!sessionId || docker.refreshing}
            onClick={() => {
              setSortRevision((v) => v + 1);
              void store.refresh("docker");
            }}
          >
            <RefreshCw
              size={14}
              className={docker.refreshing ? "spinning" : ""}
            />
          </button>
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
        <div className="resource-help">
          {metricMessage(metric, state.connected)}
        </div>
      </header>
      <div className="docker-controls" hidden={showDetails}>
        <div
          className="docker-segments"
          role="group"
          aria-label="Docker 对象类型"
        >
          {(["containers", "images"] as const).map((page) => (
            <button
              key={page}
              aria-pressed={view.page === page}
              onClick={() => switchPage(page)}
            >
              {page === "containers" ? "容器" : "镜像"}
              <span>
                {(page === "containers" ? docker.containers : docker.images)
                  .value?.length ?? "—"}
              </span>
            </button>
          ))}
        </div>
        <label className="docker-search">
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
        <div className="docker-filters">
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
          <span>{filtered.length} 项</span>
        </div>
      </div>
      {(docker.engine.error ||
        (engineState !== "running" && status?.message) ||
        metric.error) && (
        <p className="docker-notice" role="status">
          {docker.engine.error || metric.error || status?.message}
          {metric.value && " · 保留上次列表，连接恢复后可操作。"}
        </p>
      )}
      <div className="docker-content">
        <div className="docker-list-pane" hidden={showDetails}>
          <div
            className="docker-list"
            role="listbox"
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
                  return (
                    <button
                      ref={(element) => {
                        if (element && pendingFocus.current === key) {
                          element.focus();
                          pendingFocus.current = null;
                        }
                      }}
                      id={`${id}-row-${index}`}
                      type="button"
                      role="option"
                      aria-selected={
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
                        pendingFocus.current = dockerKey(filtered[next]);
                        list.current?.scrollIntoView({
                          index: next,
                          done: () =>
                            requestAnimationFrame(() => {
                              const target = document.getElementById(
                                `${id}-row-${next}`,
                              );
                              if (target) {
                                target.focus();
                                pendingFocus.current = null;
                              }
                            }),
                        });
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
                          ? (containerStateNames[item.state] ?? item.state)
                          : item.size}
                      </span>
                    </button>
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
                {metricMessage(selectedMetric, state.connected)}
              </div>
              <dl className="docker-facts">
                <dt>ID</dt>
                <dd className="docker-id">{selection.id}</dd>
                {selected && "state" in selected ? (
                  <>
                    <dt>镜像</dt>
                    <dd>{selected.image}</dd>
                    <dt>状态</dt>
                    <dd>
                      {containerStateNames[selected.state] ?? selected.state} ·{" "}
                      {selected.status}
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
              {selection.kind === "containers" && (
                <div className="docker-actions">
                  {(["logs", "terminal"] as const).map((operation) => {
                    const result =
                        actions[operationKey(selection.id, operation)],
                      allowed =
                        canAct &&
                        (operation === "logs" ||
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
                            current.state !== "running"
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
    </section>
  );
}
