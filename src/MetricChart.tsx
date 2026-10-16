import { useId, useMemo, useState } from "react";
import type { HistoryPoint } from "./resourceMonitoring";
export interface ChartPoint {
  sequence: number;
  at: number;
  gap: boolean;
}
export interface ChartLine<T extends ChartPoint = HistoryPoint> {
  label: string;
  color: string;
  value: (point: T) => number | null | undefined;
}
export interface ChartCursor {
  at: number | null;
  end: number | null;
  pinned: boolean;
}
export const liveChartCursor: ChartCursor = {
  at: null,
  end: null,
  pinned: false,
};
export function chartPath<T extends ChartPoint>(
  points: T[],
  value: ChartLine<T>["value"],
  from: number,
  to: number,
  max: number,
) {
  if (to <= from || max <= 0) return "";
  let path = "",
    drawing = false,
    previous = 0;
  for (const point of points) {
    const y = value(point);
    if (point.at < from || point.at > to || y == null || !Number.isFinite(y)) {
      drawing = false;
      continue;
    }
    const x = ((point.at - from) / (to - from)) * 300;
    const py = 80 - Math.max(0, Math.min(1, y / max)) * 76;
    if (point.gap || point.at - previous > 30_000) drawing = false;
    path += `${drawing ? "L" : "M"}${x.toFixed(2)},${py.toFixed(2)} `;
    drawing = true;
    previous = point.at;
  }
  return path.trim();
}
// A cursor inside a missing interval must not borrow a value from either edge.
export function chartSampleAt<T extends ChartPoint>(
  points: T[],
  at: number | null,
): T | undefined {
  if (
    at == null ||
    !points.length ||
    at < points[0].at ||
    at > points[points.length - 1].at
  )
    return;
  let lo = 0,
    hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (points[mid].at < at) lo = mid + 1;
    else hi = mid;
  }
  const next = points[lo];
  if (next.at === at || lo === 0) return next;
  const previous = points[lo - 1];
  if (next.gap || next.at - previous.at > 30_000) return;
  return at - previous.at <= next.at - at ? previous : next;
}
export function MetricChart<T extends ChartPoint>({
  label,
  points,
  lines,
  minutes,
  now,
  maximum,
  format,
  compact = false,
  cursor: controlled,
  onCursorChange,
}: {
  label: string;
  points: T[];
  lines: ChartLine<T>[];
  minutes: number;
  now: number;
  maximum?: number;
  format: (value: number | null | undefined) => string;
  compact?: boolean;
  cursor?: ChartCursor;
  onCursorChange?: (cursor: ChartCursor) => void;
}) {
  const id = useId();
  const [local, setLocal] = useState<ChartCursor>(liveChartCursor);
  const cursor = controlled ?? local;
  const change = onCursorChange ?? setLocal;
  const end = compact ? now : (cursor.end ?? now);
  const from = end - minutes * 60_000;
  const visible = useMemo(
    () => points.filter((p) => p.at >= from && p.at <= end),
    [points, from, end],
  );
  const selected = compact ? undefined : chartSampleAt(visible, cursor.at);
  const max =
    maximum ??
    Math.max(1, ...visible.flatMap((p) => lines.map((l) => l.value(p) ?? 0))) *
      1.1;
  const hasData = visible.some((p) => lines.some((l) => l.value(p) != null));
  const pointAt = (element: SVGSVGElement, clientX: number) => {
    const r = element.getBoundingClientRect();
    const currentEnd = cursor.end ?? now;
    return (
      currentEnd -
      minutes * 60_000 +
      Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * minutes * 60_000
    );
  };
  return (
    <div className={`metric-chart${compact ? " metric-sparkline" : ""}`}>
      {!compact && (
        <div className="metric-chart-scale" aria-label={`${label}纵轴刻度`}>
          <span>{format(max)}</span>
          <span>{format(max / 2)}</span>
          <span>{format(0)}</span>
        </div>
      )}
      {!compact && !hasData && (
        <span className="metric-chart-empty">尚无采样</span>
      )}
      <svg
        viewBox="0 0 300 88"
        preserveAspectRatio="none"
        tabIndex={compact ? undefined : 0}
        role="img"
        aria-label={`${label}趋势${compact ? "" : "，左右方向键查看采样值，回车固定，Escape 回到实时"}`}
        aria-describedby={compact ? undefined : id}
        onPointerMove={
          compact
            ? undefined
            : (e) => {
                if (!cursor.pinned)
                  change({
                    at: pointAt(e.currentTarget, e.clientX),
                    end: cursor.end ?? now,
                    pinned: false,
                  });
              }
        }
        onPointerLeave={
          compact
            ? undefined
            : () => {
                if (!cursor.pinned) change(liveChartCursor);
              }
        }
        onBlur={
          compact
            ? undefined
            : () => {
                if (!cursor.pinned) change(liveChartCursor);
              }
        }
        onClick={
          compact
            ? undefined
            : (e) =>
                change({
                  at: pointAt(e.currentTarget, e.clientX),
                  end: cursor.end ?? now,
                  pinned: !cursor.pinned,
                })
        }
        onKeyDown={
          compact
            ? undefined
            : (e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  change(liveChartCursor);
                  return;
                }
                if (
                  ![
                    "ArrowLeft",
                    "ArrowRight",
                    "Home",
                    "End",
                    "Enter",
                    " ",
                  ].includes(e.key) ||
                  !visible.length
                )
                  return;
                e.preventDefault();
                const i = selected
                  ? visible.indexOf(selected)
                  : visible.length - 1;
                const target =
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? visible.length - 1
                      : e.key === "ArrowLeft"
                        ? Math.max(0, i - 1)
                        : e.key === "ArrowRight"
                          ? Math.min(visible.length - 1, i + 1)
                          : i;
                change({
                  at: visible[target].at,
                  end: cursor.end ?? now,
                  pinned: true,
                });
              }
        }
      >
        {[4, 42, 80].map((y) => (
          <line
            key={y}
            x1="0"
            x2="300"
            y1={y}
            y2={y}
            className="metric-grid"
            aria-hidden="true"
          />
        ))}
        {lines.map((line, i) => (
          <path
            key={line.label}
            d={chartPath(visible, line.value, from, end, max)}
            fill="none"
            stroke={line.color}
            strokeWidth="1.8"
            strokeDasharray={i ? "5 3" : undefined}
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {selected && (
          <line
            x1={((selected.at - from) / (end - from)) * 300}
            x2={((selected.at - from) / (end - from)) * 300}
            y1="0"
            y2="84"
            className="metric-cursor"
          />
        )}
      </svg>
      {!compact && (
        <div className="metric-chart-caption" id={id}>
          {cursor.at !== null ? (
            <>
              <span>
                {new Date(selected?.at ?? cursor.at).toLocaleTimeString()}
              </span>
              <span>
                {selected
                  ? lines
                      .map((l) => `${l.label} ${format(l.value(selected))}`)
                      .join(" · ")
                  : "此时段无采样"}
              </span>
            </>
          ) : (
            <>
              <span>
                {new Date(from).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
              <span>
                {new Date(end).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
            </>
          )}
        </div>
      )}
      {!compact && !controlled && cursor.pinned && (
        <button
          className="resource-link"
          onClick={() => change(liveChartCursor)}
        >
          回到实时
        </button>
      )}
    </div>
  );
}
