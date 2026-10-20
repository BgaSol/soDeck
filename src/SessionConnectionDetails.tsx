import { monitorLive } from "./resourceMonitoring";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, ChevronUp, LockKeyhole, X } from "lucide-react";
import { useMonitor } from "./MonitorProvider";
import { ClientTrafficCard } from "./ClientTrafficCard";
import { rate } from "./resourceMonitoring";
import { formatMountBytes } from "./remoteMounts";

export interface ConnectionPreferences {
  minutes: 5 | 15 | 60;
  scope: "total" | "monitoring";
}

export function SessionConnectionDetails({
  active,
  status,
  progress,
  route,
  preferences,
  onChange,
}: {
  active: boolean;
  status: string;
  progress: string;
  route: string;
  preferences: ConnectionPreferences;
  onChange: (value: ConnectionPreferences) => void;
}) {
  const { store, state } = useMonitor();
  const id = useId();
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(Date.now);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const [position, setPosition] = useState({ left: 8, bottom: 40, width: 420 });
  const dismiss = (restore = true) => {
    setOpen(false);
    if (restore) trigger.current?.focus();
  };
  useEffect(() => {
    if (!active) setOpen(false);
  }, [active]);
  useEffect(() => {
    if (!open || !active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [open, active]);
  useEffect(() => {
    if (open && active) void store.loadTrafficHistory(preferences.minutes);
  }, [open, active, store, preferences.minutes, monitorLive(state)]);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      if (!trigger.current || !panel.current) return;
      const rect = trigger.current.getBoundingClientRect();
      const footer = trigger.current.closest("footer")!.getBoundingClientRect();
      const width = Math.min(420, footer.width - 16, window.innerWidth - 16);
      setPosition({
        left: Math.max(
          8,
          footer.left + 8,
          Math.min(rect.right - width, window.innerWidth - width - 8),
        ),
        bottom: Math.max(8, window.innerHeight - footer.top + 8),
        width,
      });
    };
    place();
    close.current?.focus();
    const observer = new ResizeObserver(place);
    observer.observe(panel.current!);
    // The footer moves when the workspace switches from columns to one pane,
    // even when the trigger and popover keep the same size.
    const footer = trigger.current?.closest("footer");
    if (footer) observer.observe(footer);
    const outside = (event: PointerEvent) => {
      if (
        !panel.current?.contains(event.target as Node) &&
        !trigger.current?.contains(event.target as Node)
      )
        dismiss(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        dismiss();
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", key, true);
    window.addEventListener("resize", place);
    return () => {
      observer.disconnect();
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("resize", place);
    };
  }, [open]);
  const traffic = state.traffic?.total;
  const stale =
    !!traffic &&
    (!monitorLive(state) || state.traffic?.status === "unavailable");
  const routeParts = route.split(" → ");
  const destination = routeParts.pop();
  return (
    <>
      <div
        className="session-connection-summary"
        role="group"
        aria-label="会话连接摘要"
      >
        <span className="session-connection-state" title={progress}>
          <LockKeyhole size={12} aria-hidden="true" />
          <span>{status}</span>
        </span>
        {status === "已连接" && route ? (
          <span
            className="session-connection-route"
            tabIndex={0}
            title={route}
            aria-label={`连接路径：${route}`}
          >
            {routeParts.length > 0 && (
              <span className="connection-route-prefix">
                {routeParts.join(" → ")}
              </span>
            )}
            <span className="connection-route-target">
              {routeParts.length > 0 ? " → " : ""}
              {destination}
            </span>
          </span>
        ) : (
          <span className="session-connection-progress" role="status">
            {progress}
          </span>
        )}
        <span className="session-connection-encoding" title="终端编码：UTF-8">
          UTF-8
        </span>
      </div>
      <div
        className="connection-traffic-summary"
        role="group"
        aria-label="本机通信摘要"
      >
        <div className={`connection-traffic-values${stale ? " is-stale" : ""}`}>
          <span className="connection-traffic-pair">
            <span>
              本机通信
              {stale && (
                <span className="connection-traffic-stale"> · 最后数据</span>
              )}
            </span>
            <span aria-label={`本机发送 ${rate(traffic?.sentPerSecond)}`}>
              ↑ {rate(traffic?.sentPerSecond)}
            </span>
            <span aria-label={`本机接收 ${rate(traffic?.receivedPerSecond)}`}>
              ↓ {rate(traffic?.receivedPerSecond)}
            </span>
          </span>
          <span className="connection-traffic-pair">
            <span>累计</span>
            <span
              aria-label={`累计发送 ${formatMountBytes(traffic?.sentBytes)}`}
            >
              ↑ {formatMountBytes(traffic?.sentBytes)}
            </span>
            <span
              aria-label={`累计接收 ${formatMountBytes(traffic?.receivedBytes)}`}
            >
              ↓ {formatMountBytes(traffic?.receivedBytes)}
            </span>
          </span>
        </div>
        <button
          ref={trigger}
          className="session-information-trigger quiet"
          aria-label="通信详情"
          aria-haspopup="dialog"
          aria-controls={open ? id : undefined}
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          通信详情
          {open ? <ChevronDown size={12} /> : <ChevronUp size={12} />}
        </button>
      </div>
      {open &&
        active &&
        createPortal(
          <div
            ref={panel}
            id={id}
            className="session-connection-popover"
            role="dialog"
            aria-label="通信详情"
            style={{
              ...position,
              maxHeight: `calc(100vh - ${position.bottom + 8}px)`,
            }}
          >
            <header>
              <strong>通信详情</strong>
              <button
                ref={close}
                className="icon-button"
                aria-label="关闭通信详情"
                onClick={() => dismiss()}
              >
                <X size={15} />
              </button>
            </header>
            <div className="connection-detail-content">
              <p className="resource-help">本客户端与 SSH 目标之间的通信估算</p>
              <div
                className="resource-range"
                role="group"
                aria-label="通信时间范围"
              >
                {([5, 15, 60] as const).map((minutes) => (
                  <button
                    key={minutes}
                    aria-pressed={preferences.minutes === minutes}
                    onClick={() => onChange({ ...preferences, minutes })}
                  >
                    {minutes === 60 ? "1 小时" : `${minutes} 分钟`}
                  </button>
                ))}
              </div>
              <ClientTrafficCard
                minutes={preferences.minutes}
                now={now}
                scope={preferences.scope}
                onScopeChange={(scope) => onChange({ ...preferences, scope })}
              />
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
