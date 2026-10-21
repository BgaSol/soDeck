import { useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  diagnosticsApi,
  type DiagnosticsScope,
  type DiagnosticsStatus,
} from "./diagnosticsApi";
import { errorOf } from "./types";
import "./diagnostics.css";

export function DiagnosticsSection() {
  const [status, setStatus] = useState<DiagnosticsStatus>();
  const [scope, setScope] = useState<DiagnosticsScope>("recent");
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [saved, setSaved] = useState(false);
  const desktop = isTauri();
  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    const refresh = () =>
      diagnosticsApi
        .status()
        .then((s) => {
          if (alive) setStatus(s);
        })
        .catch((e) => {
          if (alive) setError(errorOf(e).message);
        });
    void refresh();
    const timer = setInterval(
      () => void refresh(),
      busy === "export" ? 700 : 5000,
    );
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [busy, desktop]);
  async function perform(name: string, action: () => Promise<void>) {
    if (busy) return;
    setBusy(name);
    setError("");
    setNotice("");
    try {
      await action();
      setStatus(await diagnosticsApi.status());
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy("");
    }
  }
  const unavailable = !desktop || !status || status.unavailable;
  return (
    <section className="diagnostics-settings" aria-label="本地诊断">
      <h3>本地诊断</h3>
      <p>
        自动记录应用运行过程和错误，仅保存在此设备。可直接拷贝整个日志目录，或打包后提供给开发者。
      </p>
      <dl>
        <div>
          <dt>记录状态</dt>
          <dd>
            {!desktop
              ? "仅桌面应用可用"
              : !status
                ? "正在读取…"
                : status.unavailable
                  ? "记录不可用"
                  : status.error || status.incomplete
                    ? "部分记录缺失"
                    : status.fallback
                      ? "使用临时目录"
                      : "正常"}
          </dd>
        </div>
        {status && (
          <>
            <div>
              <dt>当前运行</dt>
              <dd>{new Date(status.startedAt).toLocaleString()}</dd>
            </div>
            <div>
              <dt>已保留</dt>
              <dd>
                {status.runs} 次运行 · {(status.bytes / 1048576).toFixed(1)} MiB
              </dd>
            </div>
          </>
        )}
      </dl>
      <p>保留最近 7 天，最多 100 MiB；达到任一限制即清理旧记录。</p>
      {status?.directory && (
        <div className="diagnostics-path">
          <code>{status.directory}</code>
          <button
            type="button"
            disabled={!!busy}
            onClick={() =>
              void perform("copy", async () => {
                await writeText(status.directory);
                setNotice("日志路径已复制。");
              })
            }
          >
            复制路径
          </button>
        </div>
      )}
      <div className="diagnostics-actions">
        <button
          type="button"
          disabled={unavailable || !!busy}
          onClick={() => void perform("open", diagnosticsApi.open)}
        >
          打开日志目录
        </button>
        <label>
          打包范围
          <select
            aria-label="日志打包范围"
            disabled={unavailable || !!busy}
            value={scope}
            onChange={(e) => setScope(e.target.value as DiagnosticsScope)}
          >
            <option value="recent">最近 24 小时及最近异常运行</option>
            <option value="current">本次运行</option>
            <option value="all">全部保留日志</option>
          </select>
        </label>
        <button
          type="button"
          disabled={unavailable || !!busy}
          onClick={() =>
            void perform("export", async () => {
              setSaved(false);
              const result = await diagnosticsApi.export(scope);
              if (result) {
                setSaved(true);
                setNotice(
                  `已保存：${result.path}${result.warnings ? `。包含 ${result.warnings} 项完整性说明，请查看 manifest.json。` : ""}`,
                );
              } else setNotice("已取消保存。");
            })
          }
        >
          打包日志…
        </button>
        <button
          type="button"
          disabled={unavailable || !!busy}
          onClick={() => setConfirm(true)}
        >
          清理日志
        </button>
      </div>
      {busy === "export" && (
        <div role="status">
          <p>
            {status?.exportPhase === "compressing"
              ? "正在生成 ZIP…"
              : "正在准备日志…"}
          </p>
          {!!status?.exportTotal && (
            <progress max={status.exportTotal} value={status.exportCompleted} />
          )}
          <button
            type="button"
            onClick={() =>
              void diagnosticsApi
                .cancel()
                .catch((e) => setError(errorOf(e).message))
            }
          >
            取消打包
          </button>
        </div>
      )}
      {confirm && (
        <div className="diagnostics-confirm">
          <p>
            清理应用管理的诊断记录？当前操作的旧上下文会被删除。终端历史、容器日志和已导出的文件会保留。
          </p>
          <button
            type="button"
            disabled={!!busy}
            onClick={() => setConfirm(false)}
          >
            保留日志
          </button>
          <button
            type="button"
            disabled={!!busy}
            onClick={() =>
              void perform("clear", async () => {
                await diagnosticsApi.clear();
                setConfirm(false);
                setNotice("诊断记录已清理，新的运行记录会继续保存。");
              })
            }
          >
            确认清理
          </button>
        </div>
      )}
      {(error || status?.error) && (
        <p className="panel-error" role="alert">
          {error || status?.error}
        </p>
      )}
      <p role="status" className="diagnostics-notice">
        {notice}
      </p>
      {saved && (
        <button
          type="button"
          onClick={() => void perform("reveal", diagnosticsApi.reveal)}
        >
          在文件夹中显示日志包
        </button>
      )}
    </section>
  );
}
