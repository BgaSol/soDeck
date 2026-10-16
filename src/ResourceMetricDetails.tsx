import { useMonitor } from "./MonitorProvider";
import { percent, selectInterface } from "./resourceMonitoring";
import { formatMountBytes } from "./remoteMounts";
import type { ResourcePreferences } from "./resourcePresentation";

export function ResourceMetricDetails({
  preferences,
  onChange,
  interfaceNotice,
}: {
  preferences: ResourcePreferences;
  onChange: (value: ResourcePreferences) => void;
  interfaceNotice: string;
}) {
  const { state } = useMonitor();
  const { cpu, memory, network } = state.metrics;
  const memoryValue = memory.value;
  const selectedMetric = preferences.selectedMetric ?? "cpu";
  const net = selectInterface(
    network.value ?? [],
    preferences.networkInterface,
  );
  return (
    <section className="resource-detail" aria-label="指标详情">
      {selectedMetric === "cpu" && (
        <>
          <h3>最新核心采样</h3>
          <div className="cpu-core-grid">
            {Object.entries(cpu.value?.cores ?? {}).map(([name, value]) => (
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
            ))}
          </div>
          <dl className="resource-facts">
            <dt>Load · 1 / 5 / 15 分钟</dt>
            <dd>
              {cpu.value?.load?.map((n) => n.toFixed(2)).join(" / ") ?? "—"}
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
                {selectInterface(network.value ?? [], "")?.name ?? "等待网卡"}
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
            <span className="chart-legend-primary">实线：服务器接收</span> ·{" "}
            <span className="chart-legend-secondary">虚线：服务器发送</span>
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
  );
}
