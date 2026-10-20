import { useId } from "react";
import { restartPolicies } from "./createContainerDraft";
export function RestartPolicyFields({
  policy,
  count,
  onPolicy,
  onCount,
  error,
  onBlur,
  disabled = false,
}: {
  policy: string;
  count: string;
  onPolicy: (value: string) => void;
  onCount: (value: string) => void;
  error?: string;
  onBlur?: () => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <>
      <label className="docker-form-field">
        重启策略
        <select
          disabled={disabled}
          aria-label="重启策略"
          value={policy}
          onChange={(e) => onPolicy(e.target.value)}
        >
          {restartPolicies.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {policy === "on-failure" && (
        <label className="docker-form-field">
          最大重试次数
          <input
            disabled={disabled}
            data-field="maximumRetryCount"
            aria-label="最大重试次数"
            aria-invalid={!!error}
            aria-describedby={id}
            type="number"
            min="0"
            max="100000"
            value={count}
            onChange={(e) => onCount(e.target.value)}
            onBlur={onBlur}
          />
          <small id={id}>{error || "0 表示不限次数"}</small>
        </label>
      )}
    </>
  );
}
