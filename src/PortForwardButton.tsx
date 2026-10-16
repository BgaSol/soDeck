import { useEffect, useId, useState } from "react";
import { Network } from "lucide-react";
import { desktop } from "./api";
import { forwardApi, type ForwardSummary } from "./portForwardApi";

export function PortForwardButton({
  serverId,
  onError,
  summary = false,
  iconOnly = false,
}: {
  serverId?: string;
  onError: (e: unknown) => void;
  summary?: boolean;
  iconOnly?: boolean;
}) {
  const [counts, setCounts] = useState<ForwardSummary | null>(null);
  const descriptionId = useId();
  useEffect(() => {
    if (!summary || !desktop) return;
    let disposed = false,
      timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try {
        const next = await forwardApi.summary();
        if (!disposed) setCounts(next);
      } catch {
        /* Existing controls remain usable if the summary is unavailable. */
      } finally {
        if (!disposed) timer = setTimeout(() => void refresh(), 2000);
      }
    }
    void refresh();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [summary]);
  const description = counts
    ? `${counts.ready} 条已启动${counts.attention ? ` · ${counts.attention} 条需处理` : ""}`
    : "";
  const state = counts?.attention
    ? "attention"
    : counts?.ready
      ? "ready"
      : null;
  return (
    <button
      className={`quiet forward-entry${iconOnly ? " forward-entry-icon" : ""}`}
      title={description ? `端口转发 · ${description}` : "端口转发"}
      aria-label="端口转发"
      aria-describedby={description ? descriptionId : undefined}
      disabled={!desktop}
      onClick={() => void forwardApi.open(serverId).catch(onError)}
    >
      <Network size={15} aria-hidden="true" />
      {!iconOnly && <span>端口转发</span>}
      {state && (
        <span
          className="forward-entry-indicator"
          data-state={state}
          aria-hidden="true"
        />
      )}
      {description && (
        <span id={descriptionId} className="sr-only">
          {description}
        </span>
      )}
    </button>
  );
}
