import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { HardDrive, X } from "lucide-react";
import {
  formatMountBytes,
  mountKind,
  mountSource,
  usageSeverity,
} from "./remoteMounts";
import type { CachedMountUsage, RemoteMount } from "./remoteMounts";

export function MountDetailsPopover({
  mount,
  usage,
  anchor,
  onClose,
  onRefresh,
  onReveal,
}: {
  mount: RemoteMount;
  usage?: CachedMountUsage;
  anchor: HTMLElement;
  onClose: () => void;
  onRefresh: () => void;
  onReveal?: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const dismiss = useRef(onClose);
  dismiss.current = onClose;
  const [position, setPosition] = useState({ left: 8, top: 8 });
  useLayoutEffect(() => {
    const place = () => {
      if (!dialog.current || !anchor.isConnected) {
        dismiss.current();
        return;
      }
      const rect = anchor.getBoundingClientRect();
      const bounds = dialog.current.getBoundingClientRect();
      setPosition({
        left: Math.max(
          8,
          Math.min(rect.left, window.innerWidth - bounds.width - 8),
        ),
        top: Math.max(
          8,
          Math.min(rect.bottom + 6, window.innerHeight - bounds.height - 8),
        ),
      });
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(dialog.current!);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [anchor]);
  useLayoutEffect(() => {
    close.current?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        dismiss.current();
        if (anchor.isConnected) anchor.focus();
      }
    };
    const outside = (event: PointerEvent) => {
      if (
        !dialog.current?.contains(event.target as Node) &&
        !anchor.contains(event.target as Node)
      )
        dismiss.current();
    };
    document.addEventListener("keydown", key, true);
    document.addEventListener("pointerdown", outside);
    return () => {
      document.removeEventListener("keydown", key, true);
      document.removeEventListener("pointerdown", outside);
    };
  }, [anchor]);
  const value = usage?.value;
  const severity = usageSeverity(value?.usedPercent);
  return createPortal(
    <div
      className={`mount-popover mount-${severity}`}
      ref={dialog}
      role="dialog"
      aria-label="挂载详情"
      style={position}
    >
      <div className="mount-popover-head">
        <HardDrive size={16} />
        <strong>挂载详情</strong>
        <button
          ref={close}
          className="icon-button"
          aria-label="关闭挂载详情"
          onClick={() => {
            onClose();
            if (anchor.isConnected) anchor.focus();
          }}
        >
          <X size={16} />
        </button>
      </div>
      <dl>
        <dt>挂载到</dt>
        <dd>{mount.target}</dd>
        <dt>来源</dt>
        <dd>{mountSource(mount)}</dd>
        <dt>文件系统</dt>
        <dd>
          {mount.fsType}
          {mountKind(mount) === "memory"
            ? " · 内存文件系统"
            : mountKind(mount) === "network"
              ? " · 网络文件系统"
              : ""}
        </dd>
        <dt>访问方式</dt>
        <dd>{mount.readOnly ? "只读" : "读写"}</dd>
      </dl>
      {value?.status === "ready" ? (
        <div className="mount-capacity">
          <div className="mount-capacity-line">
            <span>已用 {formatMountBytes(value.usedBytes)}</span>
            <span>
              {value.usedPercent == null
                ? "使用率不可用"
                : `${value.usedPercent}%`}
            </span>
          </div>
          {value.usedPercent != null && (
            <div
              className="mount-meter"
              role="meter"
              aria-label="文件系统使用率"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={value.usedPercent}
            >
              <span style={{ width: `${value.usedPercent}%` }} />
            </div>
          )}
          <div className="mount-capacity-line">
            <span>总计 {formatMountBytes(value.totalBytes)}</span>
            <span>可用 {formatMountBytes(value.availableBytes)}</span>
          </div>
          {severity !== "normal" && (
            <p className="mount-warning-text">
              {severity === "critical" ? "空间即将用尽" : "空间紧张"}
            </p>
          )}
        </div>
      ) : (
        <p className="mount-unavailable">
          {usage?.loading
            ? "正在读取容量…"
            : value?.status === "unsupported"
              ? value.error || "此文件系统不提供常规容量"
              : "容量暂不可用"}
        </p>
      )}
      <div className="mount-update" role="status">
        {usage?.stale ? "上次数据 · " : ""}
        {value?.updatedAt
          ? `更新于 ${new Date(value.updatedAt).toLocaleTimeString()}`
          : "尚无容量数据"}
        {usage?.loading && value ? " · 更新中…" : ""}
      </div>
      {usage?.error && <p className="mount-query-error">{usage.error}</p>}
      {onReveal && (
        <button className="quiet mount-reveal" onClick={onReveal}>
          在远程文件中定位
        </button>
      )}
      <button
        className="quiet mount-refresh"
        disabled={usage?.loading}
        onClick={onRefresh}
      >
        刷新挂载信息
      </button>
    </div>,
    document.body,
  );
}
