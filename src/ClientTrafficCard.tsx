import { useMonitor } from "./MonitorProvider";
import { MetricChart } from "./MetricChart";
import { rate } from "./resourceMonitoring";
import { formatMountBytes } from "./remoteMounts";
import type { TrafficValues } from "./serverMonitoring";
export function ClientTrafficCard({
  minutes,
  now,
  scope,
  onScopeChange,
}: {
  minutes: number;
  now: number;
  scope: "total" | "monitoring";
  onScopeChange: (value: "total" | "monitoring") => void;
}) {
  const { state } = useMonitor();
  const traffic = state.traffic;
  const values = traffic?.[scope];
  const bytes = (s?: string) => (s == null ? "—" : formatMountBytes(s));
  const groups: [string, TrafficValues | undefined][] = [
    ["监控查询", traffic?.monitoring],
    ["终端与日志", traffic?.terminalAndLogs],
    ["文件操作", traffic?.files],
    ["其他／共享", traffic?.other],
  ];
  return (
    <section className="client-traffic-card" aria-label="SoDeck 通信">
      <div className="traffic-content">
        {state.trafficHistoryLoading && (
          <p className="resource-notice">正在读取通信历史…</p>
        )}
        {state.trafficHistoryError && (
          <p className="resource-error" role="status">
            {state.trafficHistoryError}
          </p>
        )}
        <div className="traffic-scope" role="group" aria-label="通信趋势范围">
          {(
            [
              ["total", "全部通信"],
              ["monitoring", "其中：监控查询"],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              aria-pressed={scope === value}
              onClick={() => onScopeChange(value)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="traffic-directions">
          <div>
            <span>本机发出 → SSH 目标</span>
            <strong>{rate(values?.sentPerSecond)}</strong>
            <small>累计 {bytes(values?.sentBytes)}</small>
          </div>
          <div>
            <span>从 SSH 目标接收 → 本机</span>
            <strong>{rate(values?.receivedPerSecond)}</strong>
            <small>累计 {bytes(values?.receivedBytes)}</small>
          </div>
        </div>
        <MetricChart
          label={scope === "total" ? "SoDeck 通信" : "监控查询通信"}
          points={state.trafficPoints}
          minutes={minutes}
          now={now}
          format={rate}
          lines={[
            {
              label: "本机发出",
              color: "var(--chart-primary)",
              value: (p) => (scope === "total" ? p.sent : p.monitorSent),
            },
            {
              label: "本机接收",
              color: "var(--chart-secondary)",
              value: (p) =>
                scope === "total" ? p.received : p.monitorReceived,
            },
          ]}
        />
        <div className="metric-status">
          <span>
            {!traffic
              ? "正在建立通信统计"
              : `${traffic.activeConnections} 条目标连接 · ${!state.connected ? "已断开 · 最后数据" : traffic.status === "unavailable" ? "最后数据" : "更新于"} ${new Date(traffic.sampledAt).toLocaleTimeString()}`}
          </span>
        </div>
        <details>
          <summary>通信用途明细</summary>
          <div className="traffic-breakdown">
            {groups.map(([name, v]) => (
              <div key={name}>
                <strong>{name}</strong>
                <span>
                  发送 {rate(v?.sentPerSecond)} · 接收{" "}
                  {rate(v?.receivedPerSecond)}
                </span>
                <small>
                  累计发送 {bytes(v?.sentBytes)} · 接收{" "}
                  {bytes(v?.receivedBytes)}
                </small>
              </div>
            ))}
          </div>
        </details>
        <details>
          <summary>统计范围与口径</summary>
          <p className="resource-help">
            统计此连接身份在所有窗口及后台任务中的 SSH
            通信，包含协议开销。监控查询包含资源、磁盘和 Docker 采集及手动刷新。
          </p>
          <p className="resource-help">
            不含 TCP/IP 包头、重传、服务器直传的文件主体及远端 Docker
            后续链路；跳板外层不重复累计。
          </p>
          {traffic && (
            <p className="resource-help">
              本次运行累计 · 自 {new Date(traffic.startedAt).toLocaleString()}
              ；退出应用后重置。
            </p>
          )}
        </details>
        {state.trafficHistoryTruncated && (
          <p className="resource-notice">
            通信历史已按内存上限裁剪，累计流量保留。
          </p>
        )}
      </div>
    </section>
  );
}
