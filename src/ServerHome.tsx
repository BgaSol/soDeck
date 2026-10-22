import { useEffect, useRef, useState } from "react";
import {
  Network,
  PanelRight,
  Plus,
  Search,
  Server as ServerIcon,
} from "lucide-react";
import { desktop } from "./api";
import { addressOf, filterServers, type Server } from "./types";
import { ServerActions } from "./ServerActions";
import type { WorkspaceTab } from "./workspaces";
import { ServerDetails } from "./ServerDetails";
import "./server-home.css";

interface Props {
  servers: Server[];
  tabs: WorkspaceTab[];
  selectedId: string;
  query: string;
  tag: string;
  loading: boolean;
  active: boolean;
  focusRequest: number;
  loadFailed: boolean;
  pending: { serverId: string; busy: boolean; error?: string } | null;
  notice: { text: string; serverId: string } | null;
  onSelect: (id: string) => void;
  onQuery: (query: string) => void;
  onTag: (tag: string) => void;
  onConnect: (server: Server, forceNew?: boolean) => void;
  onEdit: (server: Server) => void;
  onCopy: (server: Server) => void;
  onDelete: (server: Server) => void;
  onNew: () => void;
  onCancel: () => void;
  onForward: (server: Server) => void;
  onReveal: (id: string) => void;
}

