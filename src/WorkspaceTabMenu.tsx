import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export interface TabMenuItem {
  id: string;
  label: string;
  detail?: string;
  selected?: boolean;
  disabled?: boolean;
  info?: boolean;
  groupBefore?: boolean;
  danger?: boolean;
  run: () => void;
}

export function WorkspaceTabMenu({
  id,
  title,
  anchor,
  x,
  y,
  items,
  onClose,
}: {
  id: string;
  title: string;
  anchor: HTMLElement;
  x: number;
  y: number;
  items: TabMenuItem[];
  onClose: (restoreFocus: boolean) => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  const [position, setPosition] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const element = menu.current!;
    const bounds = element.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(x, innerWidth - bounds.width - 8)),
      top: Math.max(8, Math.min(y, innerHeight - bounds.height - 8)),
    });
    (
      element.querySelector<HTMLElement>(
        '[aria-checked="true"]:not(:disabled)',
      ) ?? element.querySelector<HTMLElement>("button:not(:disabled)")
    )?.focus();
    const outside = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !element.contains(event.target) &&
        !anchor.contains(event.target)
      )
        close.current(false);
    };
    const resize = () => close.current(true);
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", resize);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", resize);
    };
  }, [x, y, anchor]);
  return createPortal(
    <div
      ref={menu}
      id={id}
      className="context-menu workspace-tab-menu"
      role="menu"
      aria-label={title}
      style={position}
      onKeyDown={(event) => {
        if (event.key === "Escape" || event.key === "Tab") {
          event.preventDefault();
          event.stopPropagation();
          close.current(true);
          return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key))
          return;
        event.preventDefault();
        const enabled = Array.from(
          menu.current!.querySelectorAll<HTMLButtonElement>(
            "button:not(:disabled)",
          ),
        );
        const index = enabled.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? enabled.length - 1
              : (index +
                  (event.key === "ArrowDown" ? 1 : -1) +
                  enabled.length) %
                enabled.length;
        enabled[next]?.focus();
      }}
    >
      <div className="tab-menu-heading" role="presentation">
        {title}
      </div>
      {items.map((item) =>
        item.info ? (
          <div key={item.id} className="tab-menu-info" role="presentation">
            {item.label}
            {item.detail && <span>{item.detail}</span>}
          </div>
        ) : (
          <button
            key={item.id}
            className={`${item.groupBefore ? "tab-menu-group" : ""}${item.danger ? " tab-menu-danger" : ""}`}
            role={item.selected === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.selected}
            disabled={item.disabled}
            title={item.detail}
            tabIndex={-1}
            onClick={() => {
              close.current(false);
              item.run();
            }}
          >
            <span className="tab-menu-name">{item.label}</span>
            {item.detail && (
              <span className="tab-menu-detail">{item.detail}</span>
            )}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
