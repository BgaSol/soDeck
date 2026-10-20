//! Application-owned updater. No plugin IPC permissions are granted to webviews.
use crate::{
    model::{AppError, AppResult},
    State,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

const EVENT: &str = "app-update-changed";
const RELEASES: &str = "https://github.com/BgaSol/soDeck/releases";
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    auto_check: bool,
    skipped_version: Option<String>,
    last_check: Option<u64>,
    next_check: u64,
    pending_version: Option<String>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            auto_check: true,
            skipped_version: None,
            last_check: None,
            next_check: 0,
            pending_version: None,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u64,
    phase: String,
    current_version: String,
    version: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    message: Option<String>,
    automatic_install: bool,
    install_reason: String,
    auto_check: bool,
    skipped_version: Option<String>,
    last_check: Option<u64>,
}
struct Core {
    view: Snapshot,
    saved: Saved,
    update: Option<Update>,
    bytes: Option<Arc<Vec<u8>>>,
    failures: usize,
    load_error: Option<String>,
}
pub struct UpdateManager {
    path: PathBuf,
    core: Mutex<Core>,
}
fn persist(path: &std::path::Path, saved: &Saved) -> AppResult<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| error("更新状态路径无效"))?)
            .map_err(error)?;
    file.write_all(&serde_json::to_vec_pretty(saved).map_err(error)?)
        .map_err(error)?;
    file.as_file().sync_all().map_err(error)?;
    file.persist(path).map_err(error)?;
    Ok(())
}
fn install_support() -> (bool, String) {
    use tauri::utils::{config::BundleType, platform::bundle_type};
    if cfg!(debug_assertions) {
        return (false, "开发构建仅检查更新，请使用正式安装包升级。".into());
    }
    let supported = matches!(
        bundle_type(),
        Some(BundleType::Nsis | BundleType::App | BundleType::AppImage)
    );
    (
        supported,
        if supported {
            "安装将关闭 SoDeck 的所有窗口并重启。"
        } else {
            "此安装方式请下载对应安装包后手动安装（MSI / DEB 或未识别来源）。"
        }
        .into(),
    )
}
fn update_target() -> AppResult<String> {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "windows" if arch == "x86_64" => Ok("windows-x86_64-nsis".into()),
        "macos" if matches!(arch, "x86_64" | "aarch64") => Ok(format!("darwin-{arch}")),
        "linux" if arch == "x86_64" => Ok("linux-x86_64".into()),
        _ => Err(error("当前平台没有对应的升级包，请查看发布页面。")),
    }
}
fn stable_newer(current: &str, next: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(next),
    ) {
        (Ok(a), Ok(b)) => b.pre.is_empty() && b > a,
        _ => false,
    }
}
fn trusted_package(version: &str, url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .path()
            .starts_with(&format!("/BgaSol/soDeck/releases/download/v{version}/"))
        && url.query().is_none()
        && url.fragment().is_none()
}
impl UpdateManager {
    fn publish(&self, app: &tauri::AppHandle, change: impl FnOnce(&mut Core)) -> Snapshot {
        let mut c = self.core.lock().unwrap();
        change(&mut c);
        c.view.revision += 1;
        c.view.auto_check = c.saved.auto_check;
        c.view.skipped_version = c.saved.skipped_version.clone();
        c.view.last_check = c.saved.last_check;
        let view = c.view.clone();
        let _ = app.emit(EVENT, &view);
        view
    }
    pub fn installation_in_progress(&self) -> bool {
        matches!(
            self.core.lock().unwrap().view.phase.as_str(),
            "preparing" | "installing"
        )
    }
    fn fail(&self, app: &tauri::AppHandle, e: &AppError) {
        self.publish(app, |c| {
            if c.view.phase == "checking" {
                c.saved.next_check = now() + 900;
                let _ = persist(&self.path, &c.saved);
            }
            c.view.phase = "failed".into();
            c.view.message = Some(e.message.clone());
        });
    }
}
fn busy(phase: &str) -> bool {
    matches!(
        phase,
        "checking" | "downloading" | "preparing" | "installing"
    )
}

