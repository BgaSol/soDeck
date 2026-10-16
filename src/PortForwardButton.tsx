import { useEffect, useState } from "react";
import { Network } from "lucide-react";
import { desktop } from "./api";
import { forwardApi, type ForwardSummary } from "./portForwardApi";

export function PortForwardButton({
  serverId,
  onError,
  summary = false,
}: {
  serverId?: string;
  onError: (e: unknown) => void;
  summary?: boolean;
}) {
  const [counts, setCounts] = useState<ForwardSummary | null>(null);
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
  return (
    <button
      className="quiet"
      title="端口转发"
      aria-label="端口转发"
      disabled={!desktop}
      onClick={() => void forwardApi.open(serverId).catch(onError)}
    >
      <Network size={15} />
      <span>
        端口转发{summary && counts ? " " + counts.ready : ""}
        {counts?.attention ? " !" : ""}
      </span>
    </button>
  );
}
