import type { InstallReview } from "./updateApi";
import { impactLabel } from "./updatePresentation";
export function UpdateInstallReview({
  review,
  pending,
  onLater,
  onRefresh,
  onInstall,
}: {
  review: InstallReview;
  pending: boolean;
  onLater: () => void;
  onRefresh: () => void;
  onInstall: () => void;
}) {
  return (
    <div className="update-review" role="group" aria-label="安装前检查">
      <strong>
        {review.blockers.length ? "安装前需要处理" : "确认安装并重启 SoDeck"}
      </strong>
      {review.blockers.length > 0 ? (
        <ul>
          {review.blockers.map((item) => (
            <li key={item.kind}>
              {impactLabel[item.kind] ?? item.kind}（{item.count} 项）
            </li>
          ))}
        </ul>
      ) : (
        <>
          <p>安装会关闭所有窗口并断开列出的会话。已保存配置会保留。</p>
          {review.interruptions.length > 0 && (
            <ul>
              {review.interruptions.map((item) => (
                <li key={item.kind}>
                  {impactLabel[item.kind] ?? item.kind}：{item.count} 项
                  {item.connections ? `，${item.connections} 条连接` : ""}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      <div className="update-actions">
        <button disabled={pending} onClick={onLater}>
          稍后
        </button>
        <button disabled={pending} onClick={onRefresh}>
          重新检查
        </button>
        {!review.blockers.length && (
          <button
            className="primary"
            disabled={pending || !review.reviewToken}
            onClick={onInstall}
          >
            确认安装并重启
          </button>
        )}
      </div>
    </div>
  );
}
