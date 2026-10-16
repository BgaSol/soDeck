import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

interface Item {
  label: string;
  run: () => void;
  danger?: boolean;
}

export function PortForwardMenu({
  label,
  children,
  items,
  disabled,
}: {
  label: string;
  children: ReactNode;
  items: Item[];
  disabled?: boolean;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  function close() {
    setOpen(false);
    trigger.current?.focus();
  }
  return (
    <>
      <button
        ref={trigger}
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        disabled={disabled}
        onClick={() => setOpen(!open)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {children}
      </button>
      {open && !disabled && (
        <Menu
          id={id}
          label={label}
          anchor={trigger.current!}
          items={items}
          close={close}
          dismiss={() => setOpen(false)}
        />
      )}
    </>
  );
}
function Menu({
  id,
  label,
  anchor,
  items,
  close,
  dismiss,
}: {
  id: string;
  label: string;
  anchor: HTMLElement;
  items: Item[];
  close: () => void;
  dismiss: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const dismissRef = useRef(dismiss);
  dismissRef.current = dismiss;
  useLayoutEffect(() => {
    const menu = ref.current!;
    const a = anchor.getBoundingClientRect(),
      b = menu.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(a.right - b.width, innerWidth - b.width - 8)),
      top:
        a.bottom + b.height + 12 <= innerHeight
          ? a.bottom + 4
          : Math.max(8, a.top - b.height - 4),
    });
    menu.querySelector<HTMLButtonElement>("button")?.focus();
    function outside(e: PointerEvent) {
      if (
        e.target instanceof Node &&
        !menu.contains(e.target) &&
        !anchor.contains(e.target)
      )
        dismissRef.current();
    }
    const dismissOnMove = (e: Event) => {
      if (e.target instanceof Node && menu.contains(e.target)) return;
      dismissRef.current();
    };
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", dismissOnMove);
    window.addEventListener("scroll", dismissOnMove, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", dismissOnMove);
      window.removeEventListener("scroll", dismissOnMove, true);
    };
  }, [anchor]);
  return createPortal(
    <div
      id={id}
      ref={ref}
      className="context-menu forward-menu"
      role="menu"
      aria-label={label}
      style={position}
      onKeyDown={(e) => {
        if (e.key === "Escape" || e.key === "Tab") {
          e.preventDefault();
          e.stopPropagation();
          close();
          return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
        e.preventDefault();
        const buttons = Array.from(
          ref.current!.querySelectorAll<HTMLButtonElement>("button"),
        );
        const index = buttons.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        const next =
          e.key === "Home"
            ? 0
            : e.key === "End"
              ? buttons.length - 1
              : (index + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) %
                buttons.length;
        buttons[next]?.focus();
      }}
    >
      {items.map((item) => (
        <button
          key={item.label}
          role="menuitem"
          tabIndex={-1}
          className={item.danger ? "danger" : undefined}
          onClick={() => {
            close();
            item.run();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}
