import { Pause, ArrowDownToLine, Copy, X } from "lucide-react";
interface Props {
  status: string;
  action: string;
  following: boolean;
  onAction: () => void;
  count: number;
  selected: boolean;
  onCopy: () => void;
  onExport: () => void;
  onClear: () => void;
  notice: string;
  busy: boolean;
}
export function LogReadingBar(p: Props) {
  return (
    <footer className="log-reading-bar">
      <div className="log-reading-summary">
        <span>已保存 {p.count.toLocaleString()} 条</span>
        <span className="log-reading-status">{p.status}</span>
      </div>
      {p.selected && (
        <div
          className="log-selection-actions"
          role="group"
          aria-label="选中日志操作"
        >
          <button disabled={p.busy} onClick={p.onCopy}>
            <Copy size={14} />
            复制原文
          </button>
          <button disabled={p.busy} onClick={p.onExport}>
            导出所选
          </button>
          <button aria-label="取消选择" onClick={p.onClear}>
            <X size={14} />
          </button>
        </div>
      )}
      <span className="log-notice" role="status">
        {p.notice}
      </span>
      <button
        className={
          p.following ? "log-follow-control" : "log-follow-control log-resume"
        }
        onClick={p.onAction}
      >
        {p.following ? <Pause size={14} /> : <ArrowDownToLine size={14} />}
        <span>{p.action}</span>
      </button>
    </footer>
  );
}
