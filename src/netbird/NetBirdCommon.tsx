import { Check, ChevronRight, Copy } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Peer } from "./netbirdApi";
import { latency, statusLabel } from "./netbirdPresentation";
export function CopyValue({ value }: { value: string }) {
  const [state, setState] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <button
      className="icon-button nb-copy"
      title={state || "复制"}
      aria-label={state || `复制 ${value}`}
      onClick={() => {
        void navigator.clipboard
          .writeText(value)
          .then(
            () => setState("已复制"),
            () => setState("复制失败"),
          )
          .then(() => {
            clearTimeout(timer.current);
            timer.current = setTimeout(() => setState(""), 1800);
          });
      }}
    >
      {state === "已复制" ? <Check size={13} /> : <Copy size={13} />}
      <span className="sr-only" role="status">
        {state}
      </span>
    </button>
  );
}
export function Fact({
  label,
  children,
  copy,
}: {
  label: string;
  children: ReactNode;
  copy?: string;
}) {
  return (
    <div className="nb-fact">
      <dt>{label}</dt>
      <dd>
        <span>{children}</span>
        {copy && <CopyValue value={copy} />}
      </dd>
    </div>
  );
}
export function PeerRow({
  peer,
  onOpen,
}: {
  peer: Peer;
  onOpen: (id: string) => void;
}) {
  return (
    <button
      className="nb-peer-row"
      onClick={() => onOpen(peer.id)}
      title={peer.name}
    >
      <span className="nb-peer-main">
        <strong>{peer.name}</strong>
        <span className="nb-secondary">
          {peer.ipv4 || peer.ipv6 || "地址未知"} ·{" "}
          {statusLabel(peer.connectionMode)}
        </span>
      </span>
      <span className="nb-peer-side">
        <span className={peer.state === "connected" ? "nb-good" : "nb-muted"}>
          {statusLabel(peer.state)}
        </span>
        <span className="nb-secondary">{latency(peer.latencyMs)}</span>
      </span>
      <ChevronRight size={13} aria-hidden="true" />
    </button>
  );
}
