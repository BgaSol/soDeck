import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Cpu,
  ExternalLink,
  MemoryStick,
  RefreshCw,
} from "lucide-react";
import { useMonitor } from "./MonitorProvider";
import { ResourceMetricDetails } from "./ResourceMetricDetails";
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
import { errorOf } from "./types";
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
export function ResourcePanel({
  serverName,
  active,
  preferences,
  onChange,
  standalone = false,
  onOpenMetric,
  onRevealMount,
  onOpenWindow,
}: {
  serverName: string;
  active: boolean;
  preferences: ResourcePreferences;
  onChange: (next: ResourcePreferences) => void;
  standalone?: boolean;
  onOpenMetric: (metric: ResourceMetric, trigger: HTMLButtonElement) => void;
  onRevealMount?: (path: string) => void;
  onOpenWindow?: () => Promise<void>;
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
  const detail = standalone || !!preferences.detailOpen;
  const [windowError, setWindowError] = useState("");
  const [opening, setOpening] = useState(false);
  const back = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (active && detail && !standalone) back.current?.focus();
  }, [active, detail, standalone, selectedMetric]);
  useEffect(() => {
    if (active) void store.loadHistory(minutes);
  }, [store, active, minutes]);
  useEffect(() => {
    setCursor(liveChartCursor);
  }, [minutes, netName, store, active, standalone, state.connected]);
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
      scroll.current.scrollTop = detail
        ? 0
        : (preferences.scrollTop?.compact ?? 0);
  }, [active, detail, selectedMetric]);
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
    standalone
      ? onChange({ ...preferences, selectedMetric: metric })
      : onOpenMetric(metric, trigger);
  const reading = chartSampleAt(state.points, cursor.at);
  const memoryValue = memory.value;
  return (
    <section
      className={`resource-panel${standalone ? " resource-window-panel" : detail ? " resource-detail-panel" : ""}`}
      aria-label="服务器资源监控"
    >
      {!standalone && detail && (
        <button
          ref={back}
          className="resource-link monitor-back"
          onClick={() => {
            onChange({ ...preferences, detailOpen: false });
            requestAnimationFrame(() =>
              scroll.current
                ?.querySelector<HTMLButtonElement>(
                  `[data-resource="${selectedMetric}"] button`,
                )
                ?.focus(),
            );
          }}
        >
          <ArrowLeft size={14} />
          返回监控
        </button>
      )}
      <header className="resource-panel-head">
        <div>
          <strong>
            {detail && !standalone
              ? { cpu: "CPU", memory: "内存", network: "服务器网络" }[
                  selectedMetric
                ]
              : serverName}
          </strong>
          <span>
            {!state.connected
              ? "已断开 · 保留最后采样"
              : metricMessage(
                  detail ? state.metrics[selectedMetric] : cpu,
                  true,
                )}
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
      {onOpenWindow && (
        <button
          className="resource-link monitor-open-window"
          disabled={opening || !state.connected}
          onClick={async () => {
            setOpening(true);
            setWindowError("");
            try {
              await onOpenWindow();
            } catch (error) {
              setWindowError(errorOf(error).message);
            } finally {
              setOpening(false);
            }
          }}
        >
          <ExternalLink size={13} />
          {opening ? "正在打开…" : "在独立窗口打开"}
        </button>
      )}
      {windowError && (
        <p className="resource-error" role="alert">
          {windowError}
        </p>
      )}
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
          !detail &&
          onChange({
            ...preferences,
            scrollTop: {
              compact: e.currentTarget.scrollTop,
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
          {(["cpu", "memory", "network"] as const)
            .filter((key) => !detail || standalone || key === selectedMetric)
            .map((key) => {
              const metric = state.metrics[key];
              const label =
                key === "cpu"
                  ? "CPU"
                  : key === "memory"
                    ? "内存"
                    : "服务器网络";
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
                      aria-pressed={
                        standalone ? selectedMetric === key : undefined
                      }
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
                      compact={!detail}
                      cursor={cursor}
                      onCursorChange={setCursor}
                    />
                  </div>
                  {(detail ||
                    metric.status !== "ready" ||
                    !state.connected) && (
                    <ResourceStatus
                      metric={metric}
                      connected={state.connected}
                    />
                  )}
                </section>
              );
            })}
        </div>
        {detail && (
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
        {state.historyLoading && detail && (
          <p className="resource-notice">正在读取已有历史…</p>
        )}
        {state.historyError && (
          <p className="resource-error">{state.historyError}</p>
        )}
        {state.historyTruncated && (
          <p className="resource-notice">历史已按内存上限裁剪。</p>
        )}
        <div className="resource-lower">
          {detail && (
            <ResourceMetricDetails
              preferences={preferences}
              onChange={onChange}
              interfaceNotice={interfaceNotice}
            />
          )}
          {!detail && onRevealMount && (
            <div className="resource-storage">
              <MountOverview
                active={active}
                preferences={preferences}
                onChange={onChange}
                onReveal={onRevealMount}
              />
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
