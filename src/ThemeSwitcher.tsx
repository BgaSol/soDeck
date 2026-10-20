import { createPortal } from "react-dom";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Monitor, Moon, Sun, ChevronDown, Check } from "lucide-react";
import {
  getThemePreference,
  setThemePreference,
  subscribeTheme,
  themeOptions,
  type ThemePreference,
} from "./theme";
const items = themeOptions.map(
  ([value, label]) =>
    [value, label, { system: Monitor, light: Sun, dark: Moon }[value]] as const,
);
export function ThemeSwitcher() {
  const preference = useSyncExternalStore(subscribeTheme, getThemePreference),
    [open, setOpen] = useState(false),
    [index, setIndex] = useState(0);
  const ref = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null),
    menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ top: 0, right: 0 });
  function close() {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  }
  function show() {
    const bounds = trigger.current?.getBoundingClientRect();
    if (bounds)
      setPosition({
        top: bounds.bottom + 6,
        right: Math.max(8, window.innerWidth - bounds.right),
      });
    setIndex(items.findIndex(([value]) => value === preference));
    setOpen(true);
  }
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => {
      if (
        !ref.current?.contains(e.target as Node) &&
        !menu.current?.contains(e.target as Node)
      )
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, [open]);
  useEffect(() => {
    if (open)
      menu.current
        ?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')
        [index]?.focus();
  }, [open, index]);
  const Icon = items.find(([value]) => value === preference)![2];
  return (
    <div className="theme-switcher" ref={ref}>
      <button
        ref={trigger}
        type="button"
        aria-label="外观主题"
        title="外观主题"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            show();
          }
        }}
      >
        <Icon size={15} aria-hidden="true" />
        <ChevronDown size={11} aria-hidden="true" />
      </button>
      {open &&
        createPortal(
          <div
            ref={menu}
            style={position}
            className="context-menu theme-menu"
            role="menu"
            aria-label="外观主题"
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing) return;
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                close();
              } else if (e.key === "Tab") {
                setOpen(false);
              } else if (
                ["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)
              ) {
                e.preventDefault();
                setIndex((v) =>
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? 2
                      : (v + (e.key === "ArrowDown" ? 1 : 2)) % 3,
                );
              }
            }}
          >
            {items.map(([value, label, ItemIcon], i) => (
              <button
                key={value}
                type="button"
                tabIndex={index === i ? 0 : -1}
                role="menuitemradio"
                aria-checked={preference === value}
                onClick={() => {
                  setThemePreference(value as ThemePreference);
                  close();
                }}
              >
                <ItemIcon size={14} aria-hidden="true" />
                <span>{label}</span>
                {preference === value && <Check size={14} aria-hidden="true" />}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </div>
  );
}
