import { Copy, Network, Pencil, Plus, Terminal, Trash2 } from "lucide-react";
import { desktop } from "./api";
import type { Server } from "./types";

interface Props {
  server: Server;
  connected: boolean;
  connecting: boolean;
  canConnect: boolean;
  canNewSession: boolean;
  deleteBlocked: boolean;
  onConnect: (server: Server, forceNew?: boolean) => void;
  onEdit: (server: Server) => void;
  onCopy: (server: Server) => void;
  onForward: (server: Server) => void;
  onDelete: (server: Server) => void;
}

export function ServerActions(props: Props) {
  const { server } = props;
  const actions = [
    {
      id: "connect",
      label: props.connected
        ? "进入会话"
        : props.connecting
          ? "连接中…"
          : "连接",
      icon: Terminal,
      disabled: !props.canConnect,
      run: () => props.onConnect(server),
    },
    {
      id: "new-session",
      label: "新建会话",
      icon: Plus,
      disabled: !props.canNewSession,
      run: () => props.onConnect(server, true),
    },
    {
      id: "edit",
      label: "编辑",
      icon: Pencil,
      disabled: !desktop,
      run: () => props.onEdit(server),
    },
    {
      id: "copy",
      label: "复制配置",
      icon: Copy,
      disabled: !desktop,
      run: () => props.onCopy(server),
    },
    {
      id: "forward",
      label: "端口转发",
      icon: Network,
      disabled: !desktop,
      run: () => props.onForward(server),
    },
    {
      id: "delete",
      label: "删除配置",
      icon: Trash2,
      disabled: !desktop || props.deleteBlocked,
      run: () => props.onDelete(server),
    },
  ];
  return (
    <div
      className="server-actions"
      role="group"
      aria-label={`${server.name} 快捷操作`}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
    >
      {actions.map(({ id, label, icon: Icon, disabled, run }) => (
        <span key={id} className={`server-action-wrap action-${id}`}>
          <button
            type="button"
            className={`icon-button server-action ${id === "connect" ? "catalog-connect" : ""}`}
            data-server-action={id}
            aria-label={label}
            disabled={disabled}
            onClick={run}
          >
            <Icon size={17} aria-hidden="true" />
          </button>
          <span className="server-action-tooltip" aria-hidden="true">
            {label}
          </span>
        </span>
      ))}
    </div>
  );
}
