import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { APP_VERSION } from "./brand";
import { errorOf } from "./types";
import { updateApi, updateBusy, type UpdatePreflight } from "./updateApi";
import { receiveUpdate, useAppUpdate } from "./useAppUpdate";
import "./software-update.css";
const phases = {
  idle: "尚未检查",
  checking: "正在检查更新…",
  upToDate: "已是最新版本",
  available: "发现新版本",
  downloading: "正在下载并验证更新…",
  ready: "更新已准备好",
  preparing: "正在准备安装…",
  installing: "正在安装，请等待…",
  failed: "更新未完成",
};
const size = (n: number) => (n / 1024 / 1024).toFixed(1) + " MB";
export function SoftwareUpdateSection() {
  const state = useAppUpdate();
  const [error, setError] = useState("");
  const [acting, setActing] = useState(false);
  const [preflight, setPreflight] = useState<UpdatePreflight | null>(null);
  const main = desktop && getCurrentWindow().label === "main";
  const busy = acting || (!!state && updateBusy(state));
  async function run(action: () => Promise<unknown>) {
    setError("");
    setActing(true);
    try {
      await action();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setActing(false);
    }
  }
  return (
    <section className="software-update" aria-label="软件更新" tabIndex={-1}>
      <h3>软件更新</h3>
      <p>
        当前版本 <strong>v{state?.currentVersion ?? APP_VERSION}</strong>
        {state?.version && (
          <>
            {" "}
            · 可用版本 <strong>v{state.version}</strong>
          </>
        )}
      </p>
      <p role="status" aria-live="polite">
        {state
          ? phases[state.phase]
          : desktop
            ? "正在读取更新状态…"
            : "请在桌面 App 中检查更新"}
      </p>
      {state?.lastCheck && (
        <p className="update-muted">
          上次成功检查：{new Date(state.lastCheck * 1000).toLocaleString()}
        </p>
      )}
      {state?.publishedAt && (
        <p className="update-muted">
          发布日期：{new Date(state.publishedAt).toLocaleDateString()}
        </p>
      )}
      {state?.notes && (
        <details>
          <summary>版本更新说明</summary>
          <div className="update-notes">{state.notes}</div>
        </details>
      )}
      {state?.phase === "downloading" && (
        <div>
          <progress
            aria-label="更新下载进度"
            max={state.total ?? undefined}
            value={state.total ? state.downloaded : undefined}
          />
          <p>
            {size(state.downloaded)}
            {state.total ? " / " + size(state.total) : " 已下载"} ·
            关闭此面板后继续下载
          </p>
        </div>
      )}
      {state?.message && <p role="status">{state.message}</p>}
      {error && (
        <p className="update-error" role="alert">
          {error}
        </p>
      )}
      {state && (
        <label className="update-preference">
          <input
            type="checkbox"
            checked={state.autoCheck}
            disabled={busy}
            onChange={(e) =>
              void run(async () =>
                receiveUpdate(
                  await updateApi.preferences(
                    e.target.checked,
                    state.skippedVersion,
                  ),
                ),
              )
            }
          />
          自动检查更新
        </label>
      )}
      <div className="update-actions">
        <button
          disabled={!desktop || busy || state?.phase === "ready"}
          onClick={() =>
            void run(async () => receiveUpdate(await updateApi.check()))
          }
        >
          检查更新
        </button>
        {!!state?.version && (
          <button
            disabled={busy}
            onClick={() => void run(() => updateApi.openRelease())}
          >
            打开发布页 / 手动下载
          </button>
        )}
        {state?.version &&
          state.automaticInstall &&
          !["ready", "preparing", "installing", "downloading"].includes(
            state.phase,
          ) &&
          main && (
            <button
              className="primary"
              disabled={busy}
              onClick={() =>
                void run(async () => receiveUpdate(await updateApi.download()))
              }
            >
              下载更新
            </button>
          )}
        {state?.phase === "ready" && main && (
          <button
            className="primary"
            disabled={busy}
            onClick={() =>
              void run(async () => setPreflight(await updateApi.preflight()))
            }
          >
            安装并重启
          </button>
        )}
        {state?.version && !main && (
          <button onClick={() => void run(() => updateApi.focusMain())}>
            前往主窗口更新
          </button>
        )}
        {state?.version && state.phase === "available" && (
          <button
            disabled={busy}
            onClick={() =>
              void run(async () =>
                receiveUpdate(
                  await updateApi.preferences(
                    state.autoCheck,
                    state.skippedVersion === state.version
                      ? null
                      : state.version,
                  ),
                ),
              )
            }
          >
            {state.skippedVersion === state.version
              ? "恢复版本提醒"
              : "跳过此版本"}
          </button>
        )}
      </div>
      {state?.version && <p className="update-muted">{state.installReason}</p>}
      {preflight && (
        <div className="update-confirm" role="group" aria-label="安装前检查">
          <strong>
            {preflight.blockers.length
              ? "安装前需要处理"
              : "确认安装并重启 SoDeck"}
          </strong>
          {preflight.blockers.length > 0 && (
            <ul>
              {preflight.blockers.map((x) => (
                <li key={x}>{x}</li>
              ))}
            </ul>
          )}
          {!preflight.blockers.length && (
            <>
              <p>
                安装将关闭所有窗口。已保存配置会保留，重启后需要重新连接服务器。
              </p>
              <ul>
                {preflight.interruptions.map((x) => (
                  <li key={x}>{x}</li>
                ))}
              </ul>
            </>
          )}
          <div className="update-actions">
            <button disabled={busy} onClick={() => setPreflight(null)}>
              稍后
            </button>
            {preflight.blockers.length ? (
              <button
                disabled={busy}
                onClick={() =>
                  void run(async () =>
                    setPreflight(await updateApi.preflight()),
                  )
                }
              >
                重新检查
              </button>
            ) : (
              <button
                className="primary"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await updateApi.install();
                    setPreflight(null);
                  })
                }
              >
                确认安装并重启
              </button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