pub fn initialize(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join("update-state.json");
    let directory_error = std::fs::create_dir_all(path.parent().unwrap())
        .err()
        .map(|e| format!("无法创建更新设置目录：{e}"));
    let (mut saved, mut load_error) = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Saved>(&bytes) {
            Ok(saved) => (saved, None),
            Err(e) => (
                Saved::default(),
                Some(format!("无法读取更新设置：{e}；请检查 update-state.json。")),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Saved::default(), None),
        Err(e) => (Saved::default(), Some(format!("无法读取更新设置：{e}"))),
    };
    if load_error.is_none() {
        load_error = directory_error;
    }
    let current = app.package_info().version.to_string();
    let message = load_error.clone().or_else(|| {
        saved.pending_version.as_ref().map(|v| {
            if current == *v || stable_newer(v, &current) {
                format!("已更新到 v{current}")
            } else {
                format!("上次更新到 v{v} 未完成，可以重新检查并下载。")
            }
        })
    });
    if load_error.is_none() && saved.pending_version.is_some() {
        let mut acknowledged = saved.clone();
        acknowledged.pending_version = None;
        if persist(&path, &acknowledged).is_ok() {
            saved = acknowledged;
        }
    }
    let (automatic_install, install_reason) = install_support();
    app.manage(UpdateManager {
        path,
        core: Mutex::new(Core {
            view: Snapshot {
                revision: 0,
                phase: "idle".into(),
                current_version: current,
                version: None,
                notes: None,
                published_at: None,
                downloaded: 0,
                total: None,
                message,
                automatic_install,
                install_reason,
                auto_check: saved.auto_check,
                skipped_version: saved.skipped_version.clone(),
                last_check: saved.last_check,
            },
            saved,
            update: None,
            bytes: None,
            failures: 0,
            load_error,
        }),
    });
    // One process-wide scheduler, independent of webview lifetime.
    if !cfg!(debug_assertions) {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            loop {
                let manager = app.state::<UpdateManager>();
                let due = {
                    let c = manager.core.lock().unwrap();
                    c.load_error.is_none()
                        && c.saved.auto_check
                        && now() >= c.saved.next_check
                        && !busy(&c.view.phase)
                        && c.bytes.is_none()
                };
                if due {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = check(&app, false).await;
                    });
                }
                std::thread::sleep(Duration::from_secs(60));
            }
        });
    }
    Ok(())
}
#[tauri::command]
pub fn update_snapshot(manager: TauriState<'_, UpdateManager>) -> Snapshot {
    manager.core.lock().unwrap().view.clone()
}

