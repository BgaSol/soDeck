import {
  ArrowRight,
  Laptop,
  Network,
  Server as ServerIcon,
} from "lucide-react";
import { endpoint } from "./portForwardApi";
import type { Server } from "./types";

export function PortForwardPath({
  hops,
  target,
  local,
}: {
  hops: Server[];
  target: string;
  local: string;
}) {
  const nodes = [
    { label: "本机", detail: local, icon: Laptop },
    ...hops.map((s, i) => ({
      label: s.name,
      detail: endpoint(s.host, s.port),
      icon: i < hops.length - 1 ? Network : ServerIcon,
    })),
    ...(!hops.length
      ? [{ label: "SSH 服务器", detail: "请选择服务器", icon: ServerIcon }]
      : []),
    { label: "目标服务", detail: target, icon: Network },
  ];
  return (
    <ol
      className="route-preview forward-route"
      aria-label="转发路径"
      style={{ gridTemplateColumns: `repeat(${nodes.length}, minmax(0, 1fr))` }}
    >
      {nodes.map(({ label, detail, icon: Icon }, i) => (
        <li key={i}>
          {i > 0 && (
            <ArrowRight size={14} className="route-arrow" aria-hidden="true" />
          )}
          <Icon size={15} aria-hidden="true" />
          <strong title={label}>{label}</strong>
          <small title={detail}>{detail}</small>
        </li>
      ))}
    </ol>
  );
}
