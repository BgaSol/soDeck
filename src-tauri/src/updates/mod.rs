//! Application-owned signed updates. Webviews never receive installer URLs or updater plugin permissions.
mod install;
mod manager;
mod model;
mod platform;
mod policy;
mod store;
#[cfg(test)]
mod tests;
mod transport;
use crate::{
    model::{AppError, AppResult},
    State,
};
use model::*;
pub use model::{InstallReview, InstallSubmission, PreferencePatch, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{mpsc, oneshot, watch};
const EVENT: &str = "app-update-changed";
const REPOSITORY: &str = "https://github.com/BgaSol/soDeck";
const DAY: u64 = 86_400;
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn error(e: impl std::fmt::Display) -> AppError {
    AppError::new("update", e)
}
pub struct UpdateManager {
    tx: mpsc::Sender<manager::Request>,
    view: Arc<Mutex<Snapshot>>,
    phase: Arc<AtomicU8>,
}
impl UpdateManager {
    pub fn installation_in_progress(&self) -> bool {
        self.phase.load(Ordering::SeqCst) == 2
    }
    pub fn cancel_preparation(&self) {
        // Synchronous close-path invalidation, even when the actor is busy saving.
        let _ = self
            .phase
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn stop(&self) {
        let _ = self.tx.try_send(manager::Request::Stop);
    }
    async fn request(&self, action: manager::Action) -> AppResult<manager::Reply> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .try_send(manager::Request::Action(action, tx))
            .map_err(|_| error("更新操作繁忙，请稍后重试。"))?;
        rx.await.map_err(|_| error("更新服务已停止。"))?
    }
    async fn snapshot_action(&self, action: manager::Action) -> AppResult<Snapshot> {
        match self.request(action).await? {
            manager::Reply::Snapshot(s) => Ok(s),
            _ => Err(error("更新响应无效。")),
        }
    }
}
pub fn initialize(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    manager::initialize(app)
}
#[tauri::command]
pub fn update_snapshot(manager: TauriState<'_, UpdateManager>) -> Snapshot {
    manager.view.lock().unwrap().clone()
}
#[tauri::command]
pub async fn update_check(manager: TauriState<'_, UpdateManager>) -> AppResult<Snapshot> {
    manager.snapshot_action(manager::Action::Check).await
}
#[tauri::command]
pub async fn update_download(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    release_id: String,
) -> AppResult<Snapshot> {
    crate::main_only(&window)?;
    manager
        .snapshot_action(manager::Action::Download(release_id))
        .await
}
#[tauri::command]
pub async fn update_cancel_download(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    operation_id: String,
) -> AppResult<Snapshot> {
    crate::main_only(&window)?;
    manager
        .snapshot_action(manager::Action::Cancel(operation_id))
        .await
}
#[tauri::command]
pub async fn update_preferences(
    manager: TauriState<'_, UpdateManager>,
    expected_revision: u64,
    patch: PreferencePatch,
) -> AppResult<Snapshot> {
    manager
        .snapshot_action(manager::Action::Preferences(expected_revision, patch))
        .await
}
#[tauri::command]
pub async fn update_skip_version(
    manager: TauriState<'_, UpdateManager>,
    release_id: String,
    skip: bool,
) -> AppResult<Snapshot> {
    manager
        .snapshot_action(manager::Action::Skip(release_id, skip))
        .await
}
#[tauri::command]
pub async fn update_preflight(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    release_id: String,
) -> AppResult<InstallReview> {
    crate::main_only(&window)?;
    match manager.request(manager::Action::Review(release_id)).await? {
        manager::Reply::Review(r) => Ok(r),
        _ => Err(error("更新响应无效。")),
    }
}
#[tauri::command]
pub async fn update_install(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    review_token: String,
) -> AppResult<InstallSubmission> {
    crate::main_only(&window)?;
    match manager
        .request(manager::Action::Install(review_token))
        .await?
    {
        manager::Reply::Submission(r) => Ok(r),
        _ => Err(error("更新响应无效。")),
    }
}
#[tauri::command]
pub async fn update_ack_notice(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    release_id: String,
) -> AppResult<Snapshot> {
    crate::main_only(&window)?;
    manager
        .snapshot_action(manager::Action::AckNotice(release_id))
        .await
}
#[tauri::command]
pub async fn update_ack_result(
    window: WebviewWindow,
    manager: TauriState<'_, UpdateManager>,
    result_id: String,
) -> AppResult<Snapshot> {
    crate::main_only(&window)?;
    manager
        .snapshot_action(manager::Action::AckResult(result_id))
        .await
}
#[tauri::command]
pub async fn update_open_manual_asset(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: TauriState<'_, UpdateManager>,
    release_id: String,
) -> AppResult<()> {
    crate::main_only(&window)?;
    use tauri_plugin_opener::OpenerExt;
    match manager
        .request(manager::Action::ManualAsset(release_id))
        .await?
    {
        manager::Reply::Url(url) => app
            .opener()
            .open_url(url, None::<String>)
            .map_err(|_| error("无法打开安装包页面。")),
        _ => Err(error("更新响应无效。")),
    }
}
#[tauri::command]
pub fn update_open_release(
    app: tauri::AppHandle,
    manager: TauriState<'_, UpdateManager>,
    release_id: Option<String>,
) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let view = manager.view.lock().unwrap().clone();
    let version = view
        .release
        .filter(|r| release_id.as_ref().is_none_or(|id| id == &r.id));
    let url = version
        .map(|r| format!("{REPOSITORY}/releases/tag/v{}", r.version))
        .unwrap_or_else(|| format!("{REPOSITORY}/releases"));
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|_| error("无法打开发布页面。"))
}
#[tauri::command]
pub fn update_restart_recovery(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: TauriState<'_, UpdateManager>,
) -> AppResult<()> {
    crate::main_only(&window)?;
    if !manager
        .view
        .lock()
        .unwrap()
        .error
        .as_ref()
        .is_some_and(|e| e.code == "restartRequired")
    {
        return Err(error("当前不需要恢复重启。"));
    }
    app.restart()
}
#[tauri::command]
pub fn update_focus_main(app: tauri::AppHandle) -> AppResult<()> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| error("找不到主窗口。"))?;
    main.unminimize().map_err(error)?;
    main.set_focus().map_err(error)?;
    main.emit("open-software-update", ()).map_err(error)
}