async fn check(app: &tauri::AppHandle, manual: bool) -> AppResult<Snapshot> {
    let manager = app.state::<UpdateManager>();
    {
        let mut c = manager.core.lock().unwrap();
        if busy(&c.view.phase) || c.bytes.is_some() {
            return Ok(c.view.clone());
        }
        if let Some(e) = &c.load_error {
            return Err(error(e));
        }
        c.view.phase = "checking".into();
        c.view.message = None;
        c.update = None;
        c.view.version = None;
        c.view.notes = None;
        c.view.published_at = None;
    }
    manager.publish(app, |_| {});
    let result = async {
        app.updater_builder()
            .target(update_target()?)
            .timeout(Duration::from_secs(30))
            .version_comparator(|current, release| {
                stable_newer(&current.to_string(), &release.version.to_string())
            })
            .build()
            .map_err(error)?
            .check()
            .await
            .map_err(error)
    }
    .await;
    match result {
        Ok(mut update) => {
            if let Some(u) = &mut update {
                u.timeout = Some(Duration::from_secs(1800));
            }
            if let Some(u) = &update {
                if !trusted_package(&u.version, &u.download_url) {
                    let e = error("更新包地址不属于此版本的官方 Release，已拒绝下载。");
                    manager.fail(app, &e);
                    return Err(e);
                }
            }
            let result: AppResult<()> = (|| {
                let mut c = manager.core.lock().unwrap();
                let mut saved = c.saved.clone();
                saved.last_check = Some(now());
                saved.next_check = now() + DAY;
                // Acknowledge startup result after it has been made available to the UI.
                saved.pending_version = None;
                persist(&manager.path, &saved)?;
                c.saved = saved;
                c.failures = 0;
                c.view.version = update.as_ref().map(|u| u.version.clone());
                c.view.notes = update.as_ref().and_then(|u| u.body.clone());
                c.view.published_at = update.as_ref().and_then(|u| {
                    u.raw_json
                        .get("pub_date")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                });
                c.view.phase = if update.is_some() {
                    "available"
                } else {
                    "upToDate"
                }
                .into();
                c.view.message = if !manual
                    && c.view.version.is_some()
                    && c.view.version == c.saved.skipped_version
                {
                    Some("此版本已跳过自动提醒，仍可手动下载。".into())
                } else {
                    None
                };
                c.update = update;
                Ok(())
            })();
            if let Err(e) = result {
                manager.fail(app, &e);
                return Err(e);
            }
            Ok(manager.publish(app, |_| {}))
        }
        Err(e) => {
            manager.publish(app, |c| {
                c.failures += 1;
                c.saved.next_check = now()
                    + match c.failures {
                        1 => 900,
                        2 => 3600,
                        _ => 21600,
                    };
                let _ = persist(&manager.path, &c.saved);
                c.view.phase = "failed".into();
                c.view.message = Some(format!(
                    "检查更新失败：{}。请重试或打开发布页面。",
                    e.message
                ));
            });
            Err(e)
        }
    }
}
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle) -> AppResult<Snapshot> {
    check(&app, true).await
}
#[tauri::command]
pub fn update_preferences(
    app: tauri::AppHandle,
    manager: TauriState<'_, UpdateManager>,
    auto_check: bool,
    skipped_version: Option<String>,
) -> AppResult<Snapshot> {
    {
        let mut c = manager.core.lock().unwrap();
        if let Some(e) = &c.load_error {
            return Err(error(e));
        }
        if skipped_version.is_some() && skipped_version != c.view.version {
            return Err(error("只能跳过当前检测到的版本。"));
        }
        let mut saved = c.saved.clone();
        saved.auto_check = auto_check;
        saved.skipped_version = skipped_version;
        persist(&manager.path, &saved)?;
        c.saved = saved;
    }
    Ok(manager.publish(&app, |_| {}))
}
#[tauri::command]
pub async fn update_download(window: WebviewWindow, app: tauri::AppHandle) -> AppResult<Snapshot> {
    crate::main_only(&window)?;
    let manager = app.state::<UpdateManager>();
    let update = {
        let mut c = manager.core.lock().unwrap();
        if !c.view.automatic_install {
            return Err(error(&c.view.install_reason));
        }
        if busy(&c.view.phase) || c.bytes.is_some() {
            return Ok(c.view.clone());
        }
        let update = c.update.clone().ok_or_else(|| error("请先检查更新。"))?;
        c.view.phase = "downloading".into();
        c.view.downloaded = 0;
        c.view.total = None;
        c.view.message = None;
        update
    };
    manager.publish(&app, |_| {});
    let mut downloaded = 0u64;
    let mut last = std::time::Instant::now();
    match update
        .download(
            |size, total| {
                downloaded += size as u64;
                if last.elapsed() >= Duration::from_millis(150) {
                    manager.publish(&app, |c| {
                        c.view.downloaded = downloaded;
                        c.view.total = total;
                    });
                    last = std::time::Instant::now();
                }
            },
            || {},
        )
        .await
    {
        Ok(bytes) => Ok(manager.publish(&app, |c| {
            c.view.downloaded = bytes.len() as u64;
            c.view.total = Some(bytes.len() as u64);
            c.bytes = Some(Arc::new(bytes));
            c.view.phase = "ready".into();
        })),
        Err(e) => {
            let e = error(format!("更新下载或签名验证失败：{e}；未执行安装。"));
            manager.fail(&app, &e);
            Err(e)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    blockers: Vec<String>,
    interruptions: Vec<String>,
}
fn preflight(state: &State) -> AppResult<Preflight> {
    let mut blockers = Vec::new();
    let mut interruptions = Vec::new();
    // Require editors to be closed: asynchronous dirty notifications alone cannot
    // protect against a keystroke racing the install confirmation.
    if state.files.blocks_update() {
        blockers.push("请保存并关闭所有远程文件编辑窗口，再安装更新。".into());
    }
    let tasks = crate::file_tasks::snapshot(state)?;
    let count = tasks.iter().filter(|t| t.blocks_update()).count();
    if count > 0 {
        blockers.push(format!(
            "{count} 个文件任务尚未结束或结果待确认，请在任务面板处理。"
        ));
    }
    if count == 0 && state.transfers.has_activity() {
        blockers.push("文件操作仍在收尾，请等待完成后重新检查。".into());
    }
    let docker = state.docker.summary();
    if docker.active + docker.attention > 0 {
        blockers.push(format!(
            "{} 个 Docker 操作执行中或结果待确认，请在 Docker 面板处理。",
            docker.active + docker.attention
        ));
    }
    if state.netbird.blocks_update()? {
        blockers.push("NetBird 操作执行中或结果待确认，请在 NetBird 面板核对。".into());
    }
    let sessions = state
        .terminals
        .lock()
        .unwrap()
        .values()
        .filter(|t| !t.stream.lock().unwrap().closed)
        .count();
    if sessions > 0 {
        interruptions.push(format!("断开 {sessions} 个终端会话"));
    }
    let logs = state.logs.active_count();
    if logs > 0 {
        interruptions.push(format!("停止 {logs} 个日志接收窗口"));
    }
    let forwards = state.port_forwarding.summary();
    if forwards.active > 0 {
        interruptions.push(format!(
            "停止 {} 条端口转发（{} 条业务连接）",
            forwards.active, forwards.connections
        ));
    }
    Ok(Preflight {
        blockers,
        interruptions,
    })
}
#[tauri::command]
pub fn update_preflight(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<Preflight> {
    crate::main_only(&window)?;
    preflight(&state)
}
#[tauri::command]
pub async fn update_install(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    confirmed: bool,
) -> AppResult<()> {
    crate::main_only(&window)?;
    if !confirmed {
        return Err(error("请先确认安装并重启。"));
    }
    let manager = app.state::<UpdateManager>();
    let (update, bytes) = {
        let mut c = manager.core.lock().unwrap();
        if busy(&c.view.phase) {
            return Err(error("更新任务正在运行。"));
        }
        if !c.view.automatic_install {
            return Err(error(&c.view.install_reason));
        }
        let u = c.update.clone().ok_or_else(|| error("请先下载更新。"))?;
        let b = c.bytes.clone().ok_or_else(|| error("请先下载更新。"))?;
        c.view.phase = "preparing".into();
        c.view.message = None;
        (u, b)
    };
    manager.publish(&app, |_| {});
    let mut acquired_shutdown = false;
    let result: AppResult<()> = async {
        update.verify(&bytes).map_err(error)?;
        {
            let _gate = state.configuration.lock().unwrap();
            state.ensure_running()?;
            let status = preflight(&state)?;
            if !status.blockers.is_empty() {
                return Err(error(status.blockers.join("\n")));
            }
            state.stopping.store(true, Ordering::SeqCst);
            acquired_shutdown = true;
        }
        // Persist before any resource is stopped; a failed write must not exit.
        {
            let mut c = manager.core.lock().unwrap();
            let mut saved = c.saved.clone();
            saved.pending_version = Some(update.version.clone());
            persist(&manager.path, &saved)?;
            c.saved = saved;
        }
        crate::shutdown_work(state.inner().clone()).await?;
        manager.publish(&app, |c| c.view.phase = "installing".into());
        crate::blocking(move || update.install(bytes.as_ref()).map_err(error)).await?;
        app.restart();
    }
    .await;
    if let Err(e) = &result {
        if acquired_shutdown {
            state.stopping.store(false, Ordering::SeqCst);
        }
        manager.publish(&app, |c| {
            c.view.phase = "ready".into();
            c.view.message = Some(format!(
                "安装未完成：{}。已断开的会话需要重新连接。",
                e.message
            ));
        });
    }
    result
}
#[tauri::command]
pub fn update_open_release(
    app: tauri::AppHandle,
    manager: TauriState<'_, UpdateManager>,
) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let version = manager.core.lock().unwrap().view.version.clone();
    let url = match version.filter(|v| semver::Version::parse(v).is_ok()) {
        Some(v) => format!("{RELEASES}/tag/v{v}"),
        None => RELEASES.into(),
    };
    app.opener().open_url(url, None::<String>).map_err(error)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_preflight_observes_file_activity_and_shutdown_gate() {
        let state = crate::connection::tests::state();
        assert!(preflight(&state).unwrap().blockers.is_empty());
        let mutation = state
            .files
            .begin_mutation("server", &["/file".into()], "编辑")
            .unwrap();
        assert!(!preflight(&state).unwrap().blockers.is_empty());
        drop(mutation);
        assert!(preflight(&state).unwrap().blockers.is_empty());
        state.stopping.store(true, Ordering::SeqCst);
        assert!(state.ensure_running().is_err());
    }
    #[test]
    fn stable_versions_only() {
        assert!(stable_newer("0.1.9", "0.1.10"));
        assert!(!stable_newer("0.2.0", "0.1.10"));
        assert!(!stable_newer("0.1.0", "0.2.0-beta.1"));
        assert!(!stable_newer("0.1.0", "0.1.0"));
    }
    #[test]
    fn package_urls_are_pinned_to_official_version() {
        let check = |u| trusted_package("0.2.0", &tauri::Url::parse(u).unwrap());
        assert!(check(
            "https://github.com/BgaSol/soDeck/releases/download/v0.2.0/setup.exe"
        ));
        for u in [
            "http://github.com/BgaSol/soDeck/releases/download/v0.2.0/setup.exe",
            "https://github.com/BgaSol/soDeck/releases/download/v0.1.0/setup.exe",
            "https://example.com/BgaSol/soDeck/releases/download/v0.2.0/setup.exe",
        ] {
            assert!(!check(u));
        }
    }
    #[test]
    fn persisted_preferences_and_failed_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let saved = Saved {
            auto_check: false,
            skipped_version: Some("0.2.0".into()),
            ..Default::default()
        };
        persist(&path, &saved).unwrap();
        let loaded: Saved = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(!loaded.auto_check);
        assert_eq!(loaded.skipped_version, saved.skipped_version);
        assert!(persist(&dir.path().join("missing/state.json"), &saved).is_err());
        let defaults: Saved = serde_json::from_str("{}").unwrap();
        assert!(defaults.auto_check);
    }
}
