import { useEffect, useRef, useState } from "react";
import { AlertCircle, Check, LoaderCircle } from "lucide-react";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  actionName,
  healthName,
  operationActive,
  type DockerOperation,
} from "../dockerApi";
import { containerStateNames } from "../dockerPresentation";
import { errorOf } from "../types";
export function DockerOperations({
  endpoint,
  onContainer,
}: {
  endpoint: string;
  onContainer?: (id: string, name: string) => void;
}) {
  const { store, state } = useDockerManagement();
  const [error, setError] = useState("");
  const [older, setOlder] = useState<DockerOperation[]>([]);
  const [cursor, setCursor] = useState<string | null | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const scope = `${store.sessionId}/${endpoint}`;
  const current = useRef(scope),
    flight = useRef(false);
  current.current = scope;
  useEffect(() => {
    setOlder([]);
    setCursor(undefined);
    setError("");
    setLoading(false);
  }, [scope]);
  const combined = new Map(
    [...older, ...state.operations].map((task) => [task.id, task]),
  );
  const tasks = [...combined.values()]
    .filter((t) => t.engine.endpoint === endpoint)
    .sort((a, b) => b.createdAt - a.createdAt);
  const active = tasks.filter(operationActive).length,
    attention = tasks.filter((t) => t.phase === "unknown").length;
  if (!tasks.length) return null;
  async function reconcile(task: DockerOperation) {
    setError("");
    try {
      await dockerApi.reconcile(store.sessionId, task.id);
    } catch (e) {
      setError(errorOf(e).message);
    }
  }
  async function loadEarlier() {
    if (flight.current) return;
    flight.current = true;
    setLoading(true);
    setError("");
    try {
      const page = await dockerApi.history(
        store.sessionId,
        cursor ?? state.operations.at(-1)?.id ?? null,
      );
      if (current.current === scope) {
        setOlder((previous) => [...previous, ...page.operations]);
        setCursor(page.nextCursor);
      }
    } catch (e) {
      if (current.current === scope) setError(errorOf(e).message);
    } finally {
      flight.current = false;
      if (current.current === scope) setLoading(false);
    }
  }
  return (
    <details className="docker-operations">
      <summary>
        <span>
          {active ? (
            <LoaderCircle size={14} className="spinning" />
          ) : attention ? (
            <AlertCircle size={14} />
          ) : (
            <Check size={14} />
          )}
          操作记录
        </span>
        <span>
          {active
            ? `${active} 项进行中`
            : attention
              ? `${attention} 项待确认`
              : `${tasks.length} 项`}
        </span>
      </summary>
      <div className="docker-operation-list" aria-label="Docker 操作记录">
        <p className="resource-help">
          记录保留在本次 App 运行期间。关闭此面板后任务继续。
        </p>
        {error && (
          <p className="resource-error" role="alert">
            {error}
          </p>
        )}
        {tasks.map((task) => (
          <article
            className={`docker-operation phase-${task.phase}`}
            key={task.id}
            data-operation-id={task.id}
          >
            <header>
              <strong>
                {actionName(task.action)} · {task.targetName}
              </strong>
              <span>
                {operationActive(task)
                  ? "进行中"
                  : task.phase === "unknown"
                    ? "待确认"
                    : task.phase === "failed"
                      ? "失败"
                      : task.phase === "acknowledged"
                        ? "已知晓"
                        : "已完成"}
              </span>
            </header>
            <p role={operationActive(task) ? "status" : undefined}>
              {task.message}
            </p>
            {task.observedState && (
              <p className="resource-help">
                观察状态：
                {containerStateNames[task.observedState] ??
                  (task.observedState === "removed"
                    ? "已移除"
                    : task.observedState)}
                {task.observedHealth && ` · ${healthName(task.observedHealth)}`}
              </p>
            )}
            {task.createdContainerId && onContainer && (
              <button
                className="resource-link"
                onClick={() =>
                  onContainer(task.createdContainerId!, task.targetName)
                }
              >
                查看已创建的容器
              </button>
            )}
            {!!task.items?.length && (
              <details className="docker-item-results">
                <summary>逐项结果 · {task.items.length}</summary>
                {task.items.map((item, i) => (
                  <div key={i}>
                    <strong>{item.name}</strong>
                    <span>
                      {item.phase === "completed"
                        ? "完成"
                        : item.phase === "failed"
                          ? "失败"
                          : item.phase === "unknown"
                            ? "待确认"
                            : "进行中"}{" "}
                      · {item.message}
                    </span>
                  </div>
                ))}
              </details>
            )}
            <time>{new Date(task.createdAt).toLocaleTimeString()}</time>
            {task.phase === "unknown" && (
              <div className="docker-operation-actions">
                <button
                  disabled={task.reconciling}
                  onClick={() => void reconcile(task)}
                >
                  {task.reconciling ? "正在核对…" : "核对当前状态"}
                </button>
                <details>
                  <summary>继续管理此对象</summary>
                  <p>
                    原命令仍可能在远端完成。请先核对当前状态，解除锁定后才能发起新的操作。
                  </p>
                  <button
                    disabled={task.reconciling || !task.observedState}
                    onClick={() => {
                      void dockerApi
                        .acknowledge(store.sessionId, task.id)
                        .catch((e) => setError(errorOf(e).message));
                    }}
                  >
                    已知晓，解除操作锁
                  </button>
                </details>
              </div>
            )}
          </article>
        ))}
        {cursor !== null && (
          <button
            className="resource-link"
            disabled={loading}
            onClick={() => void loadEarlier()}
          >
            {loading ? "正在读取…" : "读取更早记录"}
          </button>
        )}
      </div>
    </details>
  );
}
