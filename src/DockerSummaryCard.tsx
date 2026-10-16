import { Boxes, ChevronRight } from "lucide-react";
import { useMonitor, useDockerInterest } from "./MonitorProvider";
import { metricMessage } from "./resourcePresentation";
export const dockerStatusNames: Record<string, string> = {
  running: "可访问",
  missing: "未找到 Docker 命令",
  permission: "权限不足",
  unreachable: "服务不可达",
  error: "检测失败",
  disconnected: "未连接",
  detecting: "检测中",
};
export function DockerSummaryCard({
  onOpen,
  active,
}: {
  onOpen: () => void;
  active: boolean;
}) {
  const { state } = useMonitor();
  useDockerInterest({ summary: active, containers: false, images: false });
  const { engine, summary } = state.docker;
  const healthyEngine = state.connected && engine.value?.state === "running";
  const counts = summary.value;
  const current = healthyEngine && summary.status === "ready";
  const status = !state.connected
    ? "已断开"
    : (dockerStatusNames[engine.value?.state ?? "detecting"] ?? "检测失败");
  return (
    <section className="docker-summary" aria-label="Docker 摘要">
      <button
        className="docker-summary-button"
        onClick={onOpen}
        aria-label="查看 Docker 详情"
      >
        <span>
          <Boxes size={15} />
          <strong>Docker</strong>
        </span>
        <span className="docker-summary-counts">
          {counts && current ? (
            <>
              <span>{counts.total} 个</span>
              {counts.total > 0 && (
                <span>{counts.byState.running ?? 0} 运行</span>
              )}
              {!!counts.byState.restarting && (
                <span className="warning">
                  {counts.byState.restarting} 重启中
                </span>
              )}
              {!!counts.byState.exited && (
                <span>{counts.byState.exited} 停止</span>
              )}
              {!!counts.unhealthy && (
                <span className="danger">{counts.unhealthy} 健康异常</span>
              )}
              {counts.total === 0 && <span>尚无容器</span>}
            </>
          ) : (
            <span>
              {status}
              {counts ? ` · 上次 ${counts.total} 个容器` : ""}
            </span>
          )}
          <ChevronRight size={13} />
        </span>
      </button>
      {!current && (
        <span className="metric-status">
          {metricMessage(summary, state.connected)}
        </span>
      )}
    </section>
  );
}
