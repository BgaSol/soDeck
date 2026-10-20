import { useEffect, useRef, useState } from "react";
import { actionName, type DockerPreview } from "../dockerApi";
import { errorOf } from "../types";
import { previewExpired } from "./dockerActionFlow";
export function DockerConfirmationView({
  preview,
  onConfirm,
  onBack,
  onRefresh,
  uncertain = false,
  onRetry,
  disabled = false,
  backLabel = "取消",
  externalError = "",
}: {
  preview: DockerPreview;
  onConfirm: () => Promise<void>;
  onBack: () => void;
  onRefresh: () => Promise<void>;
  uncertain?: boolean;
  onRetry?: () => Promise<void>;
  disabled?: boolean;
  backLabel?: string;
  externalError?: string;
}) {
  const [busy, setBusy] = useState<"submit" | "refresh" | null>(null),
    [error, setError] = useState("");
  const [expired, setExpired] = useState(() => previewExpired(preview)),
    flight = useRef(false);
  useEffect(() => {
    setExpired(previewExpired(preview));
    setError("");
    const timer = window.setTimeout(
      () => setExpired(true),
      Math.max(0, preview.expiresAt - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [preview.id, preview.expiresAt]);
  async function run(kind: "submit" | "refresh") {
    if (flight.current) return;
    if (kind === "submit" && !uncertain && previewExpired(preview)) {
      setExpired(true);
      return;
    }
    flight.current = true;
    setBusy(kind);
    setError("");
    try {
      await (kind === "refresh"
        ? onRefresh()
        : uncertain && onRetry
          ? onRetry()
          : onConfirm());
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      flight.current = false;
      setBusy(null);
    }
  }
  const destructive =
    [
      "remove",
      "kill",
      "imageRemove",
      "volumeRemove",
      "networkRemove",
      "networkDisconnect",
      "cachePrune",
    ].includes(preview.action.type) ||
    (preview.action.type === "compose" && preview.action.mode === "down");
  return (
    <>
      <div className="docker-flow-body">
        <p className="docker-confirm-object">{preview.targetName}</p>
        <dl className="docker-facts">
          <dt>目标引擎</dt>
          <dd>{preview.engine.endpoint}</dd>
          <dt>
            {preview.action.type === "create"
              ? "容器名称"
              : preview.action.type === "compose"
                ? "项目"
                : preview.action.type.startsWith("image")
                  ? "镜像 ID"
                  : [
                        "volumeCreate",
                        "volumeRemove",
                        "networkCreate",
                        "networkRemove",
                        "networkConnect",
                        "networkDisconnect",
                        "cachePrune",
                      ].includes(preview.action.type)
                    ? "目标"
                    : "容器 ID"}
          </dt>
          <dd className="docker-id">{preview.targetId}</dd>
        </dl>
        {preview.consequences.map((text) => (
          <p key={text}>{text}</p>
        ))}

        {expired && !uncertain && (
          <p className="resource-help" role="status">
            确认已过期。重新检查后即可继续，当前设置会保留。
          </p>
        )}
        {uncertain && (
          <p className="resource-help" role="status">
            尚未收到提交结果。请核对原请求，避免重复操作。
          </p>
        )}
        {(error || externalError) && (
          <p className="resource-error" role="alert">
            {error || externalError}
          </p>
        )}
      </div>
      <footer className="dialog-actions">
        <button
          type="button"
          data-initial-focus
          disabled={!!busy || uncertain}
          onClick={onBack}
        >
          {backLabel}
        </button>
        {(expired || error || externalError) && !uncertain && (
          <button
            type="button"
            disabled={!!busy || disabled}
            onClick={() => void run("refresh")}
          >
            {busy === "refresh" ? "正在重新检查…" : "重新检查"}
          </button>
        )}
        <button
          type="button"
          className={destructive && !uncertain ? "danger" : "primary"}
          disabled={!!busy || disabled || (!uncertain && expired)}
          onClick={() => void run("submit")}
        >
          {busy === "submit"
            ? "正在核对…"
            : uncertain
              ? "核对提交结果"
              : "确认" + actionName(preview.action)}
        </button>
      </footer>
    </>
  );
}
