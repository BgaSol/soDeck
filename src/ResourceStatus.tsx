import type { Metric } from "./resourceMonitoring";
import { hasOldValue, metricMessage } from "./resourcePresentation";
export function ResourceStatus({
  metric,
  connected,
}: {
  metric: Metric<unknown>;
  connected: boolean;
}) {
  return (
    <div
      className={`metric-status${hasOldValue(metric, connected) ? " is-stale" : ""}`}
    >
      <span>{metricMessage(metric, connected)}</span>
      {metric.error && <p>{metric.error}</p>}
    </div>
  );
}
