import { useEffect, useState } from "react";
import { dockerApi } from "../dockerApi";
import { errorOf } from "../types";
export function ContainerDiagnostics({
  contextId,
  token,
  containerId,
  section,
}: {
  contextId: string;
  token: string;
  containerId: string;
  section: "configuration" | "processes";
}) {
  const [open, setOpen] = useState(false),
    [reveal, setReveal] = useState(false),
    [revision, setRevision] = useState(0);
  const [value, setValue] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    const hide = () => {
      if (document.hidden) {
        setOpen(false);
        setReveal(false);
        setValue("");
      }
    };
    document.addEventListener("visibilitychange", hide);
    return () => document.removeEventListener("visibilitychange", hide);
  }, []);
  useEffect(() => {
    setValue("");
    setError("");
    if (!open) {
      setReveal(false);
      return;
    }
    let alive = true;
    setBusy(true);
    void dockerApi
      .diagnostics(contextId, token, containerId, section, reveal)
      .then(
        (text) => {
          if (alive) setValue(text);
        },
        (e) => {
          if (alive) setError(errorOf(e).message);
        },
      )
      .finally(() => {
        if (alive) setBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [contextId, token, containerId, section, open, reveal, revision]);
  return (
    <details
      className="docker-detail-group"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary>
        {section === "configuration" ? "配置" : "进程"}
        <span>
          {section === "configuration" ? "命令、环境变量与标签" : "按需读取"}
        </span>
      </summary>
      {open && (
        <>
          <div className="docker-diagnostics-tools">
            {section === "configuration" && (
              <label>
                <input
                  type="checkbox"
                  checked={reveal}
                  onChange={(e) => setReveal(e.target.checked)}
                />
                显示完整配置与敏感值
              </label>
            )}
            <button
              className="resource-link"
              disabled={busy}
              onClick={() => setRevision((v) => v + 1)}
            >
              刷新
            </button>
          </div>
          {busy && (
            <p className="resource-help" role="status">
              正在读取…
            </p>
          )}
          {error && (
            <p className="resource-error" role="alert">
              {error}
            </p>
          )}
          {value && (
            <pre className="docker-raw-config" tabIndex={0}>
              {value}
            </pre>
          )}
        </>
      )}
    </details>
  );
}
