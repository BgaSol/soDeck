import { useCallback, useEffect, useRef, useState } from "react";
import {
  Boxes,
  ChevronRight,
  Layers,
  RefreshCw,
  TerminalSquare,
  ScrollText,
} from "lucide-react";
import { api } from "./api";
import { errorOf } from "./types";
import type { Container, DockerImage, DockerStatus } from "./types";
const statusNames: Record<string, string> = {
  running: "运行中",
  detecting: "检测中",
  missing: "未找到命令",
  permission: "权限不足",
  unreachable: "服务不可达",
  error: "检测失败",
  disconnected: "未连接",
};
const containerStates: Record<string, string> = {
  running: "运行中",
  exited: "已停止",
  paused: "已暂停",
  restarting: "重启中",
  created: "已创建",
  dead: "已结束",
  removing: "删除中",
};
export interface DockerPanelSnapshot {
  status: DockerStatus;
  images: DockerImage[];
  containers: Container[];
  rootOpen: boolean;
  imagesOpen: boolean;
  containersOpen: boolean;
}
export function DockerPanel({
  sessionId,
  serverName,
  active = true,
  initialState,
  onStateChange,
}: {
  sessionId: string | null;
  serverName: string;
  active?: boolean;
  initialState?: DockerPanelSnapshot;
  onStateChange?: (snapshot: DockerPanelSnapshot) => void;
}) {
  const [status, setStatus] = useState<DockerStatus>(
    initialState?.status || {
      state: "disconnected",
      message: "连接服务器后自动检测",
      endpoint: "",
    },
  );
  const [images, setImages] = useState<DockerImage[]>(
      initialState?.images || [],
    ),
    [containers, setContainers] = useState<Container[]>(
      initialState?.containers || [],
    ),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [opening, setOpening] = useState("");
  const [openingLogs, setOpeningLogs] = useState("");
  const [rootOpen, setRootOpen] = useState(initialState?.rootOpen ?? true),
    [imagesOpen, setImagesOpen] = useState(initialState?.imagesOpen ?? false),
    [containersOpen, setContainersOpen] = useState(
      initialState?.containersOpen ?? true,
    );
  const initialMount = useRef(true);
  useEffect(() => {
    onStateChange?.({
      status,
      images,
      containers,
      rootOpen,
      imagesOpen,
      containersOpen,
    });
  }, [
    status,
    images,
    containers,
    rootOpen,
    imagesOpen,
    containersOpen,
    onStateChange,
  ]);
  const current = useRef({ active, rootOpen, imagesOpen, containersOpen });
  current.current = { active, rootOpen, imagesOpen, containersOpen };
  const epoch = useRef(0),
    inFlight = useRef(false),
    rerun = useRef(false);
  const refresh = useCallback(async () => {
    if (
      !sessionId ||
      !current.current.active ||
      document.visibilityState !== "visible"
    )
      return;
    if (inFlight.current) {
      rerun.current = true;
      return;
    }
    inFlight.current = true;
    const token = epoch.current;
    setBusy(true);
    setError("");
    try {
      const result = await api.docker(sessionId);
      if (token !== epoch.current) return;
      setStatus(result);
      if (result.state !== "running") {
        setImages([]);
        setContainers([]);
        return;
      }
      const expanded = current.current;
      const [newImages, newContainers] = await Promise.all([
        expanded.rootOpen && expanded.imagesOpen
          ? api.images(sessionId, result.endpoint)
          : Promise.resolve(null),
        expanded.rootOpen && expanded.containersOpen
          ? api.containers(sessionId, result.endpoint)
          : Promise.resolve(null),
      ]);
      if (token !== epoch.current) return;
      if (newImages) setImages(newImages);
      if (newContainers) setContainers(newContainers);
    } catch (error) {
      if (token === epoch.current) {
        setError(errorOf(error).message);
        setStatus((previous) => ({
          ...previous,
          state: "error",
          message: "未能更新 Docker 状态",
        }));
      }
    } finally {
      if (token === epoch.current) {
        setBusy(false);
        inFlight.current = false;
        if (rerun.current) {
          rerun.current = false;
          queueMicrotask(() => void refresh());
        }
      }
    }
  }, [sessionId]);
  useEffect(() => {
    epoch.current++;
    if (initialMount.current && initialState) {
      initialMount.current = false;
      return () => {
        epoch.current++;
      };
    }
    initialMount.current = false;
    inFlight.current = false;
    rerun.current = false;
    setImages([]);
    setContainers([]);
    setOpening("");
    setOpeningLogs("");
    setError("");
    setBusy(false);
    setStatus({
      state: sessionId ? "detecting" : "disconnected",
      message: sessionId ? "正在检查 Docker 引擎" : "连接服务器后自动检测",
      endpoint: "",
    });
    return () => {
      epoch.current++;
    };
  }, [sessionId, refresh]);
  useEffect(() => {
    if (!sessionId || !active) return;
    void refresh();
    const resume = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", resume);
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, 15000);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", resume);
    };
  }, [sessionId, active, refresh]);
  function toggle(key: "rootOpen" | "imagesOpen" | "containersOpen") {
    const value = !current.current[key];
    current.current = { ...current.current, [key]: value };
    if (key === "rootOpen") setRootOpen(value);
    else if (key === "imagesOpen") setImagesOpen(value);
    else setContainersOpen(value);
    if (value) void refresh();
  }
  async function enter(container: Container) {
    if (!sessionId || status.state !== "running") return;
    const token = epoch.current;
    setOpening(container.id);
    setError("");
    try {
      await api.openContainer(sessionId, container, status.endpoint);
    } catch (error) {
      if (token === epoch.current) setError(errorOf(error).message);
    } finally {
      if (token === epoch.current) setOpening("");
    }
  }
  async function viewLogs(container: Container) {
    if (!sessionId || status.state !== "running") return;
    const token = epoch.current;
    setOpeningLogs(container.id);
    setError("");
    try {
      await api.openContainerLogs(sessionId, container, status.endpoint);
    } catch (error) {
      if (token === epoch.current) setError(errorOf(error).message);
    } finally {
      if (token === epoch.current) setOpeningLogs("");
    }
  }
  return (
    <section className="docker-panel" aria-label="服务器服务树">
      <header className="panel-head service-head">
        <div className="panel-title">
          <h2>
            <Boxes size={16} aria-hidden="true" />
            服务
          </h2>
          <button
            className="icon-button"
            aria-label="刷新 Docker 状态"
            disabled={!sessionId || busy}
            onClick={() => void refresh()}
          >
            <RefreshCw size={15} className={busy ? "spinning" : ""} />
          </button>
        </div>
        <span className="panel-server">{serverName || "未选择服务器"}</span>
      </header>
      <div className="service-scroll">
        <button
          className="tree-toggle docker-root"
          aria-expanded={rootOpen}
          onClick={() => toggle("rootOpen")}
        >
          <ChevronRight size={14} className={rootOpen ? "expanded" : ""} />
          <Boxes size={16} />
          <span>Docker</span>
          <span
            className={`tree-state ${status.state === "running" ? "running" : status.state === "error" || status.state === "permission" || status.state === "unreachable" ? "failed" : ""}`}
            aria-live="polite"
          >
            {busy ? "检测中" : statusNames[status.state] || status.state}
          </span>
        </button>
        {rootOpen && (
          <div className="tree-body">
            {status.endpoint && (
              <div className="docker-endpoint" title={status.endpoint}>
                {status.endpoint}
              </div>
            )}
            {status.state !== "running" && (
              <p className="tree-message">{status.message}</p>
            )}
            {(status.state === "running" ||
              images.length > 0 ||
              containers.length > 0) && (
              <>
                {status.state !== "running" && (
                  <p className="tree-message">列表已过期，请刷新后再操作。</p>
                )}
                <button
                  className="tree-toggle"
                  aria-expanded={imagesOpen}
                  onClick={() => toggle("imagesOpen")}
                >
                  <ChevronRight
                    size={14}
                    className={imagesOpen ? "expanded" : ""}
                  />
                  <span>镜像</span>
                  {imagesOpen && (
                    <span className="tree-count">{images.length}</span>
                  )}
                </button>
                {imagesOpen && (
                  <ul className="tree-items">
                    {images.map((image) => (
                      <li
                        className="image-item"
                        key={`${image.id}/${image.name}`}
                        title={`${image.id}\n${image.size}`}
                      >
                        <Layers size={14} />
                        <span>{image.name}</span>
                      </li>
                    ))}
                    {!images.length && !busy && (
                      <li className="tree-message">没有镜像</li>
                    )}
                  </ul>
                )}
                <button
                  className="tree-toggle"
                  aria-expanded={containersOpen}
                  onClick={() => toggle("containersOpen")}
                >
                  <ChevronRight
                    size={14}
                    className={containersOpen ? "expanded" : ""}
                  />
                  <span>容器</span>
                  {containersOpen && (
                    <span className="tree-count">{containers.length}</span>
                  )}
                </button>
                {containersOpen && (
                  <ul className="tree-items">
                    {containers.map((container) => (
                      <li className="container-item" key={container.id}>
                        <div
                          className="container-info"
                          title={`${container.id}\n${container.image}\n${container.status}`}
                        >
                          <span>{container.name}</span>
                          <small
                            className={
                              container.state === "running"
                                ? "running"
                                : "muted"
                            }
                          >
                            {containerStates[container.state] ||
                              container.state}
                          </small>
                        </div>
                        <div className="container-actions">
                          <button
                            className="container-button"
                            aria-label={`查看 ${container.name} 容器日志`}
                            disabled={
                              status.state !== "running" ||
                              busy ||
                              !!openingLogs
                            }
                            onClick={() => void viewLogs(container)}
                          >
                            <ScrollText size={13} />
                            {openingLogs === container.id ? "打开…" : "日志"}
                          </button>
                          {container.state === "running" && (
                            <button
                              className="container-button"
                              aria-label={`进入 ${container.name} 容器终端`}
                              disabled={
                                status.state !== "running" || busy || !!opening
                              }
                              onClick={() => void enter(container)}
                            >
                              <TerminalSquare size={13} />
                              {opening === container.id ? "连接…" : "终端"}
                            </button>
                          )}
                        </div>
                      </li>
                    ))}
                    {!containers.length && !busy && (
                      <li className="tree-message">没有容器</li>
                    )}
                  </ul>
                )}
              </>
            )}
            {error && (
              <p className="panel-error" role="alert">
                {error}
              </p>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
