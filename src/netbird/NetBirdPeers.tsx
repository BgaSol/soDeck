import { useMemo, useState } from "react";
import { Search, SlidersHorizontal } from "lucide-react";
import { Virtuoso } from "react-virtuoso";
import type { Snapshot } from "./netbirdApi";
import { filterPeers } from "./netbirdPresentation";
import { PeerRow } from "./NetBirdCommon";
export interface PeerFilters {
  query: string;
  state: string;
  mode: string;
}
export function NetBirdPeers({
  snapshot,
  filters,
  onChange,
  onPeer,
  scrollTop,
  onScroll,
}: {
  snapshot: Snapshot;
  filters: PeerFilters;
  onChange: (value: PeerFilters) => void;
  onPeer: (id: string) => void;
  scrollTop: number;
  onScroll: (top: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const peers = useMemo(
    () =>
      filterPeers(snapshot.peers, filters.query, filters.state, filters.mode),
    [snapshot.peers, filters],
  );
  const count = Number(!!filters.state) + Number(!!filters.mode);
  return (
    <div className="nb-peers">
      <div className="nb-search-row">
        <label className="nb-search">
          <Search size={14} />
          <input
            aria-label="搜索 NetBird 节点"
            placeholder="名称、IP 或 FQDN"
            value={filters.query}
            onChange={(e) => onChange({ ...filters, query: e.target.value })}
          />
        </label>
        <button
          className="icon-button"
          aria-label={`筛选${count ? ` ${count}` : ""}`}
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <SlidersHorizontal size={15} />
          {count || null}
        </button>
      </div>
      {open && (
        <div className="nb-filters">
          <label>
            状态
            <select
              value={filters.state}
              onChange={(e) => onChange({ ...filters, state: e.target.value })}
            >
              <option value="">全部状态</option>
              <option value="connected">已连接</option>
              <option value="connecting">连接中</option>
              <option value="idle">空闲</option>
              <option value="unknown">未知</option>
            </select>
          </label>
          <label>
            连接方式
            <select
              value={filters.mode}
              onChange={(e) => onChange({ ...filters, mode: e.target.value })}
            >
              <option value="">全部方式</option>
              <option value="direct">直连</option>
              <option value="relayed">中继</option>
              <option value="unknown">未知</option>
            </select>
          </label>
        </div>
      )}
      <p className="nb-list-count">
        {peers.length} 个匹配节点
        {!snapshot.peersComplete ? " · 列表尚不完整" : ""}
      </p>
      {!peers.length ? (
        <p className="nb-empty">
          {snapshot.peers.length
            ? "没有匹配的节点，试试其他条件。"
            : snapshot.peersComplete
              ? "当前没有可见节点"
              : "尚无完整的节点数据"}
        </p>
      ) : (
        <Virtuoso
          className="nb-peer-list"
          data={peers}
          computeItemKey={(_, peer) => peer.id}
          initialScrollTop={scrollTop}
          onScroll={(e) => onScroll(e.currentTarget.scrollTop)}
          itemContent={(_, peer) => <PeerRow peer={peer} onOpen={onPeer} />}
        />
      )}
    </div>
  );
}
