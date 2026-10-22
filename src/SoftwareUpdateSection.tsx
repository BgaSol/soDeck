import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "./api";
import { APP_VERSION } from "./brand";
import { errorOf } from "./types";
import { updateApi, type InstallReview } from "./updateApi";
import { receiveUpdate, useAppUpdate } from "./useAppUpdate";
import "./software-update.css";
import { phaseLabel, updateActions } from "./updatePresentation";
import { UpdateInstallReview } from "./UpdateInstallReview";

const size = (n: number) => (n / 1024 / 1024).toFixed(1) + " MB";
const mainWindow = () => desktop && getCurrentWindow().label === "main";

export function SoftwareUpdateSection() {
  const state = useAppUpdate();
  const [error, setError] = useState("");
  const [pending, setPending] = useState<Set<string>>(() => new Set());
  const [review, setReview] = useState<InstallReview | null>(null);
  const main = mainWindow();
  const actions = updateActions(state, main);

  useEffect(() => {
    if (state?.release && main)
      void updateApi
        .ackNotice(state.release.id)
        .then(receiveUpdate)
        .catch(() => {});
  }, [state?.release?.id, main]);

  async function run(key: string, action: () => Promise<unknown>) {
    setPending((keys) => new Set(keys).add(key));
    setError("");
    try {
      await action();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setPending((keys) => {
        const next = new Set(keys);
        next.delete(key);
        return next;
      });
    }
  }
  async function preflight() {
    if (!state?.release) return;
    const next = await updateApi.preflight(state.release.id);
    setReview(next);
  }
  async function install() {
    if (!review?.reviewToken) return;
    const result = await updateApi.install(review.reviewToken);
    if (result.status === "reviewChanged") setReview(result.review);
    else setReview(null);
  }
  const downloaded = state?.progress?.downloaded ?? 0;
  const total = state?.progress?.total ?? null;
  const skip =
    !!state?.release && state.skippedVersion === state.release.version;
  const canDownload = actions.download;
  const canCheck = desktop && actions.check;

  return (
    <section className="software-update" aria-label="软件更新" tabIndex={-1}>
      <h3>软件更新</h3>
      <p>
        当前版本 <strong>v{state?.currentVersion ?? APP_VERSION}</strong>
        {state?.release && (
          <>
            {" "}
            · 可用版本 <strong>v{state.release.version}</strong>
          </>
        )}
      </p>
      <p className="update-status" role="status" aria-live="polite">
        {state
          ? phaseLabel[state.phase]
          : desktop
            ? "正在读取更新状态"
            : "请在桌面 App 中检查更新"}
      </p>
      {state?.lastSuccessfulCheckAt && (
        <p className="update-muted">
          上次检查：
          {new Date(state.lastSuccessfulCheckAt * 1000).toLocaleString()}
        </p>
      )}
      {state?.release?.publishedAt && (
        <p className="update-muted">
          发布日期：{new Date(state.release.publishedAt).toLocaleDateString()}
        </p>
      )}
      {!!state?.release?.notes && (
        <details>
          <summary>版本更新说明</summary>
          <div className="update-notes">{state.release.notes}</div>
        </details>
      )}
      {(state?.phase === "downloading" || state?.phase === "verifying") && (
        <div className="update-progress">
          {state.phase === "downloading" && (
            <progress
              aria-label="更新下载进度"
              max={total ?? undefined}
              value={total ? downloaded : undefined}
            />
          )}
          <p>
            {state.phase === "downloading"
              ? `${size(downloaded)}${total ? ` / ${size(total)}` : " 已下载"} · 关闭此面板后会继续`
              : "下载完成，正在验证安装包"}
          </p>
        </div>
      )}
      {state?.message && !state.error && (
        <p className="update-muted">{state.message}</p>
      )}
      {state?.result && (
        <div className={`update-result ${state.result.outcome}`} role="status">
          <span>
            {state.result.outcome === "completed"
              ? `已更新到 v${state.result.targetVersion}`
              : `更新到 v${state.result.targetVersion} 未完成`}
          </span>
          <button
            disabled={!main || pending.has("ackResult")}
            onClick={() =>
              void run("ackResult", async () =>
                receiveUpdate(await updateApi.ackResult(state.result!.id)),
              )
            }
          >
            关闭
          </button>
        </div>
      )}
      {state && (
        <div className="update-preferences">
          <label>
            <input
              type="checkbox"
              checked={state.preferences.autoCheck}
              disabled={
                !desktop ||
                !actions.preferences ||
                pending.has("autoCheck") ||
                pending.has("autoDownload")
              }
              onChange={(e) =>
                void run("autoCheck", async () =>
                  receiveUpdate(
                    await updateApi.preferences(state.preferences.revision, {
                      autoCheck: e.target.checked,
                    }),
                  ),
                )
              }
            />
            自动检查更新
          </label>
          <label>
            <input
              type="checkbox"
              checked={state.preferences.autoDownload}
              disabled={
                !desktop ||
                !actions.preferences ||
                pending.has("autoCheck") ||
                pending.has("autoDownload")
              }
              onChange={(e) =>
                void run("autoDownload", async () =>
                  receiveUpdate(
                    await updateApi.preferences(state.preferences.revision, {
                      autoDownload: e.target.checked,
                    }),
                  ),
                )
              }
            />
            自动下载更新
          </label>
        </div>
      )}
      {error && (
        <p className="update-error" role="alert">
          {error}
        </p>
      )}
      {state?.error && (
        <p className="update-error" role="alert">
          {state.error.stage === "download"
            ? "下载或验证没有完成。"
            : (state.message ?? "更新没有完成。")}
        </p>
      )}
      <div className="update-actions">
        <button
          disabled={
            !canCheck || pending.has("check") || pending.has("download")
          }
          onClick={() =>
            void run("check", async () => {
              setReview(null);
              receiveUpdate(await updateApi.check());
            })
          }
        >
          检查更新
        </button>
        {state?.release && (
          <button
            disabled={pending.has("release")}
            onClick={() =>
              void run("release", () =>
                updateApi.openRelease(state.release!.id),
              )
            }
          >
            打开发布页
          </button>
        )}
        {state?.release && state.install.mode === "manual" && main && (
          <button
            className="primary"
            disabled={pending.has("manual")}
            onClick={() =>
              void run("manual", () =>
                updateApi.openManualAsset(state.release!.id),
              )
            }
          >
            下载 {state.install.manualFormat?.toUpperCase()} 安装包
          </button>
        )}
        {canDownload && (
          <button
            className="primary"
            disabled={pending.has("download") || pending.has("check")}
            onClick={() =>
              void run("download", async () => {
                setReview(null);
                receiveUpdate(await updateApi.download(state!.release!.id));
              })
            }
          >
            {state?.error?.retry === "download" ? "重新下载" : "下载更新"}
          </button>
        )}
        {(state?.phase === "downloading" || state?.phase === "verifying") &&
          state.operation &&
          main && (
            <button
              disabled={pending.has("cancel")}
              onClick={() =>
                void run("cancel", async () =>
                  receiveUpdate(
                    await updateApi.cancelDownload(state.operation!.id),
                  ),
                )
              }
            >
              取消下载
            </button>
          )}
        {state?.phase === "ready" &&
          main &&
          review?.releaseId !== state.release?.id && (
            <button
              className="primary"
              disabled={pending.has("preflight") || pending.has("install")}
              onClick={() => void run("preflight", preflight)}
            >
              安装并重启
            </button>
          )}
        {state?.release && state.phase === "available" && (
          <button
            disabled={pending.has("skip")}
            onClick={() =>
              void run("skip", async () => {
                receiveUpdate(
                  await updateApi.skipVersion(state.release!.id, !skip),
                );
              })
            }
          >
            {skip ? "恢复版本提醒" : "跳过此版本"}
          </button>
        )}
        {state?.release && !main && (
          <button
            disabled={pending.has("focus")}
            onClick={() => void run("focus", () => updateApi.focusMain())}
          >
            前往主窗口更新
          </button>
        )}
        {state?.error?.code === "restartRequired" && main && (
          <button
            className="primary"
            disabled={pending.has("recovery")}
            onClick={() =>
              void run("recovery", () => updateApi.restartRecovery())
            }
          >
            重启 SoDeck
          </button>
        )}
      </div>
      {state?.install.reason && state.release && (
        <p className="update-muted">{state.install.reason}</p>
      )}
      {review &&
        state?.phase === "ready" &&
        review.releaseId === state.release?.id && (
          <UpdateInstallReview
            review={review}
            pending={pending.has("preflight") || pending.has("install")}
            onLater={() => setReview(null)}
            onRefresh={() => void run("preflight", preflight)}
            onInstall={() => void run("install", install)}
          />
        )}
    </section>
  );
}
