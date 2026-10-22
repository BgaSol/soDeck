import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type AriaAttributes,
  type KeyboardEvent,
} from "react";
import {
  Check,
  ChevronDown,
  Search,
  Server as ServerIcon,
  X,
} from "lucide-react";
import { searchSSHServers, sshServerSummary } from "./sshServerPickerUtils";
import type { Server } from "./types";

interface PickerProps {
  id: string;
  servers: Server[];
  value: string;
  onChange: (id: string) => void;
  onBlur: () => void;
  disabled?: boolean;
  "aria-invalid"?: AriaAttributes["aria-invalid"];
  "aria-describedby"?: string;
}

export function SSHServerPicker({
  id,
  servers,
  value,
  onChange,
  onBlur,
  disabled,
  "aria-invalid": invalid,
  "aria-describedby": describedBy,
}: PickerProps) {
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const server = servers.find((item) => item.id === value);
  const summary = server
    ? sshServerSummary(
        server,
        servers.find((item) => item.id === server.jumpServerId),
      )
    : null;

  function close(restoreFocus = true) {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus({ preventScroll: true });
  }

  return (
    <div
      ref={root}
      className="ssh-server-picker"
      onBlur={(event) => {
        if (!root.current?.contains(event.relatedTarget as Node | null)) {
          setOpen(false);
          onBlur();
        }
      }}
    >
      <button
        ref={trigger}
        id={id}
        type="button"
        role="combobox"
        className="ssh-server-picker-trigger"
        aria-expanded={open && !disabled}
        aria-haspopup="listbox"
        aria-controls={open && !disabled ? `${id}-listbox` : undefined}
        aria-invalid={invalid}
        aria-describedby={
          [describedBy, summary ? `${id}-summary` : ""]
            .filter(Boolean)
            .join(" ") || undefined
        }
        disabled={disabled}
        onClick={() => setOpen(!open)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <ServerIcon size={17} aria-hidden="true" />
        <span className="ssh-server-picker-copy" id={`${id}-summary`}>
          <strong>
            {summary?.title ||
              (value ? "原 SSH 节点已不可用" : "选择 SSH 访问节点")}
          </strong>
          {summary ? (
            <>
              {summary.identity && <small>{summary.identity}</small>}
              <small className="ssh-server-picker-route" title={summary.route}>
                {summary.route}
              </small>
            </>
          ) : (
            <small>由此节点访问目标服务</small>
          )}
        </span>
        <ChevronDown size={15} aria-hidden="true" />
      </button>
      {open && !disabled && (
        <SSHServerOptions
          id={id}
          anchor={trigger.current!}
          servers={servers}
          value={value}
          onSelect={(next) => {
            onChange(next);
            close();
          }}
          close={close}
        />
      )}
    </div>
  );
}

function SSHServerOptions({
  id,
  anchor,
  servers,
  value,
  onSelect,
  close,
}: {
  id: string;
  anchor: HTMLButtonElement;
  servers: Server[];
  value: string;
  onSelect: (id: string) => void;
  close: (restoreFocus?: boolean) => void;
}) {
  const popup = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const closeRef = useRef(close);
  closeRef.current = close;
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState(value);
  const options = useMemo(
    () => searchSSHServers(servers, query),
    [servers, query],
  );
  const byId = useMemo(
    () => new Map(servers.map((server) => [server.id, server])),
    [servers],
  );
  const activeIndex = Math.max(
    0,
    options.findIndex((server) => server.id === activeId),
  );
  const optionId = (index: number) => `${id}-option-${index}`;
  const [position, setPosition] = useState({
    left: 0,
    top: 0,
    width: 0,
    maxHeight: 360,
  });

  useLayoutEffect(() => {
    const element = popup.current!;
    // Keep the popup in the modal's DOM and in the top layer, outside its scroll clipping.
    element.showPopover();
    function positionPopup() {
      const box = anchor.getBoundingClientRect();
      const below = innerHeight - box.bottom - 12;
      const above = box.top - 12;
      const upwards = below < 300 && above > below;
      const maxHeight = Math.min(360, Math.max(0, upwards ? above : below));
      const height = Math.min(element.scrollHeight, maxHeight);
      setPosition({
        left: Math.max(8, Math.min(box.left, innerWidth - box.width - 8)),
        top: upwards ? box.top - height - 4 : box.bottom + 4,
        width: Math.min(box.width, innerWidth - 16),
        maxHeight,
      });
    }
    // Measure at the trigger's width before deciding which side has room.
    element.style.width = `${Math.min(anchor.getBoundingClientRect().width, innerWidth - 16)}px`;
    positionPopup();
    search.current?.focus({ preventScroll: true });
    function outside(event: PointerEvent) {
      if (
        event.target instanceof Node &&
        !element.contains(event.target) &&
        !anchor.contains(event.target)
      )
        closeRef.current();
    }
    function scroll(event: Event) {
      if (event.target instanceof Node && element.contains(event.target))
        return;
      positionPopup();
    }
    const observer = new ResizeObserver(positionPopup);
    observer.observe(element);
    window.addEventListener("resize", positionPopup);
    window.addEventListener("scroll", scroll, true);
    document.addEventListener("pointerdown", outside, true);
    return () => {
      observer.disconnect();
      element.hidePopover();
      window.removeEventListener("resize", positionPopup);
      window.removeEventListener("scroll", scroll, true);
      document.removeEventListener("pointerdown", outside, true);
    };
  }, [anchor]);

  useLayoutEffect(() => {
    const container = list.current;
    const option = container?.children[activeIndex] as HTMLElement | undefined;
    if (!container || !option) return;
    const top = option.offsetTop;
    const bottom = top + option.offsetHeight;
    if (top < container.scrollTop) container.scrollTop = top;
    else if (bottom > container.scrollTop + container.clientHeight)
      container.scrollTop = bottom - container.clientHeight;
  }, [activeIndex, options]);

  function keyDown(event: KeyboardEvent) {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "Tab") {
      // Continue the form's normal tab order from the trigger.
      close();
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (options[activeIndex]) onSelect(options[activeIndex].id);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!options.length) return;
      const next =
        (activeIndex + (event.key === "ArrowDown" ? 1 : -1) + options.length) %
        options.length;
      setActiveId(options[next].id);
    }
  }

  return (
    <div
      ref={popup}
      popover="manual"
      className="ssh-server-picker-popover"
      style={position}
      onKeyDown={keyDown}
    >
      <div className="ssh-server-picker-search">
        <Search size={15} aria-hidden="true" />
        <input
          ref={search}
          type="search"
          aria-label="搜索 SSH 节点"
          aria-controls={`${id}-listbox`}
          aria-activedescendant={
            options.length ? optionId(activeIndex) : undefined
          }
          autoComplete="off"
          spellCheck={false}
          placeholder="搜索名称、地址、用户或跳板"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setActiveId("");
          }}
        />
        {query && (
          <button
            type="button"
            className="icon-button"
            aria-label="清除节点搜索"
            tabIndex={-1}
            onClick={() => {
              setQuery("");
              setActiveId(value);
              search.current?.focus();
            }}
          >
            <X size={13} aria-hidden="true" />
          </button>
        )}
      </div>
      <div
        ref={list}
        id={`${id}-listbox`}
        className="ssh-server-picker-list"
        role="listbox"
        aria-label="SSH 访问节点"
      >
        {options.map((server, index) => {
          const summary = sshServerSummary(
            server,
            byId.get(server.jumpServerId || ""),
          );
          return (
            <div
              key={server.id}
              id={optionId(index)}
              role="option"
              aria-selected={server.id === value}
              className={`ssh-server-picker-option${index === activeIndex ? " active" : ""}`}
              onPointerMove={() => setActiveId(server.id)}
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => onSelect(server.id)}
            >
              <span className="ssh-server-picker-copy">
                <strong title={summary.title}>{summary.title}</strong>
                {summary.identity && (
                  <small title={summary.identity}>{summary.identity}</small>
                )}
                <small
                  className="ssh-server-picker-route"
                  title={summary.route}
                >
                  {summary.route}
                </small>
              </span>
              {server.id === value && <Check size={16} aria-hidden="true" />}
            </div>
          );
        })}
      </div>
      {!options.length && (
        <div className="ssh-server-picker-empty" role="status">
          <strong>
            {servers.length ? "没有匹配的 SSH 节点" : "暂无 SSH 节点"}
          </strong>
          <span>
            {servers.length
              ? "试试其他名称、地址、用户或跳板。"
              : "请先在主窗口添加 SSH 服务器。"}
          </span>
        </div>
      )}
      <div className="ssh-server-picker-foot">
        选择能访问目标服务的最终 SSH 节点
      </div>
    </div>
  );
}
