import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import {
  terminalStateLabel,
  terminalTitle,
  type TerminalTab,
} from "./workspaces";
import "./terminal-tabs.css";

export function TerminalTabs({
  tabs,
  activeId,
  disabled,
  onSelect,
  onCreate,
  onClose,
  onRename,
  onReorder,
}: {
  tabs: TerminalTab[];
  activeId: string | null;
  disabled: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onClose: (id: string) => void;
  onRename: (id: string, title: string | null) => void;
  onReorder: (ids: string[]) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const dragged = useRef<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");
  useEffect(() => {
    track.current
      ?.querySelector<HTMLButtonElement>('[aria-selected="true"]')
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId, tabs.length]);
  return (
    <div className="terminal-tabs">
      <div
        ref={track}
        role="tablist"
        aria-label="终端标签"
        className="terminal-tabs-track"
        onKeyDown={(e) => {
          if (
            (e.key === "Delete" || e.key === "Backspace") &&
            !disabled &&
            activeId
          ) {
            e.preventDefault();
            onClose(activeId);
            return;
          }
          const index = tabs.findIndex((t) => t.id === activeId);
          if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
            e.preventDefault();
            const to = index + (e.key === "ArrowLeft" ? -1 : 1);
            if (!disabled && index >= 0 && to >= 0 && to < tabs.length) {
              const ids = tabs.map((t) => t.id);
              [ids[index], ids[to]] = [ids[to], ids[index]];
              onReorder(ids);
            }
            return;
          }
          const next =
            e.key === "ArrowRight"
              ? (index + 1) % tabs.length
              : e.key === "ArrowLeft"
                ? (index - 1 + tabs.length) % tabs.length
                : e.key === "Home"
                  ? 0
                  : e.key === "End"
                    ? tabs.length - 1
                    : -1;
          if (next < 0 || disabled) return;
          e.preventDefault();
          onSelect(tabs[next].id);
          track.current
            ?.querySelector<HTMLButtonElement>(
              `[data-terminal-id="${tabs[next].id}"]`,
            )
            ?.focus();
        }}
      >
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={`terminal-tab ${activeId === tab.id ? "active" : ""}`}
            draggable={!disabled && renaming !== tab.id}
            onDragStart={(e) => {
              e.stopPropagation();
              dragged.current = tab.id;
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("application/x-sodeck-terminal", tab.id);
            }}
            onDragEnd={() => {
              dragged.current = null;
            }}
            onDragOver={(e) => {
              if (dragged.current) {
                e.preventDefault();
                e.stopPropagation();
              }
            }}
            onDrop={(e) => {
              if (!dragged.current || disabled) return;
              e.preventDefault();
              e.stopPropagation();
              if (dragged.current === tab.id) {
                dragged.current = null;
                return;
              }
              const ids = tabs
                .filter((t) => t.id !== dragged.current)
                .map((t) => t.id);
              ids.splice(Math.max(0, ids.indexOf(tab.id)), 0, dragged.current);
              dragged.current = null;
              onReorder(ids);
            }}
          >
            {renaming === tab.id ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  onRename(tab.id, name.trim() || null);
                  setRenaming(null);
                }}
              >
                <input
                  autoFocus
                  aria-label="终端名称"
                  value={name}
                  maxLength={80}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  onBlur={() => setRenaming(null)}
                />
              </form>
            ) : (
              <button
                type="button"
                role="tab"
                id={`terminal-tab-${tab.id}`}
                data-terminal-id={tab.id}
                aria-controls={`terminal-panel-${tab.id}`}
                aria-selected={activeId === tab.id}
                tabIndex={activeId === tab.id ? 0 : -1}
                disabled={disabled}
                title={`${terminalTitle(tab)} · ${terminalStateLabel(tab.state)}；双击重命名`}
                onClick={(e) => {
                  onSelect(tab.id);
                  e.currentTarget.scrollIntoView({
                    block: "nearest",
                    inline: "nearest",
                  });
                }}
                onDoubleClick={() => {
                  setName(tab.title || "");
                  setRenaming(tab.id);
                }}
                onKeyDown={(e) => {
                  if (e.key === "F2") {
                    e.preventDefault();
                    setName(tab.title || "");
                    setRenaming(tab.id);
                  }
                }}
              >
                <span className={`terminal-tab-dot ${tab.state}`} />
                {terminalTitle(tab)}
              </button>
            )}
            <button
              className="terminal-tab-close"
              aria-label={`关闭${terminalTitle(tab)}`}
              disabled={disabled}
              onClick={() => onClose(tab.id)}
            >
              <X size={12} />
            </button>
          </div>
        ))}
      </div>
      {tabs.length > 1 && (
        <select
          className="terminal-tabs-all"
          aria-label="全部终端"
          value={activeId ?? ""}
          disabled={disabled}
          onChange={(e) => onSelect(e.target.value)}
        >
          {tabs.map((t) => (
            <option key={t.id} value={t.id}>
              {terminalTitle(t)} · {terminalStateLabel(t.state)}
            </option>
          ))}
        </select>
      )}
      <button
        className="icon-button"
        aria-label="新建终端"
        title="新建终端"
        disabled={disabled}
        onClick={onCreate}
      >
        <Plus size={16} />
      </button>
    </div>
  );
}
