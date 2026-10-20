import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Search,
  SlidersHorizontal,
  Settings2,
  Download,
  X,
} from "lucide-react";
import type { LogPreferences } from "./preferences";
import type { LogBoot } from "../logApi";
export interface ToolbarProps {
  searchOpen: boolean;
  setSearchOpen: (v: boolean) => void;
  draft: string;
  setDraft: (v: string) => void;
  caseSensitive: boolean;
  setCaseSensitive: (v: boolean) => void;
  liveSearch: boolean;
  setLiveSearch: (v: boolean) => void;
  stream: string;
  setStream: (v: string) => void;
  from: string;
  setFrom: (v: string) => void;
  to: string;
  setTo: (v: string) => void;
  clear: () => void;
  preferences: LogPreferences;
  setPreferences: (v: Partial<LogPreferences>) => void;
  exporting: boolean;
  records: boolean;
  queryReady: boolean;
  filtered: boolean;
  selected: boolean;
  onExport: (scope: "all" | "query" | "selection") => void;
  onRange: () => void;
  boots: LogBoot[];
  boot: string;
  onBoot: (id: string) => void;
}
export function LogToolbar(p: ToolbarProps) {
  const [menu, setMenu] = useState<string | null>(null),
    bar = useRef<HTMLDivElement>(null),
    input = useRef<HTMLInputElement>(null),
    trigger = useRef<HTMLButtonElement | null>(null);
  function close() {
    setMenu(null);
    trigger.current?.focus();
  }
  useEffect(() => {
    if (p.searchOpen) input.current?.focus();
  }, [p.searchOpen]);
  useEffect(() => {
    if (!menu) return;
    const down = (e: PointerEvent) => {
      if (!bar.current?.contains(e.target as Node)) setMenu(null);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener("pointerdown", down);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("keydown", key, true);
    };
  }, [menu]);
  function tool(
    id: string,
    label: string,
    icon: ReactNode,
    content: ReactNode,
  ) {
    return (
      <div className="log-tool-anchor">
        <button
          aria-expanded={menu === id}
          aria-controls={"log-popover-" + id}
          onClick={(e) => {
            trigger.current = e.currentTarget;
            setMenu(menu === id ? null : id);
          }}
        >
          {icon}
          <span>{label}</span>
        </button>
        {menu === id && (
          <div
            id={"log-popover-" + id}
            className="log-popover"
            role="region"
            aria-label={label}
          >
            {content}
          </div>
        )}
      </div>
    );
  }
  const filters = !!(p.draft || p.stream || p.from || p.to);
  return (
    <div className="log-toolbar-shell" ref={bar}>
      <div className="log-tools">
        <button
          aria-expanded={p.searchOpen}
          onClick={() => p.setSearchOpen(!p.searchOpen)}
        >
          <Search size={15} />
          查找
        </button>
        {tool(
          "filter",
          "筛选",
          <SlidersHorizontal size={15} />,
          <>
            <label>
              输出流
              <select
                aria-label="日志输出流"
                value={p.stream}
                onChange={(e) => p.setStream(e.target.value)}
              >
                <option value="">全部输出流</option>
                <option value="stdout">stdout</option>
                <option value="stderr">stderr</option>
              </select>
            </label>
            <label>
              从
              <input
                aria-label="日志开始时间"
                type="datetime-local"
                value={p.from}
                onChange={(e) => p.setFrom(e.target.value)}
              />
            </label>
            <label>
              至
              <input
                aria-label="日志结束时间"
                type="datetime-local"
                value={p.to}
                onChange={(e) => p.setTo(e.target.value)}
              />
            </label>
            <small>
              本地时区 · {Intl.DateTimeFormat().resolvedOptions().timeZone}
            </small>
            {p.boots.length > 0 && (
              <label>
                启动周期
                <select
                  aria-label="启动周期"
                  value={p.boot}
                  onChange={(e) => p.onBoot(e.target.value)}
                >
                  <option value="">全部已保存</option>
                  {p.boots.map((b, i) => (
                    <option
                      value={b.id}
                      key={b.id}
                      disabled={i > 0 && !b.finishedAt}
                    >
                      {i === 0
                        ? "最近观测的启动"
                        : b.finishedAt
                          ? "已观测启动"
                          : "结束边界未知"}{" "}
                      · {new Date(b.startedAt).toLocaleString()}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <small>无法确认启动周期的历史仍可在“全部已保存”中查看。</small>
          </>,
        )}
        <span className="log-tool-spacer" />
        {tool(
          "display",
          "显示",
          <Settings2 size={15} />,
          <>
            <label>
              <input
                type="checkbox"
                checked={p.preferences.timestamps}
                onChange={(e) =>
                  p.setPreferences({ timestamps: e.target.checked })
                }
              />
              时间戳
            </label>
            <label>
              <input
                type="checkbox"
                checked={p.preferences.wrap}
                onChange={(e) => p.setPreferences({ wrap: e.target.checked })}
              />
              自动换行
            </label>
            <label>
              <input
                type="checkbox"
                checked={p.preferences.highlight}
                onChange={(e) =>
                  p.setPreferences({ highlight: e.target.checked })
                }
              />
              高亮
            </label>
            <label>
              字号
              <input
                aria-label="日志字号"
                type="range"
                min="11"
                max="22"
                value={p.preferences.fontSize}
                onChange={(e) =>
                  p.setPreferences({ fontSize: Number(e.target.value) })
                }
              />
              <span>{p.preferences.fontSize}</span>
            </label>
            <button
              onClick={() => {
                close();
                p.onRange();
              }}
            >
              按序号选择…
            </button>
          </>,
        )}
        {tool(
          "export",
          "导出",
          <Download size={15} />,
          <>
            <button
              disabled={!p.records || p.exporting}
              onClick={() => {
                close();
                p.onExport("all");
              }}
            >
              全部已保存
            </button>
            <button
              disabled={!p.filtered || !p.queryReady || p.exporting}
              onClick={() => {
                close();
                p.onExport("query");
              }}
            >
              当前筛选结果
            </button>
            <button
              disabled={!p.selected || !p.queryReady || p.exporting}
              onClick={() => {
                close();
                p.onExport("selection");
              }}
            >
              选中记录
            </button>
          </>,
        )}
      </div>
      {p.searchOpen && (
        <div className="log-search" role="search">
          <div className="log-search-main">
            <Search size={15} />
            <input
              ref={input}
              aria-label="搜索全部本地日志"
              placeholder="查找已保存的日志…"
              value={p.draft}
              onChange={(e) => p.setDraft(e.target.value)}
            />
            <label>
              <input
                type="checkbox"
                checked={p.caseSensitive}
                onChange={(e) => p.setCaseSensitive(e.target.checked)}
              />
              区分大小写
            </label>
            <label>
              <input
                type="checkbox"
                checked={p.liveSearch}
                onChange={(e) => p.setLiveSearch(e.target.checked)}
              />
              实时筛选
            </label>
            <button
              aria-label="收起查找"
              onClick={() => p.setSearchOpen(false)}
            >
              <X size={15} />
            </button>
          </div>
        </div>
      )}
      {filters && (
        <div className="log-filter-chips" aria-label="当前筛选条件">
          {p.draft && (
            <button onClick={() => p.setDraft("")} aria-label="移除关键词筛选">
              关键词：{p.draft}
              <X size={12} />
            </button>
          )}
          {p.stream && (
            <button onClick={() => p.setStream("")} aria-label="移除输出流筛选">
              {p.stream}
              <X size={12} />
            </button>
          )}
          {(p.from || p.to) && (
            <button
              onClick={() => {
                p.setFrom("");
                p.setTo("");
                p.onBoot("");
              }}
              aria-label="移除时间范围"
            >
              {p.from || "开始"} — {p.to || "现在"}
              <X size={12} />
            </button>
          )}
          <button className="log-clear-filters" onClick={p.clear}>
            清除条件
          </button>
        </div>
      )}
    </div>
  );
}