export function ServerHome(props: Props) {
  const { servers, tabs, selectedId, query, tag, active, pending } = props;
  const visible = filterServers(servers, query, tag);
  const tags = [...new Set(servers.flatMap((server) => server.tags))].sort();
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [copyNotice, setCopyNotice] = useState("");
  const [showDetails, setShowDetails] = useState(false);
  const detailsToggle = useRef<HTMLButtonElement>(null);
  function backToList() {
    setShowDetails(false);
    detailsToggle.current?.focus({ preventScroll: true });
  }
  function copyAddress(server: Server) {
    void navigator.clipboard
      .writeText(addressOf(server))
      .then(() => setCopyNotice(`已复制地址：${addressOf(server)}`))
      .catch(() => setCopyNotice("地址复制失败，请在编辑窗口中手动复制。"));
  }
  useEffect(() => {
    if (!props.loading && !visible.some((server) => server.id === selectedId))
      props.onSelect(visible[0]?.id || "");
  }, [servers, query, tag, selectedId, props.loading, props.onSelect]);
  useEffect(() => {
    if (active) {
      if (!showDetails || matchMedia("(min-width: 1001px)").matches)
        search.current?.focus({ preventScroll: true });
    }
  }, [active, props.focusRequest]);
  useEffect(() => {
    if (showDetails && !visible.length) {
      setShowDetails(false);
      if (active) search.current?.focus({ preventScroll: true });
    }
  }, [showDetails, visible.length, active]);
  useEffect(() => {
    if (selectedId !== props.notice?.serverId) return;
    const row = Array.from(
      list.current?.querySelectorAll<HTMLElement>("[data-server-id]") || [],
    ).find((element) => element.dataset.serverId === selectedId);
    row?.scrollIntoView({ block: "nearest" });
  }, [selectedId, props.notice]);
  function connected(server: Server) {
    return tabs.filter((t) => t.server.id === server.id && t.state === "已连接")
      .length;
  }
  function canConnect(server: Server, forceNew = false) {
    return desktop && (!pending?.busy || (!forceNew && connected(server) > 0));
  }
  function selectWithKeyboard(event: React.KeyboardEvent) {
    if (event.key === "Escape" && showDetails) {
      event.preventDefault();
      backToList();
      return;
    }
    if (
      event.target instanceof HTMLInputElement &&
      event.target !== search.current
    )
      return;
    if (
      !(event.target instanceof HTMLInputElement) &&
      !(event.target as HTMLElement).matches(".catalog-name")
    )
      return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const index = visible.findIndex((s) => s.id === selectedId);
      const next =
        visible[
          Math.max(
            0,
            Math.min(
              visible.length - 1,
              index + (event.key === "ArrowDown" ? 1 : -1),
            ),
          )
        ];
      if (next) {
        props.onSelect(next.id);
        const row = Array.from(
          list.current?.querySelectorAll<HTMLElement>("[data-server-id]") || [],
        ).find((el) => el.dataset.serverId === next.id);
        row?.scrollIntoView({ block: "nearest" });
        if (event.target !== search.current)
          row?.querySelector<HTMLButtonElement>(".catalog-name")?.focus();
      }
    } else if (event.key === "Enter" && event.target === search.current) {
      event.preventDefault();
      const server = visible.find((s) => s.id === selectedId);
      if (server && canConnect(server)) props.onConnect(server);
    }
  }
  const selected = visible.find((s) => s.id === selectedId);
  return (
    <section
      className="server-home"
      aria-label="服务器"
      hidden={!active}
      data-inspector-open={!!selected && showDetails}
      onKeyDown={selectWithKeyboard}
    >
      <header className="server-home-heading">
        <div className="server-home-title">
          <h1>
            服务器 <span>{servers.length}</span>
          </h1>
          <p>你的远程工作空间</p>
        </div>
        <div className="server-home-header-actions">
          <button
            ref={detailsToggle}
            className="icon-button details-toggle"
            aria-label={showDetails ? "返回服务器列表" : "查看服务器详情"}
            aria-expanded={!!selected && showDetails}
            aria-controls="selected-server-details"
            disabled={!selected}
            title={showDetails ? "返回服务器列表" : "查看服务器详情"}
            onClick={() => setShowDetails(!showDetails)}
          >
            <PanelRight size={19} />
          </button>
          <button
            className="primary catalog-add"
            onClick={props.onNew}
            disabled={!desktop || props.loading || props.loadFailed}
          >
            <Plus size={17} />
            新增服务器
          </button>
        </div>
      </header>
      <div className="server-home-body">
        <div className="server-library">
          <div className="server-home-filters">
            <label className="catalog-search">
              <Search size={17} aria-hidden="true" />
              <input
                ref={search}
                type="search"
                aria-label="搜索服务器"
                placeholder="搜索名称、地址、用户名或标签"
                value={query}
                onChange={(e) => props.onQuery(e.target.value)}
              />
            </label>
            <select
              aria-label="按标签筛选"
              value={tag}
              onChange={(e) => props.onTag(e.target.value)}
            >
              <option value="">全部标签</option>
              {tags.map((t) => (
                <option key={t}>{t}</option>
              ))}
            </select>
          </div>
          <div className="catalog-section-label">
            <span>
              {query ? "搜索结果" : tag || "全部服务器"}
              <span className="catalog-result-count">{visible.length}</span>
            </span>
            <span>按名称排序</span>
          </div>
          <div
            className="server-catalog"
            ref={list}
            aria-label="服务器列表"
            role="list"
            aria-busy={props.loading}
          >
            {visible.map((server) => {
              const count = connected(server);
              const attempt = pending?.serverId === server.id ? pending : null;
              const failed =
                attempt?.error ||
                tabs.some(
                  (t) => t.server.id === server.id && t.state === "连接失败",
                );
              const jump = servers.find((s) => s.id === server.jumpServerId);
              const state = attempt?.busy
                ? "pending"
                : count
                  ? "connected"
                  : failed
                    ? "failed"
                    : "idle";
              return (
                <div
                  role="listitem"
                  className={`server-row catalog-row${selectedId === server.id ? " selected" : ""}`}
                  data-server-id={server.id}
                  key={server.id}
                  onClick={() => props.onSelect(server.id)}
                  onDoubleClick={(e) => {
                    if (
                      !(e.target as HTMLElement).closest(
                        ".catalog-actions, .catalog-address, .catalog-tags",
                      ) &&
                      canConnect(server)
                    )
                      props.onConnect(server);
                  }}
                >
                  <div className="catalog-symbol" aria-hidden="true">
                    <ServerIcon size={21} />
                  </div>
                  <div className="catalog-content">
                    <div className="catalog-row-heading">
                      <button
                        className="catalog-name"
                        title={server.name}
                        aria-pressed={selectedId === server.id}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && canConnect(server)) {
                            e.preventDefault();
                            props.onConnect(server);
                          }
                        }}
                      >
                        {server.name}
                      </button>
                      <span
                        className="catalog-status"
                        data-state={state}
                        title="会话状态仅统计本窗口"
                      >
                        <i aria-hidden="true" />
                        {attempt?.busy
                          ? "连接中…"
                          : count
                            ? `已连接 · ${count} 个会话`
                            : failed
                              ? "连接失败"
                              : "未连接"}
                      </span>
                    </div>
                    <button
                      className="catalog-address"
                      title={`复制地址 ${addressOf(server)}`}
                      aria-label={`复制 ${server.name} 的地址`}
                      onClick={() => copyAddress(server)}
                    >
                      {server.username && (
                        <span className="catalog-account">
                          {server.username}@
                        </span>
                      )}
                      {addressOf(server)}
                    </button>
                    <div className="catalog-meta">
                      <span className="catalog-route">
                        {server.jumpServerId && (
                          <Network size={12} aria-hidden="true" />
                        )}
                        {server.jumpServerId
                          ? `经 ${jump?.name || "不可用的跳板"}`
                          : "直接连接"}
                      </span>
                      <div className="catalog-tags">
                        {server.tags.slice(0, 2).map((t) => (
                          <span key={t}>{t}</span>
                        ))}
                        {server.tags.length > 2 && (
                          <details>
                            <summary
                              aria-label={`查看 ${server.name} 的全部标签`}
                            >
                              +{server.tags.length - 2}
                            </summary>
                            <div>{server.tags.join(" · ")}</div>
                          </details>
                        )}
                      </div>
                    </div>
                  </div>
                  <div className="catalog-actions">
                    <ServerActions
                      server={server}
                      connected={count > 0}
                      connecting={!!attempt?.busy}
                      canConnect={canConnect(server)}
                      canNewSession={canConnect(server, true)}
                      deleteBlocked={!!attempt}
                      onConnect={props.onConnect}
                      onEdit={props.onEdit}
                      onCopy={props.onCopy}
                      onForward={props.onForward}
                      onDelete={props.onDelete}
                    />
                  </div>
                </div>
              );
            })}
            {props.loading && (
              <div className="catalog-empty" role="status">
                正在读取服务器配置…
              </div>
            )}
            {!props.loading && !visible.length && (
              <div className="catalog-empty">
                <ServerIcon size={28} />
                <h2>
                  {props.loadFailed
                    ? "暂时无法读取服务器配置"
                    : servers.length
                      ? "没有匹配的服务器"
                      : "添加第一台服务器"}
                </h2>
                <p>
                  {props.loadFailed
                    ? "请查看上方错误提示。"
                    : servers.length
                      ? "试试其他关键词，或清除筛选。"
                      : "保存连接配置，开始你的第一个 SSH 会话。"}
                </p>
                {!props.loadFailed &&
                  (servers.length ? (
                    <button
                      onClick={() => {
                        props.onQuery("");
                        props.onTag("");
                      }}
                    >
                      清除筛选
                    </button>
                  ) : (
                    <button
                      className="primary"
                      disabled={!desktop}
                      onClick={props.onNew}
                    >
                      添加服务器
                    </button>
                  ))}
              </div>
            )}
          </div>
        </div>
        <ServerDetails
          server={selected}
          jump={servers.find((s) => s.id === selected?.jumpServerId)}
          sessions={selected ? connected(selected) : 0}
          pending={pending?.serverId === selected?.id ? pending : null}
          canConnect={!!selected && canConnect(selected)}
          canNewSession={!!selected && canConnect(selected, true)}
          failed={tabs.some(
            (t) => t.server.id === selected?.id && t.state === "连接失败",
          )}
          focusRequested={active && showDetails}
          onConnect={props.onConnect}
          onEdit={props.onEdit}
          onCopy={props.onCopy}
          onForward={props.onForward}
          onDelete={props.onDelete}
          onCopyAddress={copyAddress}
          onBack={backToList}
        />
      </div>
      {pending && (
        <div
          className="catalog-feedback"
          role={pending.error ? "alert" : "status"}
        >
          <span>
            {pending.busy
              ? "正在建立连接，请完成指纹确认或登录认证…"
              : pending.error}
          </span>
          {!pending.busy && (
            <button
              disabled={!desktop}
              onClick={() => {
                const source = servers.find((s) => s.id === pending.serverId);
                if (source) props.onConnect(source, true);
              }}
            >
              重试
            </button>
          )}
          <button onClick={props.onCancel}>
            {pending.busy ? "取消连接" : "关闭连接提示"}
          </button>
        </div>
      )}
      {props.notice && (
        <div className="catalog-feedback" role="status">
          <span>
            {props.notice.text}
            {!visible.some((s) => s.id === props.notice?.serverId)
              ? "，当前筛选下不可见。"
              : ""}
          </span>
          {!visible.some((s) => s.id === props.notice?.serverId) && (
            <button onClick={() => props.onReveal(props.notice!.serverId)}>
              查看配置
            </button>
          )}
        </div>
      )}
      {copyNotice && (
        <div className="catalog-feedback" role="status">
          {copyNotice}
          <button onClick={() => setCopyNotice("")}>关闭</button>
        </div>
      )}
      <footer className="catalog-footer">
        <span>
          {selected ? `已选：${selected.name}` : "服务器配置保存在本机"}
        </span>
        <span className="catalog-keyboard-hint">
          <kbd>↑</kbd>
          <kbd>↓</kbd> 选择 <span>·</span> <kbd>Enter</kbd> 连接
        </span>
      </footer>
    </section>
  );
}
