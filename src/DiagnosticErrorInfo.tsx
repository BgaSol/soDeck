import { useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { AppError } from "./types";

export function DiagnosticErrorInfo({ error }: { error: AppError }) {
  const [notice, setNotice] = useState("");
  if (!error.traceId) return null;
  return (
    <details className="diagnostic-error-info">
      <summary>诊断信息</summary>
      <p>
        操作编号：<code>{error.traceId}</code>
        <br />
        错误类别：{error.kind}
        {error.nativeCode !== undefined ? ` · 系统码 ${error.nativeCode}` : ""}
      </p>
      <button
        type="button"
        onClick={() =>
          void writeText(error.traceId!)
            .then(() => setNotice("操作编号已复制"))
            .catch(() => setNotice("复制失败，请手动复制操作编号"))
        }
      >
        复制操作编号
      </button>
      <span role="status">{notice}</span>
    </details>
  );
}
