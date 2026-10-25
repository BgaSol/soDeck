import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { X } from "lucide-react";

export function Dialog({ title, subtitle, onClose, children, wide = false, dismissOnOutside = false, className = "" }: { title: string; subtitle?: string; onClose(): void; children: ReactNode; wide?: boolean; dismissOnOutside?: boolean; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    (ref.current?.querySelector<HTMLElement>("input, select") ?? ref.current?.querySelector<HTMLElement>("button"))?.focus();
    return () => { previous?.focus(); };
  }, []);
  return <div className="dialog-backdrop" onPointerDown={event => { if (dismissOnOutside && event.target === event.currentTarget) onClose(); }}>
    <div ref={ref} className={`dialog ${wide ? "wide" : ""} ${className}`} role="dialog" aria-modal="true" aria-labelledby="dialog-title" onKeyDown={event => {
      if (event.key === "Escape" && !event.nativeEvent.isComposing) { event.stopPropagation(); onClose(); }
      if (event.key === "Tab") {
        const focusable = Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),[tabindex="0"]') ?? []);
        const first = focusable[0], last = focusable.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    }}><header><div><h2 id="dialog-title">{title}</h2>{subtitle && <p>{subtitle}</p>}</div><button className="icon-button" aria-label="关闭对话框" onClick={onClose}><X size={18} /></button></header>{children}</div>
  </div>;
}
