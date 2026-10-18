import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, Pin, Search } from "lucide-react";
import { useMonitor, useMonitorPaths } from "./MonitorProvider";
import { MountDetailsPopover } from "./MountDetailsPopover";
import { formatMountBytes, mountSource } from "./remoteMounts";
import {
  mountGroup,
  type MountGroup,
  type ResourcePreferences,
} from "./resourcePresentation";
const groupNames: Record<MountGroup, string> = {
  storage: "存储挂载",
  system: "系统",
  runtime: "运行时",
  memory: "内存文件系统",
};
const initialView = {
  all: false,
  query: "",
  onlyPinned: false,
  groups: ["storage"] as MountGroup[],
  page: 0,
};
export function MountOverview({
  active,
  preferences,
  onChange,
  onReveal,
}: {
  active: boolean;
  preferences: ResourcePreferences;
  onChange: (value: ResourcePreferences) => void;
  onReveal: (path: string) => void;
}) {
  const { store, state } = useMonitor();
  const view = preferences.mounts ?? initialView;
  const pins = preferences.pinnedMounts ?? ["/"];
  const panel = useRef<HTMLElement>(null);
  const [visiblePaths, setVisiblePaths] = useState<string[]>([]);
  const [detail, setDetail] = useState<{
    key: string;
    path: string;
    anchor: HTMLElement;
  } | null>(null);
  const [notice, setNotice] = useState("");
  const mounts = useMemo(
    () =>
      [...(state.mounts.table?.mounts ?? [])].sort((a, b) =>
        a.target === "/"
          ? -1
          : b.target === "/"
            ? 1
            : a.target.localeCompare(b.target),
      ),
    [state.mounts.table],
  );
  const mountIssues = state.mounts.table?.issues ?? [];
  const issueByTarget = useMemo(
    () => new Map(mountIssues.map((issue) => [issue.target, issue])),
    [mountIssues],
  );
  const filtered = useMemo(
    () =>
      mounts.filter((m) => {
        if (!view.all) return pins.includes(m.target);
        const query = view.query.trim().toLocaleLowerCase();
        return (
          (!view.onlyPinned || pins.includes(m.target)) &&
          view.groups.includes(mountGroup(m)) &&
          (!query ||
            `${m.target}\n${mountSource(m)}`
              .toLocaleLowerCase()
              .includes(query))
        );
      }),
    [mounts, view, pins],
  );
  const pages = Math.max(1, Math.ceil(filtered.length / 8));
  const page = Math.min(Math.max(0, view.page), pages - 1);
  const shown = useMemo(
    () =>
      view.all ? filtered.slice(page * 8, page * 8 + 8) : filtered.slice(0, 3),
    [filtered, page, view.all],
  );
  const selected = mounts.find(
    (m) => m.key === detail?.key && m.target === detail.path,
  );
  useEffect(() => {
    if (!active || !selected) setDetail(null);
  }, [active, selected]);
  useEffect(() => {
    if (!active || !panel.current) {
      setVisiblePaths([]);
      return;
    }
    const paths = new Set<string>();
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const path = (entry.target as HTMLElement).dataset.mountPath!;
        if (entry.isIntersecting) paths.add(path);
        else paths.delete(path);
      }
      const next = [...paths].sort();
      setVisiblePaths((old) =>
        old.join("\0") === next.join("\0") ? old : next,
      );
    });
    panel.current
      .querySelectorAll<HTMLElement>("[data-mount-path]")
      .forEach((row) => observer.observe(row));
    return () => observer.disconnect();
  }, [active, shown]);
  const paths = useMemo(
    () => [
      ...new Set([...visiblePaths, ...(selected ? [selected.target] : [])]),
    ],
    [visiblePaths, selected],
  );
  useMonitorPaths(store, paths, active && state.connected);
  const changeView = (patch: Partial<typeof initialView>) =>
    onChange({ ...preferences, mounts: { ...view, ...patch } });
  function togglePin(path: string) {
    setNotice("");
    if (!pins.includes(path) && pins.length >= 3) {
      setNotice("最多关注 3 个挂载，请先取消一项。");
      return;
    }
    onChange({
      ...preferences,
      pinnedMounts: pins.includes(path)
        ? pins.filter((p) => p !== path)
        : [...pins, path],
    });
  }
  return (
    <section ref={panel} className="mount-overview" aria-label="磁盘总览">
      <header className="resource-section-heading">
        <h3>{view.all ? "全部挂载" : "关注的挂载"}</h3>
        <button
          className="resource-link"
          aria-expanded={view.all}
          onClick={() => changeView({ all: !view.all })}
        >
          {view.all ? "返回关注" : `全部 ${mounts.length} 个`}
        </button>
      </header>
      {view.all && (
        <div className="mount-list-controls">
          <label className="inspector-search">
            <Search size={14} />
            <input
              value={view.query}
              aria-label="搜索挂载路径或来源"
              placeholder="搜索路径或来源"
              onChange={(e) => changeView({ query: e.target.value, page: 0 })}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.stopPropagation();
                  changeView({ query: "", page: 0 });
                }
              }}
            />
          </label>
          <label className="resource-checkbox">
            <input
              type="checkbox"
              checked={view.onlyPinned}
              onChange={(e) =>
                changeView({ onlyPinned: e.target.checked, page: 0 })
              }
            />
            仅显示关注
          </label>
          <div
            className="mount-group-filters"
            role="group"
            aria-label="挂载分组"
          >
            {(Object.keys(groupNames) as MountGroup[]).map((group) => (
              <label className="resource-checkbox" key={group}>
                <input
                  type="checkbox"
                  checked={view.groups.includes(group)}
                  onChange={(e) =>
                    changeView({
                      groups: e.target.checked
                        ? [...view.groups, group]
                        : view.groups.filter((g) => g !== group),
                      page: 0,
                    })
                  }
                />
                {groupNames[group]}{" "}
                <span>
                  {mounts.filter((m) => mountGroup(m) === group).length}
                </span>
              </label>
            ))}
          </div>
          {view.groups.length < 4 && (
            <button
              className="resource-link"
              onClick={() =>
                changeView({
                  groups: ["storage", "system", "runtime", "memory"],
                  page: 0,
                })
              }
            >
              显示所有分组
            </button>
          )}
        </div>
      )}
      {shown.map((m) => {
        const cached = state.mounts.usage[m.key],
          usage = cached?.value;
        return (
          <div
            className="mount-overview-item"
            key={m.key}
            data-mount-path={m.target}
          >
            <button
              className="disk-overview-row"
              onClick={(e) =>
                setDetail({
                  key: m.key,
                  path: m.target,
                  anchor: e.currentTarget,
                })
              }
              aria-label={`查看 ${m.target} 挂载详情`}
            >
              <span className="disk-overview-name">
                <strong title={m.target}>{m.target}</strong>
                <span>
                  {usage?.usedPercent == null ? "—" : `${usage.usedPercent}%`}
                  {m.readOnly ? " · 只读" : ""}
                </span>
              </span>
              {usage?.status === "ready" && usage.usedPercent != null && (
                <span className="mount-meter" aria-hidden="true">
                  <span
                    style={{
                      width: `${Math.max(0, Math.min(100, usage.usedPercent))}%`,
                    }}
                  />
                </span>
              )}
              <span className="disk-overview-capacity">
                {usage?.status === "ready"
                  ? `${formatMountBytes(usage.usedBytes)} / ${formatMountBytes(usage.totalBytes)}`
                  : cached?.loading
                    ? "读取容量中"
                    : cached?.error || usage?.error || "容量暂不可用"}
                {cached?.stale
                  ? ` · 上次数据${usage?.updatedAt ? ` ${new Date(usage.updatedAt).toLocaleTimeString()}` : ""}`
                  : ""}
              </span>
              {view.all && (
                <span className="disk-overview-source" title={mountSource(m)}>
                  {mountSource(m)} · {m.fsType}
                </span>
              )}
            </button>
            <button
              className={`mount-pin icon-button${pins.includes(m.target) ? " selected" : ""}`}
              aria-label={`${pins.includes(m.target) ? "取消关注" : "关注"} ${m.target}`}
              aria-pressed={pins.includes(m.target)}
              onClick={() => togglePin(m.target)}
            >
              <Pin size={13} />
            </button>
          </div>
        );
      })}
      {!view.all &&
        pins
          .filter((path) => !mounts.some((m) => m.target === path))
          .map((path) => (
            <div className="missing-mount" key={path}>
              <span>
                {path} ·{" "}
                {state.mounts.loading
                  ? "正在获取"
                  : issueByTarget.has(path)
                    ? "挂载层级无法确认，容量已隐藏"
                    : "当前未挂载"}
              </span>
              <button className="resource-link" onClick={() => togglePin(path)}>
                取消关注
              </button>
            </div>
          ))}
      {!shown.length && (
        <p className="resource-empty">
          {state.mounts.loading
            ? "正在读取挂载信息"
            : state.mounts.table?.status === "unsupported"
              ? state.mounts.table.message || "当前环境不支持挂载信息"
              : view.all
                ? "没有匹配的挂载"
                : "在全部挂载中添加关注项"}
        </p>
      )}
      {notice && (
        <p className="resource-notice" role="status">
          {notice}
        </p>
      )}
      {view.all && mountIssues.length > 0 && (
        <p className="resource-notice" role="status">
          {mountIssues.length === 1
            ? `未列出 ${mountIssues[0].target}：挂载层级无法确认`
            : `另有 ${mountIssues.length} 个挂载路径层级无法确认，未列出容量`}
        </p>
      )}
      {state.mounts.error && (
        <p className="resource-error">{state.mounts.error}</p>
      )}
      {view.all && pages > 1 && (
        <div className="disk-pagination">
          <button
            aria-label="上一页挂载"
            disabled={page === 0}
            onClick={() => changeView({ page: page - 1 })}
          >
            <ChevronLeft size={14} />
          </button>
          <span>
            {page + 1} / {pages} · {filtered.length} 项
          </span>
          <button
            aria-label="下一页挂载"
            disabled={page === pages - 1}
            onClick={() => changeView({ page: page + 1 })}
          >
            <ChevronRight size={14} />
          </button>
        </div>
      )}
      {view.all && (
        <p className="resource-help">
          容量按挂载点分别显示，与文件树共用采样。绑定挂载与子卷不相加。
        </p>
      )}
      {selected && detail && (
        <MountDetailsPopover
          mount={selected}
          usage={state.mounts.usage[selected.key]}
          anchor={detail.anchor}
          onClose={() => setDetail(null)}
          onRefresh={() => void store.refresh("mounts")}
          onReveal={() => {
            setDetail(null);
            onReveal(selected.target);
          }}
        />
      )}
    </section>
  );
}
