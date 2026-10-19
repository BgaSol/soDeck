import { useEffect, useRef } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Copy,
  Laptop,
  LockKeyhole,
  Pencil,
  Server as ServerIcon,
  Terminal,
} from "lucide-react";
import { desktop } from "./api";
import { addressOf, type Server } from "./types";

interface Props {
  server?: Server;
  jump?: Server;
  sessions: number;
  pending: { busy: boolean; error?: string } | null;
  canConnect: boolean;
  failed: boolean;
  focusRequested: boolean;
  onConnect: (server: Server) => void;
  onEdit: (server: Server) => void;
  onCopy: (server: Server) => void;
  onCopyAddress: (server: Server) => void;
  onBack: () => void;
}

export function ServerDetails({
  server,
  jump,
  sessions,
  pending,
  canConnect,
  failed,
  focusRequested,
  onConnect,
  onEdit,
  onCopy,
  onCopyAddress,
  onBack,
}: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (focusRequested) heading.current?.focus({ preventScroll: true });
  }, [focusRequested]);
  const state = pending?.busy
    ? "pending"
    : sessions
      ? "connected"
      : pending?.error || failed
        ? "failed"
        : "idle";
  return (
    <aside
      className="server-inspector"
      id="selected-server-details"
      aria-label="服务器详情"
    >
      {server ? (
        <>
          <div className="server-detail-toolbar">
            <span>配置详情</span>
            <button
              className="quiet detail-back"
              aria-label="返回列表"
              onClick={onBack}
            >
              <ArrowLeft size={16} />
              返回列表
            </button>
            <button
              className="quiet detail-edit"
              onClick={() => onEdit(server)}
              disabled={!desktop || !!pending}
              title="编辑服务器配置"
            >
              <Pencil size={15} />
              编辑
            </button>
          </div>
          <div className="server-detail-content">
            <header className="server-detail-heading">
              <div className="server-detail-symbol" aria-hidden="true">
                <ServerIcon size={27} />
              </div>
              <div>
                <span className="detail-kind">SSH 服务器</span>
                <h2 ref={heading} tabIndex={-1}>
                  {server.name}
                </h2>
              </div>
            </header>
            <div
              className="server-detail-state catalog-status"
              data-state={state}
            >
              <i aria-hidden="true" />
              {pending?.busy
                ? "正在建立连接"
                : sessions
                  ? `已连接 · ${sessions} 个会话`
                  : pending?.error || failed
                    ? "连接失败"
                    : "尚未连接"}
              <span className="detail-state-scope">本窗口</span>
            </div>
            <div className="server-detail-actions">
              <button
                className="primary"
                disabled={!canConnect}
                onClick={() => onConnect(server)}
              >
                <Terminal size={16} />
                {sessions
                  ? "进入已有会话"
                  : pending?.busy
                    ? "正在连接"
                    : "连接服务器"}
              </button>
              <button
                className="detail-copy"
                disabled={!desktop}
                onClick={() => onCopy(server)}
              >
                <Copy size={15} />
                复制配置
              </button>
            </div>
            <section className="server-detail-section" aria-label="连接信息">
              <h3>连接信息</h3>
              <dl className="server-detail-fields">
                <div className="detail-field-wide">
                  <dt>服务器地址</dt>
                  <dd className="detail-address">
                    <span>{addressOf(server)}</span>
                    <button
                      className="icon-button"
                      aria-label="复制服务器地址"
                      title="复制服务器地址"
                      onClick={() => onCopyAddress(server)}
                    >
                      <Copy size={15} />
                    </button>
                  </dd>
                </div>
                <div>
                  <dt>用户名</dt>
                  <dd>{server.username || "未填写"}</dd>
                </div>
                <div>
                  <dt>认证方式</dt>
                  <dd>
                    {
                      {
                        agent: "SSH Agent",
                        key: "私钥文件",
                        password: "密码认证",
                      }[server.authType]
                    }
                  </dd>
                </div>
                {server.authType === "key" && (
                  <div className="detail-field-wide">
                    <dt>本地私钥</dt>
                    <dd className="detail-mono">
                      {server.privateKey || "未设置"}
                    </dd>
                  </div>
                )}
              </dl>
            </section>
            <section className="server-detail-section" aria-label="连接路径">
              <h3>
                连接路径{" "}
                <span>{server.jumpServerId ? "通过跳板机" : "直接连接"}</span>
              </h3>
              <div className="server-detail-route">
                <div>
                  <Laptop size={20} aria-hidden="true" />
                  <span>本机</span>
                </div>
                <ArrowRight size={15} aria-hidden="true" />
                {server.jumpServerId && (
                  <>
                    <div>
                      <ServerIcon size={20} aria-hidden="true" />
                      <span title={jump?.name}>
                        {jump?.name || "跳板不可用"}
                      </span>
                    </div>
                    <ArrowRight size={15} aria-hidden="true" />
                  </>
                )}
                <div>
                  <ServerIcon size={20} aria-hidden="true" />
                  <span title={server.name}>{server.name}</span>
                </div>
              </div>
            </section>
            {server.tags.length > 0 && (
              <section
                className="server-detail-section"
                aria-label="服务器标签"
              >
                <h3>标签</h3>
                <div className="server-detail-tags">
                  {server.tags.map((tag) => (
                    <span key={tag}>{tag}</span>
                  ))}
                </div>
              </section>
            )}
            {server.directHost && (
              <section
                className="server-detail-section"
                aria-label="服务器间传输"
              >
                <h3>服务器间传输</h3>
                <p className="detail-mono">
                  {addressOf({
                    ...server,
                    host: server.directHost,
                    port: server.directPort,
                  })}
                </p>
              </section>
            )}
            <p className="server-detail-storage">
              <LockKeyhole size={13} aria-hidden="true" />
              配置保存在本机
            </p>
          </div>
        </>
      ) : (
        <div className="server-detail-empty">
          <ServerIcon size={32} />
          <h2>选择一台服务器</h2>
          <p>在列表中查看配置，开始连接。</p>
        </div>
      )}
    </aside>
  );
}
