import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  type CreateContainerSpec,
  type DockerPreview,
  type DockerOperationRequest,
} from "../dockerApi";
import { DockerConfirmDialog } from "./DockerConfirmDialog";
import { errorOf } from "../types";
export function CreateContainerDialog({
  imageId,
  imageName,
  onClose,
}: {
  imageId: string;
  imageName: string;
  onClose: () => void;
}) {
  const { store, state } = useDockerManagement();
  const dialog = useRef<HTMLDialogElement>(null),
    pending = useRef(false),
    request = useRef<DockerOperationRequest | null>(null);
  const [spec, setSpec] = useState<CreateContainerSpec>({
    imageId,
    name: "",
    start: true,
    ports: [],
    environment: [],
    mounts: [],
    network: null,
    restartPolicy: "unless-stopped",
    maximumRetryCount: 0,
    command: [],
    workingDirectory: "",
  });
  const [env, setEnv] = useState(""),
    [command, setCommand] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [preview, setPreview] = useState<DockerPreview | null>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    if (!preview) dialog.current?.showModal();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [preview]);
  async function prepare() {
    if (!state.engine || pending.current) return;
    const environment = env
      .split("\n")
      .filter((line) => line.trim())
      .map((line) => {
        const at = line.indexOf("=");
        if (at < 1) throw new Error("环境变量每行须为 KEY=value");
        return { key: line.slice(0, at).trim(), value: line.slice(at + 1) };
      });
    if (new Set(environment.map((e) => e.key)).size !== environment.length)
      throw new Error("环境变量键不能重复");
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      const value = await dockerApi.prepareCreate(
        store.sessionId,
        state.engine.token,
        { ...spec, environment, command: command.split("\n").filter(Boolean) },
      );
      request.current = {
        requestId: crypto.randomUUID(),
        engineToken: value.engine.token,
        targetId: value.targetId,
        action: value.action,
        previewId: value.id,
      };
      setPreview(value);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  if (preview)
    return (
      <DockerConfirmDialog
        preview={preview}
        onClose={() => setPreview(null)}
        onConfirm={async () => {
          if (!request.current) return;
          await dockerApi.submit(store.sessionId, request.current);
          onClose();
        }}
      />
    );
  return (
    <dialog
      ref={dialog}
      className="login-dialog docker-confirm-dialog docker-form-dialog"
      aria-label="创建容器"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void prepare().catch((e) => setError(errorOf(e).message));
        }}
      >
        <header className="dialog-head">
          <h2>创建容器</h2>
        </header>
        <p className="docker-confirm-object">{imageName}</p>
        <label className="docker-form-field">
          容器名称
          <input
            autoFocus
            required
            maxLength={255}
            pattern="[A-Za-z0-9][A-Za-z0-9_.-]*"
            value={spec.name}
            onChange={(e) => setSpec((s) => ({ ...s, name: e.target.value }))}
          />
        </label>
        <details className="docker-detail-group" open>
          <summary>
            发布端口<span>{spec.ports.length}</span>
          </summary>
          {spec.ports.map((p, i) => (
            <div className="docker-create-row" key={i}>
              <label>
                宿主地址
                <input
                  aria-label={`宿主地址 ${i + 1}`}
                  value={p.hostIp}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      ports: s.ports.map((v, n) =>
                        n === i ? { ...v, hostIp: e.target.value } : v,
                      ),
                    }))
                  }
                />
              </label>
              <label>
                宿主端口
                <input
                  aria-label={`宿主端口 ${i + 1}`}
                  type="number"
                  min={1}
                  max={65535}
                  required
                  value={p.hostPort || ""}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      ports: s.ports.map((v, n) =>
                        n === i
                          ? { ...v, hostPort: Number(e.target.value) }
                          : v,
                      ),
                    }))
                  }
                />
              </label>
              <label>
                容器端口
                <input
                  aria-label={`容器端口 ${i + 1}`}
                  type="number"
                  min={1}
                  max={65535}
                  required
                  value={p.containerPort || ""}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      ports: s.ports.map((v, n) =>
                        n === i
                          ? { ...v, containerPort: Number(e.target.value) }
                          : v,
                      ),
                    }))
                  }
                />
              </label>
              <select
                aria-label={`协议 ${i + 1}`}
                value={p.protocol}
                onChange={(e) =>
                  setSpec((s) => ({
                    ...s,
                    ports: s.ports.map((v, n) =>
                      n === i ? { ...v, protocol: e.target.value } : v,
                    ),
                  }))
                }
              >
                <option value="tcp">TCP</option>
                <option value="udp">UDP</option>
              </select>
              <button
                type="button"
                className="icon-button"
                aria-label={`删除端口 ${i + 1}`}
                onClick={() =>
                  setSpec((s) => ({
                    ...s,
                    ports: s.ports.filter((_, n) => n !== i),
                  }))
                }
              >
                <X size={13} />
              </button>
            </div>
          ))}
          <button
            type="button"
            className="resource-link"
            onClick={() =>
              setSpec((s) => ({
                ...s,
                ports: [
                  ...s.ports,
                  {
                    hostIp: "127.0.0.1",
                    hostPort: 0,
                    containerPort: 0,
                    protocol: "tcp",
                  },
                ],
              }))
            }
          >
            <Plus size={12} />
            添加端口
          </button>
        </details>
        <details className="docker-detail-group">
          <summary>
            环境变量<span>仅保留在当前草稿</span>
          </summary>
          <label className="docker-form-field">
            每行 KEY=value
            <textarea
              className="form-textarea"
              autoComplete="off"
              spellCheck={false}
              rows={4}
              value={env}
              onChange={(e) => setEnv(e.target.value)}
            />
          </label>
          <p className="resource-help">
            不支持多行值；离开创建窗口后清空。通过远端私有临时文件传入。
          </p>
        </details>
        <details className="docker-detail-group">
          <summary>
            存储挂载<span>{spec.mounts.length}</span>
          </summary>
          {spec.mounts.map((m, i) => (
            <div className="docker-mount-editor" key={i}>
              <div className="docker-inline-tools">
                <select
                  aria-label={`挂载类型 ${i + 1}`}
                  value={m.kind}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      mounts: s.mounts.map((v, n) =>
                        n === i ? { ...v, kind: e.target.value } : v,
                      ),
                    }))
                  }
                >
                  <option value="bind">Bind Mount</option>
                  <option value="volume">已有 Volume</option>
                </select>
                <label>
                  <input
                    type="checkbox"
                    checked={m.readOnly}
                    onChange={(e) =>
                      setSpec((s) => ({
                        ...s,
                        mounts: s.mounts.map((v, n) =>
                          n === i ? { ...v, readOnly: e.target.checked } : v,
                        ),
                      }))
                    }
                  />
                  只读
                </label>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`删除挂载 ${i + 1}`}
                  onClick={() =>
                    setSpec((s) => ({
                      ...s,
                      mounts: s.mounts.filter((_, n) => n !== i),
                    }))
                  }
                >
                  <X size={13} />
                </button>
              </div>
              <label className="docker-form-field">
                {m.kind === "bind" ? "远端宿主绝对路径" : "已有 Volume 名称"}
                <input
                  required
                  value={m.source}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      mounts: s.mounts.map((v, n) =>
                        n === i ? { ...v, source: e.target.value } : v,
                      ),
                    }))
                  }
                />
              </label>
              <label className="docker-form-field">
                容器路径
                <input
                  required
                  value={m.destination}
                  onChange={(e) =>
                    setSpec((s) => ({
                      ...s,
                      mounts: s.mounts.map((v, n) =>
                        n === i ? { ...v, destination: e.target.value } : v,
                      ),
                    }))
                  }
                />
              </label>
            </div>
          ))}
          <button
            type="button"
            className="resource-link"
            onClick={() =>
              setSpec((s) => ({
                ...s,
                mounts: [
                  ...s.mounts,
                  {
                    kind: "bind",
                    source: "",
                    destination: "",
                    readOnly: false,
                  },
                ],
              }))
            }
          >
            <Plus size={12} />
            添加挂载
          </button>
        </details>
        <details className="docker-detail-group">
          <summary>
            更多设置<span>网络、重启与命令</span>
          </summary>
          <label className="docker-form-field">
            已有网络名称
            <input
              value={spec.network ?? ""}
              placeholder="默认 bridge"
              onChange={(e) =>
                setSpec((s) => ({ ...s, network: e.target.value || null }))
              }
            />
          </label>
          <label className="docker-form-field">
            重启策略
            <select
              value={spec.restartPolicy}
              onChange={(e) =>
                setSpec((s) => ({ ...s, restartPolicy: e.target.value }))
              }
            >
              <option value="no">不自动重启</option>
              <option value="unless-stopped">除非手动停止</option>
              <option value="always">始终重启</option>
              <option value="on-failure">仅失败时重启</option>
            </select>
          </label>
          <label className="docker-form-field">
            工作目录
            <input
              value={spec.workingDirectory}
              onChange={(e) =>
                setSpec((s) => ({ ...s, workingDirectory: e.target.value }))
              }
            />
          </label>
          <label className="docker-form-field">
            命令参数
            <textarea
              className="form-textarea"
              rows={3}
              value={command}
              onChange={(e) => setCommand(e.target.value)}
            />
            <small>
              每行一个参数，保持原样传递。例如第一行 /bin/sh，第二行
              -c，第三行才是 shell 命令。
            </small>
          </label>
        </details>
        <label className="docker-check-field">
          <input
            type="checkbox"
            checked={spec.start}
            onChange={(e) =>
              setSpec((s) => ({ ...s, start: e.target.checked }))
            }
          />
          创建后启动
        </label>
        {error && (
          <p className="resource-error" role="alert">
            {error}
          </p>
        )}
        <footer className="dialog-actions">
          <button type="button" disabled={busy} onClick={onClose}>
            取消
          </button>
          <button className="primary" disabled={busy}>
            {busy ? "正在预检…" : "预览创建"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
