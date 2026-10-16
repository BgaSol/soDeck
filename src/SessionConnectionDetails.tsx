import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, ChevronRight, LockKeyhole, X } from "lucide-react";
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
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState(false);
  const [now, setNow] = useState(Date.now);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const detailsTrigger = useRef<HTMLButtonElement>(null);
  const back = useRef<HTMLButtonElement>(null);
  const [position, setPosition] = useState({ left: 8, bottom: 40 });
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
    if (open && detail && active)
      void store.loadTrafficHistory(preferences.minutes);
  }, [open, detail, active, store, preferences.minutes, state.connected]);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      if (!trigger.current || !panel.current) return;
      const rect = trigger.current.getBoundingClientRect();
      setPosition({
        left: Math.max(
          8,
          Math.min(
            rect.left,
            window.innerWidth - panel.current.offsetWidth - 8,
          ),
        ),
        bottom: Math.max(8, window.innerHeight - rect.top + 8),
      });
    };
    place();
    close.current?.focus();
    const observer = new ResizeObserver(place);
    observer.observe(panel.current!);
    // The footer moves when the workspace switches from columns to one pane,
    // even when the trigger and popover keep the same size.
    if (trigger.current?.parentElement)
      observer.observe(trigger.current.parentElement);
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
  return (
    <>
      <button
        ref={trigger}
        className="session-information-trigger quiet"
        aria-label="会话连接详情"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          setOpen((v) => !v);
          setDetail(false);
        }}
      >
        <LockKeyhole size={12} />
        <span>{status}</span>
      </button>
      {open &&
        active &&
        createPortal(
          <div
            ref={panel}
            className="session-connection-popover"
            role="dialog"
            aria-label="会话连接详情"
            style={{
              ...position,
              left: `min(${position.left}px, max(8px, calc(100vw - 428px)))`,
              maxHeight: `calc(100vh - ${position.bottom + 8}px)`,
            }}
          >
            <header>
              {detail ? (
                <button
                  ref={back}
                  className="resource-link"
                  onClick={() => {
                    setDetail(false);
                    requestAnimationFrame(() =>
                      detailsTrigger.current?.focus(),
                    );
                  }}
                >
                  <ArrowLeft size={14} />
                  返回连接详情
                </button>
              ) : (
                <strong>会话连接详情</strong>
              )}
              <button
                ref={close}
                className="icon-button"
                aria-label="关闭连接详情"
                onClick={() => dismiss()}
              >
                <X size={15} />
              </button>
            </header>
            <div className="connection-detail-content">
              {detail ? (
                <>
                  <h3>SoDeck 通信</h3>
                  <p className="resource-help">
                    本客户端与 SSH 目标之间的通信估算
                  </p>
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
                    onScopeChange={(scope) =>
                      onChange({ ...preferences, scope })
                    }
                  />
                </>
              ) : (
                <>
                  <p role="status">{progress}</p>
                  {route && <p className="connection-route">{route}</p>}
                  <p className="resource-help">编码 · UTF-8</p>
                  <div className="connection-traffic-summary">
                    <h3>SoDeck 通信</h3>
                    <dl className="resource-facts">
                      <dt>本机发送</dt>
                      <dd>{rate(traffic?.sentPerSecond)}</dd>
                      <dt>本机接收</dt>
                      <dd>{rate(traffic?.receivedPerSecond)}</dd>
                      <dt>累计发送 / 接收</dt>
                      <dd>
                        {formatMountBytes(traffic?.sentBytes)} /{" "}
                        {formatMountBytes(traffic?.receivedBytes)}
                      </dd>
                    </dl>
                    <button
                      ref={detailsTrigger}
                      className="resource-link"
                      onClick={() => {
                        setDetail(true);
                        requestAnimationFrame(() => back.current?.focus());
                      }}
                    >
                      通信详情
                      <ChevronRight size={14} />
                    </button>
                  </div>
                </>
              )}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
