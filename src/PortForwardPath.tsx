import {
  ArrowRight,
  Laptop,
  Network,
  Server as ServerIcon,
} from "lucide-react";
import { endpoint } from "./portForwardApi";
import { sshServerSummary } from "./sshServerPickerUtils";
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
    { label: "本机", detail: local, name: "", icon: Laptop },
    ...hops.map((s, i) => ({
      label: i < hops.length - 1 ? "SSH 跳板" : "SSH 访问节点",
      name: sshServerSummary(s).title,
      detail:
        sshServerSummary(s).title === endpoint(s.host, s.port)
          ? ""
          : endpoint(s.host, s.port),
      icon: i < hops.length - 1 ? Network : ServerIcon,
    })),
    ...(!hops.length
      ? [
          {
            label: "SSH 访问节点",
            name: "",
            detail: "请选择节点",
            icon: ServerIcon,
          },
        ]
      : []),
    { label: "目标服务", name: "", detail: target, icon: Network },
  ];
  return (
    <ol
      className="route-preview forward-route"
      aria-label="转发路径"
      style={{ gridTemplateColumns: `repeat(${nodes.length}, minmax(0, 1fr))` }}
    >
      {nodes.map(({ label, name, detail, icon: Icon }, i) => (
        <li key={i}>
          {i > 0 && (
            <ArrowRight size={14} className="route-arrow" aria-hidden="true" />
          )}
          <Icon size={15} aria-hidden="true" />
          <strong title={label}>{label}</strong>
          {name && <small title={name}>{name}</small>}
          {detail && <small title={detail}>{detail}</small>}
        </li>
      ))}
    </ol>
  );
}
