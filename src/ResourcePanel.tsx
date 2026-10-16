import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Activity,
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Boxes,
  Cpu,
  Maximize2,
  MemoryStick,
  RefreshCw,
} from "lucide-react";
import { useMonitor } from "./MonitorProvider";
import { ClientTrafficCard } from "./ClientTrafficCard";
import { DockerSummaryCard } from "./DockerSummaryCard";
import { MountOverview } from "./MountOverview";
import {
  MetricChart,
  chartSampleAt,
  liveChartCursor,
  type ChartCursor,
  type ChartLine,
} from "./MetricChart";
import { ResourceStatus } from "./ResourceStatus";
import {
  percent,
  rate,
  selectInterface,
  type HistoryPoint,
} from "./resourceMonitoring";
import { formatMountBytes } from "./remoteMounts";
import {
  hasOldValue,
  metricMessage,
  type ResourceMetric,
  type ResourcePreferences,
} from "./resourcePresentation";
export {
  defaultResourcePreferences,
  normalizeResourcePreferences,
} from "./resourcePresentation";
export type { ResourcePreferences } from "./resourcePresentation";
import "./resources.css";
function useClock(active: boolean) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => {
      if (!document.hidden) setNow(Date.now());
    }, 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}
export function ResourceStatusBar({
  networkInterface,
  active,
  onOpen,
}: {
  networkInterface: string;
  active: boolean;
  onOpen: (metric: ResourceMetric, trigger: HTMLButtonElement) => void;
}) {
  const { state } = useMonitor();
  const { cpu, memory, network } = state.metrics;
  const net = selectInterface(network.value ?? [], networkInterface);
  return (
    <div
      className="resource-statusbar"
      role="group"
      aria-label="服务器资源摘要"
      hidden={!active}
    >
      <button
        onClick={(e) => onOpen("cpu", e.currentTarget)}
        className={hasOldValue(cpu, state.connected) ? "is-stale" : ""}
        title={metricMessage(cpu, state.connected)}
        aria-label="查看 CPU 监控"
      >
        <Cpu size={12} />
        <span>CPU</span>
        <strong>{percent(cpu.value?.percent)}</strong>
      </button>
      <button
        onClick={(e) => onOpen("memory", e.currentTarget)}
        className={hasOldValue(memory, state.connected) ? "is-stale" : ""}
        title={metricMessage(memory, state.connected)}
        aria-label="查看内存监控"
      >
        <MemoryStick size={12} />
        <span>内存</span>
        <strong>{percent(memory.value?.usedPercent)}</strong>
      </button>
      <button
        onClick={(e) => onOpen("network", e.currentTarget)}
        className={`resource-network-summary${hasOldValue(network, state.connected) ? " is-stale" : ""}`}
        title={`${net?.name ?? networkInterface} · 服务器接收 / 发送 · ${metricMessage(network, state.connected)}`}
        aria-label="查看网络监控"
      >
        <ArrowDown size={12} />
        <strong>{rate(net?.rxPerSecond)}</strong>
        <ArrowUp size={12} />
        <strong>{rate(net?.txPerSecond)}</strong>
      </button>
      {!state.connected && (
        <span className="resource-status-label">最后采样</span>
      )}
    </div>
  );
}
export function ServicePanel({
  preferences,
  onChange,
  monitor,
  docker,
  expanded = false,
  onToggleExpanded,
}: {
  preferences: ResourcePreferences;
  onChange: (next: ResourcePreferences) => void;
  monitor: ReactNode;
  docker: ReactNode;
  expanded?: boolean;
  onToggleExpanded?: (trigger: HTMLButtonElement) => void;
}) {
  const id = useId();
  const tabs = ["monitor", "docker"] as const;
  return (
    <aside
      className={`resource-service-panel${expanded ? " is-expanded" : ""}`}
      aria-label="服务器监控与服务"
    >
      <div className="inspector-navigation">
        <div className="resource-tabs" role="tablist" aria-label="服务器侧栏">
          {tabs.map((tab, index) => (
            <button
              key={tab}
              type="button"
              role="tab"
              id={`${id}-${tab}`}
              aria-controls={`${id}-${tab}-panel`}
              aria-selected={preferences.tab === tab}
              tabIndex={preferences.tab === tab ? 0 : -1}
              onClick={() => onChange({ ...preferences, tab })}
              onKeyDown={(e) => {
                if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
                  return;
                e.preventDefault();
                const target =
                  e.key === "Home" ? 0 : e.key === "End" ? 1 : 1 - index;
                onChange({ ...preferences, tab: tabs[target] });
                document.getElementById(`${id}-${tabs[target]}`)?.focus();
              }}
            >
              {tab === "monitor" ? <Activity size={14} /> : <Boxes size={14} />}{" "}
              {tab === "monitor" ? "总览" : "Docker"}
            </button>
          ))}
        </div>
        {onToggleExpanded && (
          <button
            className="inspector-expand quiet"
            onClick={(e) => onToggleExpanded(e.currentTarget)}
            aria-label={expanded ? "返回终端" : "展开检查器"}
          >
            {expanded ? <ArrowLeft size={14} /> : <Maximize2 size={14} />}
            <span>{expanded ? "返回终端" : "展开"}</span>
          </button>
        )}
      </div>
      <div
        role="tabpanel"
        id={`${id}-monitor-panel`}
        aria-labelledby={`${id}-monitor`}
        hidden={preferences.tab !== "monitor"}
      >
        {monitor}
      </div>
      <div
        role="tabpanel"
        id={`${id}-docker-panel`}
        aria-labelledby={`${id}-docker`}
        hidden={preferences.tab !== "docker"}
      >
        {docker}
      </div>
    </aside>
  );
}
export function ResourcePanel({
  serverName,
  active,
  preferences,
  onChange,
  expanded = false,
  onOpenMetric,
  onRevealMount,
}: {
  serverName: string;
  active: boolean;
  preferences: ResourcePreferences;
  onChange: (next: ResourcePreferences) => void;
  expanded?: boolean;
  onOpenMetric: (metric: ResourceMetric, trigger: HTMLButtonElement) => void;
  onRevealMount: (path: string) => void;
}) {
  const { store, state } = useMonitor();
  const now = useClock(active);
  const scroll = useRef<HTMLDivElement>(null);
  const [cursor, setCursor] = useState<ChartCursor>(liveChartCursor);
  const [interfaceNotice, setInterfaceNotice] = useState("");
  const { cpu, memory, network } = state.metrics;
  const net = selectInterface(
    network.value ?? [],
    preferences.networkInterface,
  );
  const netName = net?.name ?? preferences.networkInterface;
  const lastInterface = useRef(netName);
  const { minutes } = preferences;
  const selectedMetric = preferences.selectedMetric ?? "cpu";
  useEffect(() => {
    if (active) void store.loadHistory(minutes);
  }, [store, active, minutes]);
  useEffect(() => {
    setCursor(liveChartCursor);
  }, [minutes, netName, store, active, expanded]);
  useEffect(() => {
    if (
      lastInterface.current &&
      netName &&
      lastInterface.current !== netName &&
      !preferences.networkInterface
    )
      setInterfaceNotice(`自动网卡已切换至 ${netName}`);
    else if (preferences.networkInterface) setInterfaceNotice("");
    lastInterface.current = netName;
  }, [netName, preferences.networkInterface]);
  useLayoutEffect(() => {
    if (active && scroll.current)
      scroll.current.scrollTop =
        preferences.scrollTop?.[expanded ? "expanded" : "compact"] ?? 0;
  }, [active, expanded]);
  const lines = useMemo<Record<ResourceMetric, ChartLine<HistoryPoint>[]>>(
    () => ({
      cpu: [
        {
          label: "整机 CPU",
          color: "var(--chart-primary)",
          value: (p) => p.cpu,
        },
      ],
      memory: [
        {
          label: "内存",
          color: "var(--chart-primary)",
          value: (p) => p.memory,
        },
      ],
      network: [
        {
          label: "服务器接收",
          color: "var(--chart-primary)",
          value: (p) => (netName ? p.network[netName]?.[0] : null),
        },
        {
          label: "服务器发送",
          color: "var(--chart-secondary)",
          value: (p) => (netName ? p.network[netName]?.[1] : null),
        },
      ],
    }),
    [netName],
  );
  const choose = (metric: ResourceMetric, trigger: HTMLButtonElement) =>
    expanded
      ? onChange({ ...preferences, selectedMetric: metric })
      : onOpenMetric(metric, trigger);
  const reading = chartSampleAt(state.points, cursor.at);
  const memoryValue = memory.value;
  return (
    <section
      className={`resource-panel${expanded ? " resource-expanded" : ""}`}
      aria-label="服务器资源监控"
    >
      <header className="resource-panel-head">
        <div>
          <strong>
            {expanded ? `${serverName} · 服务器总览` : serverName}
          </strong>
          <span>
            {!state.connected
              ? "已断开 · 保留最后采样"
              : metricMessage(cpu, true)}
          </span>
        </div>
        <button
          className="icon-button"
          aria-label="刷新资源监控"
          disabled={!state.connected}
          onClick={() => void store.refresh("metrics")}
        >
          <RefreshCw size={14} />
        </button>
      </header>
      <div className="resource-range" role="group" aria-label="监控时间范围">
        {([5, 15, 60] as const).map((value) => (
          <button
            key={value}
            aria-pressed={minutes === value}
            onClick={() => onChange({ ...preferences, minutes: value })}
          >
            {value === 60 ? "1 小时" : `${value} 分钟`}
          </button>
        ))}
        <details className="resource-scope">
          <summary aria-label="监控范围说明">范围</summary>
          <p>
            监控当前 SSH 环境可见的系统资源，可能属于独立命名空间，不代表容器
            cgroup 配额。历史最多保留一小时；缺少采样的时段留空。
          </p>
        </details>
      </div>
      <div
        className="resource-scroll"
        ref={scroll}
        onScroll={(e) =>
          onChange({
            ...preferences,
            scrollTop: {
              compact: preferences.scrollTop?.compact ?? 0,
              expanded: preferences.scrollTop?.expanded ?? 0,
              [expanded ? "expanded" : "compact"]: e.currentTarget.scrollTop,
            },
          })
        }
      >
        {state.error && (
          <p className="resource-error" role="status">
            {state.error}
          </p>
        )}
        <div className="resource-metrics">
          {(["cpu", "memory", "network"] as const).map((key) => {
            const metric = state.metrics[key];
            const label =
              key === "cpu" ? "CPU" : key === "memory" ? "内存" : "服务器网络";
            const note =
              key === "cpu"
                ? `整机 · ${Object.keys(cpu.value?.cores ?? {}).length || "—"} 核`
                : key === "memory"
                  ? `${formatMountBytes(memoryValue?.usedBytes)} / ${formatMountBytes(memoryValue?.totalBytes)}`
                  : preferences.networkInterface
                    ? `${netName}${net ? "" : " · 暂不可用"}`
                    : `自动 · ${netName || "等待网卡"}`;
            return (
              <section
                className={`resource-metric${selectedMetric === key ? " is-selected" : ""}`}
                key={key}
                data-resource={key}
                aria-label={`${label}资源`}
              >
                <div className="resource-metric-main">
                  <button
                    className="resource-metric-title"
                    onClick={(e) => choose(key, e.currentTarget)}
                    aria-label={`查看${label}详情`}
                    aria-pressed={expanded ? selectedMetric === key : undefined}
                  >
                    <strong>{label}</strong>
                    <span>{note}</span>
                  </button>
                  <div className="resource-value">
                    {key === "cpu" ? (
                      percent(cpu.value?.percent)
                    ) : key === "memory" ? (
                      percent(memoryValue?.usedPercent)
                    ) : (
                      <span className="network-rates">
                        <span>↓ {rate(net?.rxPerSecond)}</span>
                        <span>↑ {rate(net?.txPerSecond)}</span>
                      </span>
                    )}
                  </div>
                  <MetricChart
                    label={label}
                    points={state.points}
                    lines={lines[key]}
                    minutes={minutes}
                    now={now}
                    maximum={key === "network" ? undefined : 100}
                    format={key === "network" ? rate : percent}
                    compact={!expanded}
                    cursor={cursor}
                    onCursorChange={setCursor}
                  />
                </div>
                {(expanded ||
                  metric.status !== "ready" ||
                  !state.connected) && (
                  <ResourceStatus metric={metric} connected={state.connected} />
                )}
              </section>
            );
          })}
        </div>
        {expanded && (
          <div className="resource-reading">
            <span>
              {cursor.at === null
                ? "移动到图表查看同一时刻的采样"
                : reading
                  ? `${new Date(reading.at).toLocaleTimeString()} · CPU ${percent(reading.cpu)} · 内存 ${percent(reading.memory)}${cursor.pinned ? " · 已固定" : ""}`
                  : "此时段无采样"}
            </span>
            {cursor.end !== null && (
              <button
                className="resource-link"
                onClick={() => setCursor(liveChartCursor)}
              >
                回到实时
              </button>
            )}
          </div>
        )}
        {state.historyLoading && expanded && (
          <p className="resource-notice">正在读取已有历史…</p>
        )}
        {state.historyError && (
          <p className="resource-error">{state.historyError}</p>
        )}
        {state.historyTruncated && (
          <p className="resource-notice">历史已按内存上限裁剪。</p>
        )}
        <div className="resource-lower">
          {expanded && (
            <section className="resource-detail" aria-label="指标详情">
              {selectedMetric === "cpu" && (
                <>
                  <h3>最新核心采样</h3>
                  <div className="cpu-core-grid">
                    {Object.entries(cpu.value?.cores ?? {}).map(
                      ([name, value]) => (
                        <div key={name}>
                          <span>
                            <span>{name}</span>
                            <strong>{percent(value)}</strong>
                          </span>
                          {value != null && (
                            <span className="resource-meter">
                              <span
                                style={{
                                  width: `${Math.max(0, Math.min(100, value))}%`,
                                }}
                              />
                            </span>
                          )}
                        </div>
                      ),
                    )}
                  </div>
                  <dl className="resource-facts">
                    <dt>Load · 1 / 5 / 15 分钟</dt>
                    <dd>
                      {cpu.value?.load?.map((n) => n.toFixed(2)).join(" / ") ??
                        "—"}
                    </dd>
                  </dl>
                  <p className="resource-help">
                    核心仅展示最新采样，趋势为整机占用；Load 不是百分比。
                  </p>
                </>
              )}
              {selectedMetric === "memory" && (
                <>
                  <h3>内存详情 · 最新采样</h3>
                  <dl className="resource-facts">
                    <dt>已使用</dt>
                    <dd>{formatMountBytes(memoryValue?.usedBytes)}</dd>
                    <dt>可用内存</dt>
                    <dd>{formatMountBytes(memoryValue?.availableBytes)}</dd>
                    <dt>缓存</dt>
                    <dd>{formatMountBytes(memoryValue?.cachedBytes)}</dd>
                    <dt>Swap</dt>
                    <dd>
                      {memoryValue?.swapTotalBytes === "0"
                        ? "未配置"
                        : `${formatMountBytes(memoryValue?.swapUsedBytes)} / ${formatMountBytes(memoryValue?.swapTotalBytes)}`}
                    </dd>
                  </dl>
                  <p className="resource-help">
                    占用根据 MemAvailable 计算，缓存不与其他项直接相加。
                  </p>
                </>
              )}
              {selectedMetric === "network" && (
                <>
                  <h3>服务器网络 · 最新采样</h3>
                  <label className="resource-network-picker">
                    监控网卡
                    <select
                      aria-label="监控网卡"
                      value={preferences.networkInterface}
                      onChange={(e) =>
                        onChange({
                          ...preferences,
                          networkInterface: e.target.value,
                        })
                      }
                    >
                      <option value="">
                        自动 ·{" "}
                        {selectInterface(network.value ?? [], "")?.name ??
                          "等待网卡"}
                      </option>
                      {preferences.networkInterface &&
                        !network.value?.some(
                          (n) => n.name === preferences.networkInterface,
                        ) && (
                          <option value={preferences.networkInterface}>
                            {preferences.networkInterface} · 暂不可用
                          </option>
                        )}
                      {(network.value ?? []).map((n) => (
                        <option key={n.name} value={n.name}>
                          {n.name}
                          {n.isDefault ? " · 默认路由" : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                  {!net && preferences.networkInterface && (
                    <p className="resource-notice">
                      {preferences.networkInterface} 暂不可用；保留原有选择。
                    </p>
                  )}
                  {interfaceNotice && (
                    <p className="resource-notice" role="status">
                      {interfaceNotice}
                    </p>
                  )}
                  <p className="resource-help">
                    <span className="chart-legend-primary">
                      实线：服务器接收
                    </span>{" "}
                    ·{" "}
                    <span className="chart-legend-secondary">
                      虚线：服务器发送
                    </span>
                  </p>
                  <dl className="resource-facts">
                    <dt>累计接收 / 发送</dt>
                    <dd>
                      {formatMountBytes(net?.rxBytes)} /{" "}
                      {formatMountBytes(net?.txBytes)}
                    </dd>
                    <dt>接收错误 / 丢弃</dt>
                    <dd>
                      {net?.rxErrors ?? "—"} / {net?.rxDropped ?? "—"}
                    </dd>
                    <dt>发送错误 / 丢弃</dt>
                    <dd>
                      {net?.txErrors ?? "—"} / {net?.txDropped ?? "—"}
                    </dd>
                  </dl>
                </>
              )}
            </section>
          )}
          <div className="resource-storage">
            <DockerSummaryCard
              active={active}
              onOpen={() => onChange({ ...preferences, tab: "docker" })}
            />
            <MountOverview
              active={active}
              preferences={preferences}
              onChange={onChange}
              onReveal={onRevealMount}
            />
          </div>
        </div>
        <ClientTrafficCard
          minutes={minutes}
          now={now}
          preferences={preferences}
          onChange={onChange}
        />
      </div>
    </section>
  );
}
