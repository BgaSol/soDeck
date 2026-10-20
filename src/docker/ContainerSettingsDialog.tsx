import { useRef, useState } from "react";
import type { ContainerDetail, DockerActionPayload } from "../dockerApi";
import { errorOf } from "../types";
import { RestartPolicyFields } from "./RestartPolicyFields";
import {
  validateRestartPolicy,
  validContainerName,
} from "./createContainerDraft";
import { DockerDialogFrame } from "./DockerDialogFrame";
export function ContainerSettingsDialog({
  mode,
  detail,
  onSubmit,
  onClose,
  uncertain = false,
  onRetry,
}: {
  mode: "rename" | "restartPolicy";
  detail: ContainerDetail;
  onSubmit: (action: DockerActionPayload) => Promise<void>;
  onClose: () => void;
  uncertain?: boolean;
  onRetry?: () => Promise<void>;
}) {
  const pending = useRef(false),
    [name, setName] = useState(detail.name),
    [policy, setPolicy] = useState(detail.restartPolicy || "no"),
    [count, setCount] = useState(String(detail.restartMaximum));
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  async function submit() {
    if (pending.current) return;
    if (uncertain) {
      if (!onRetry) return;
      pending.current = true;
      setBusy(true);
      setError("");
      try {
        await onRetry();
        onClose();
      } catch (e) {
        setError(errorOf(e).message);
      } finally {
        pending.current = false;
        setBusy(false);
      }
      return;
    }
    if (mode === "rename" && !validContainerName(name)) {
      setError(
        "名称须以字母或数字开头，只能包含字母、数字、下划线、点和连字符。",
      );
      return;
    }
    const issue =
      mode === "restartPolicy" ? validateRestartPolicy(policy, count) : null;
    if (issue) {
      setError(issue);
      return;
    }
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      await onSubmit(
        mode === "rename"
          ? { type: "rename", name }
          : {
              type: "restartPolicy",
              policy,
              maximumRetryCount: policy === "on-failure" ? Number(count) : 0,
            },
      );
      onClose();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <DockerDialogFrame
      title={mode === "rename" ? "重命名容器" : "重启策略"}
      onClose={onClose}
      busy={busy || uncertain}
      onSubmit={() => void submit()}
      footer={
        <>
          <button type="button" disabled={busy || uncertain} onClick={onClose}>
            取消
          </button>
          <button className="primary" disabled={busy}>
            {busy ? "正在核对…" : uncertain ? "核对提交结果" : "保存"}
          </button>
        </>
      }
    >
      <p>{detail.name}</p>
      {mode === "rename" ? (
        <label className="docker-form-field">
          容器名称
          <input
            disabled={uncertain}
            data-initial-focus
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={255}
          />
        </label>
      ) : (
        <>
          <RestartPolicyFields
            disabled={uncertain}
            policy={policy}
            count={count}
            onPolicy={setPolicy}
            onCount={setCount}
          />
          <p className="resource-help">
            由 Compose
            管理的容器应同时修改项目配置，否则下次重建可能覆盖此设置。
          </p>
        </>
      )}
      {uncertain && (
        <p className="resource-help" role="status">
          尚未收到提交结果，请核对原请求。
        </p>
      )}
      {error && (
        <p className="resource-error" role="alert">
          {error}
        </p>
      )}
    </DockerDialogFrame>
  );
}
