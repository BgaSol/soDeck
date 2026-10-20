mod collector;
mod diagnostics;
#[cfg(test)]
mod engine_tests;
mod export;
mod lifecycle;
mod protocol;
pub(crate) mod replacements;
mod snapshot;
use collector::collect_logs;
#[cfg(test)]
use collector::{command, read_logs, retry_delay};
use protocol::{ContainerObservation, LogEvent};
#[cfg(test)]
fn retryable(e: &AppError) -> bool {
    protocol::classify(e).retry(2)
}
use crate::connection::{self, SharedContext};
use crate::ssh_transport::Cancellation;
use crate::{
    log_store::{self, Source},
    model::{AppError, AppResult},
    ssh, State,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    ipc::Channel, Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

#[derive(Default)]
pub struct LogState {
    archive_window: Mutex<()>,
    windows: Mutex<HashMap<String, LogWindow>>,
    workers: Mutex<Vec<Run>>,
    snapshots: Mutex<HashMap<String, (String, Arc<snapshot::Snapshot>)>>,
    jobs: Mutex<HashMap<String, (String, Cancellation)>>,
}
struct LogWindow {
    spec: Option<SharedContext>,
    target: Option<crate::docker::ContainerTarget>,
    source: Source,
    run: Option<Run>,
}
#[derive(Clone)]
struct Run {
    id: String,
    source_id: String,
    cancel: Cancellation,
    done: Arc<(Mutex<bool>, Condvar)>,
    wake: Arc<(Mutex<bool>, Condvar)>,
    revision: Arc<std::sync::atomic::AtomicU64>,
    terminal: Arc<std::sync::atomic::AtomicBool>,
    observation: Arc<Mutex<Option<ContainerObservation>>>,
}
impl Run {
    fn new(source_id: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_id,
            cancel: Cancellation::default(),
            done: Arc::new((Mutex::new(false), Condvar::new())),
            wake: Arc::new((Mutex::new(false), Condvar::new())),
            revision: Default::default(),
            terminal: Default::default(),
            observation: Default::default(),
        }
    }
    fn emit(
        &self,
        output: &Channel<LogEvent>,
        phase: &str,
        reason: Option<&str>,
        retry_at: Option<i64>,
        error: Option<String>,
        tty: Option<bool>,
    ) {
        if phase == "failed" {
            self.terminal
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let _ = output.send(LogEvent::State {
            collector_id: self.id.clone(),
            event_revision: self
                .revision
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            phase: phase.into(),
            reason: reason.map(str::to_owned),
            retry_at,
            error,
            container: self.observation.lock().unwrap().clone(),
            tty,
        });
    }
    fn wait(&self) -> AppResult<()> {
        let (lock, signal) = &*self.done;
        let (done, _) = signal
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(45), |done| !*done)
            .unwrap();
        if *done {
            Ok(())
        } else {
            Err(AppError::new("logs", "上一次日志采集尚未结束，请稍后重试"))
        }
    }
    fn finish(&self) {
        *self.done.0.lock().unwrap() = true;
        self.done.1.notify_all();
    }
}
impl LogState {
    pub fn has_server(&self, id: &str) -> bool {
        self.windows
            .lock()
            .unwrap()
            .values()
            .any(|w| w.spec.as_ref().is_some_and(|s| s.uses_server(id)))
    }
    pub fn active_count(&self) -> usize {
        self.workers
            .lock()
            .unwrap()
            .iter()
            .filter(|r| !*r.done.0.lock().unwrap())
            .count()
    }
    pub fn stop(&self, label: &str, remove: bool) {
        for (owner, cancel) in self.jobs.lock().unwrap().values() {
            if owner == label {
                cancel.cancel();
            }
        }
        self.snapshots
            .lock()
            .unwrap()
            .retain(|_, (owner, _)| owner != label);
        let mut windows = self.windows.lock().unwrap();
        if let Some(run) = windows.get(label).and_then(|w| w.run.as_ref()) {
            run.cancel.cancel();
        }
        if remove {
            windows.remove(label);
        }
    }
    pub fn shutdown(&self) -> AppResult<()> {
        for (_, cancel) in self.jobs.lock().unwrap().values() {
            cancel.cancel();
        }
        self.snapshots.lock().unwrap().clear();
        let workers = self.workers.lock().unwrap().clone();
        for run in &workers {
            run.cancel.cancel();
        }
        for run in &workers {
            run.wait()?;
        }
        Ok(())
    }
    fn source(&self, label: &str) -> AppResult<Source> {
        self.windows
            .lock()
            .unwrap()
            .get(label)
            .map(|w| w.source.clone())
            .ok_or_else(|| AppError::new("window", "日志窗口已失效"))
    }
}
fn root(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("logs", e))?
        .join("logs"))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInfo {
    #[serde(flatten)]
    source: Source,
    can_collect: bool,
    directory: String,
    stats: log_store::Stats,
}
#[tauri::command]
pub async fn open_container_logs(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    target: crate::docker::ContainerTarget,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    validate_id(&target.id)?;
    let spec = state.spec(&session_id, window.label())?;
    let source = Source::new(&spec, &target);
    open_window(&app, &state, source, Some(spec), Some(target))
}
fn open_window(
    app: &tauri::AppHandle,
    state: &Arc<State>,
    source: Source,
    spec: Option<SharedContext>,
    target: Option<crate::docker::ContainerTarget>,
) -> AppResult<()> {
    let title = crate::brand::window_title(
        app,
        &format!("{} 日志 · {}", source.container_name, source.server_name),
    );
    let (label, create) = {
        let _gate = state.configuration.lock().unwrap();
        state.ensure_running()?;
        if let Some(spec) = &spec {
            state.check_context(spec)?;
        }
        let mut windows = state.logs.windows.lock().unwrap();
        if let Some((label, w)) = windows
            .iter_mut()
            .find(|(_, w)| w.source.source_id == source.source_id)
        {
            // Collection credentials only come from an authenticated SSH session.
            if spec.is_some() {
                w.spec = spec;
                w.target = target;
                w.source = source;
            }
            (label.clone(), false)
        } else {
            let label = format!("logs-{}", uuid::Uuid::new_v4());
            windows.insert(
                label.clone(),
                LogWindow {
                    spec,
                    target,
                    source,
                    run: None,
                },
            );
            (label, true)
        }
    };
    if !create {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.unminimize();
            w.set_focus().map_err(|e| AppError::new("window", e))?;
        }
        return Ok(());
    }
    if let Err(e) = crate::window_chrome::configure(
        app,
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html?logs=1".into())),
    )
    .title(title)
    .inner_size(1100.0, 720.0)
    .min_inner_size(650.0, 400.0)
    .build()
    {
        state.logs.stop(&label, true);
        return Err(AppError::new("window", e));
    }
    Ok(())
}
const ARCHIVES_WINDOW: &str = "logs-archives";

fn archive_access(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == ARCHIVES_WINDOW {
        Ok(())
    } else {
        crate::moves::workspace_only(window)
    }
}

#[tauri::command]
pub async fn open_log_archives_window(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    // Serialize creation so repeated clicks reuse a single library window.
    let _gate = state.logs.archive_window.lock().unwrap();
    state.ensure_running()?;
    if let Some(existing) = app.get_webview_window(ARCHIVES_WINDOW) {
        existing
            .unminimize()
            .map_err(|e| AppError::new("window", e))?;
        existing
            .set_focus()
            .map_err(|e| AppError::new("window", e))?;
        return Ok(());
    }
    crate::window_chrome::configure(
        &app,
        WebviewWindowBuilder::new(
            &app,
            ARCHIVES_WINDOW,
            WebviewUrl::App("index.html?archives=1".into()),
        ),
    )
    .title(crate::brand::window_title(&app, "本地日志"))
    .inner_size(1040.0, 720.0)
    .min_inner_size(720.0, 460.0)
    .center()
    .build()
    .map_err(|e| AppError::new("window", e))?;
    Ok(())
}

#[tauri::command]
pub async fn open_log_archive(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    source_id: String,
) -> AppResult<()> {
    archive_access(&window)?;
    let archive_root = root(&app)?;
    let registered = state.inner().clone();
    let source = crate::blocking(move || {
        let workers = registered.logs.workers.lock().unwrap();
        let source = log_store::source(&archive_root, &source_id)?;
        if !workers
            .iter()
            .any(|r| r.source_id == source_id && !*r.done.0.lock().unwrap())
        {
            crate::log_ingest::recover_archive(&archive_root, &source)?;
        }
        Ok(source)
    })
    .await?;
    open_window(&app, &state, source, None, None)
}
#[tauri::command]
pub async fn list_log_archives(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> AppResult<Vec<log_store::Archive>> {
    archive_access(&window)?;
    let archive_root = root(&app)?;
    crate::blocking(move || log_store::archives(&archive_root)).await
}
#[tauri::command]
pub async fn logs_info(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<LogInfo> {
    let source = state.logs.source(window.label())?;
    let can_collect = state
        .logs
        .windows
        .lock()
        .unwrap()
        .get(window.label())
        .is_some_and(|w| w.spec.is_some());
    let archive_root = root(&app)?;
    crate::blocking(move || {
        let stats = log_store::stats(&archive_root, &source.source_id)?;
        let directory = log_store::source_dir(&archive_root, &source.source_id)?
            .display()
            .to_string();
        let mut source = source;
        if let Ok(saved) = log_store::source(&archive_root, &source.source_id) {
            source.tty = saved.tty;
        }
        Ok(LogInfo {
            source,
            can_collect,
            directory,
            stats,
        })
    })
    .await
}
#[tauri::command]
pub async fn logs_page(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    query: log_store::Query,
    snapshot_id: Option<String>,
) -> AppResult<log_store::Page> {
    let source = state.logs.source(window.label())?;
    let archive_root = root(&app)?;
    let frozen = snapshot_id
        .map(|id| get_snapshot(&state, window.label(), &id))
        .transpose()?;
    crate::blocking(move || match frozen {
        Some(s) => s.query(&query),
        None => log_store::query(&archive_root, &source.source_id, &query),
    })
    .await
}
#[tauri::command]
pub async fn logs_export(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    from: Option<i64>,
    to: Option<i64>,
) -> AppResult<Option<String>> {
    let source = state.logs.source(window.label())?;
    let archive_root = root(&app)?;
    crate::blocking(move || {
        if from.is_some_and(|v| v < 1) || to.is_some_and(|v| v < from.unwrap_or(1)) {
            return Err(AppError::new("validation", "请选择有效的日志序号范围"));
        }
        #[cfg(debug_assertions)]
        let test_path = std::env::var_os("SSH_WORKSPACE_QA_LOG_EXPORT_DIR")
            .map(|dir| PathBuf::from(dir).join(format!("logs-{}.log", uuid::Uuid::new_v4())));
        #[cfg(not(debug_assertions))]
        let test_path: Option<PathBuf> = None;
        let path = test_path.or_else(|| {
            rfd::FileDialog::new()
                .set_title("导出本地容器日志")
                .set_file_name("container-logs.log")
                .save_file()
        });
        if let Some(path) = path {
            log_store::export(&archive_root, &source.source_id, &path, from, to)?;
            Ok(Some(path.display().to_string()))
        } else {
            Ok(None)
        }
    })
    .await
}
#[tauri::command]
pub async fn logs_copy_range(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    from: i64,
    to: i64,
    snapshot_id: Option<String>,
) -> AppResult<String> {
    let source = state.logs.source(window.label())?;
    let archive_root = root(&app)?;
    let frozen = snapshot_id
        .map(|id| get_snapshot(&state, window.label(), &id))
        .transpose()?;
    crate::blocking(move || {
        if from < 1 || to < from {
            return Err(AppError::new("validation", "请选择有效的日志序号范围"));
        }
        let mut cursor = from - 1;
        let mut text = String::new();
        loop {
            let query = log_store::Query {
                after: Some(cursor),
                snapshot: Some(to),
                limit: 500,
                ..Default::default()
            };
            let page = match &frozen {
                Some(s) => s.query(&query)?,
                None => log_store::query(&archive_root, &source.source_id, &query)?,
            };
            for row in page.rows {
                if let Some(s) = &frozen {
                    text.push_str(&String::from_utf8_lossy(&export::original(
                        &s.segments,
                        row.seq,
                        &Cancellation::default(),
                    )?));
                } else {
                    text.push_str(&row.raw);
                    if row.complete {
                        text.push('\n');
                    }
                }
                if text.len() > 8 * 1024 * 1024 {
                    return Err(AppError::new("logs", "所选内容超过 8 MiB，请使用导出"));
                }
            }
            if !page.has_more || page.next_cursor <= cursor {
                break;
            }
            cursor = page.next_cursor;
        }
        Ok(text)
    })
    .await
}
#[tauri::command]
pub fn logs_start(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    output: Channel<LogEvent>,
) -> AppResult<String> {
    let gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    let archive_root = root(&app)?;
    let run = Run::new(state.logs.source(window.label())?.source_id);
    // Register in the same order as replacing the window's run. Otherwise two
    // concurrent restarts could each wait for the other worker to finish.
    let mut workers = state.logs.workers.lock().unwrap();
    let (spec, target, source, old) = {
        let mut windows = state.logs.windows.lock().unwrap();
        let w = windows
            .get_mut(window.label())
            .ok_or_else(|| AppError::new("window", "日志窗口已失效"))?;
        let spec = w.spec.clone().ok_or_else(|| {
            AppError::new("logs", "这是本地存档；请连接服务器后从 Docker 树恢复采集")
        })?;
        let target = w.target.clone().unwrap();
        let old = w.run.replace(run.clone());
        if let Some(old) = &old {
            old.cancel.cancel();
        }
        (spec, target, w.source.clone(), old)
    };
    let previous = {
        workers.retain(|r| !*r.done.0.lock().unwrap());
        let previous = workers
            .iter()
            .filter(|r| r.source_id == run.source_id)
            .cloned()
            .collect::<Vec<_>>();
        for prior in &previous {
            prior.cancel.cancel();
        }
        workers.push(run.clone());
        previous
    };
    drop(workers);
    drop(gate);
    let id = run.id.clone();
    let label = window.label().to_owned();
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let result = (|| {
            if let Some(old) = old {
                old.wait()?;
            }
            for prior in previous {
                prior.wait()?;
            }
            if run.cancel.is_cancelled() {
                Ok(())
            } else {
                collect_logs(&spec, &target, &source, &archive_root, &run, &output)
            }
        })();
        let _ = output.send(LogEvent::End {
            error: result.err().map(|e| e.message),
        });
        if let Some(w) = state.logs.windows.lock().unwrap().get_mut(&label) {
            if w.run.as_ref().is_some_and(|r| r.id == run.id) {
                w.run = None;
            }
        }
        run.finish();
    });
    Ok(id)
}
#[tauri::command]
pub async fn logs_stop(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    collector_id: Option<String>,
) -> AppResult<()> {
    let run = state
        .logs
        .windows
        .lock()
        .unwrap()
        .get(window.label())
        .and_then(|w| w.run.clone());
    if let Some(run) = run.filter(|r| collector_id.as_ref().is_none_or(|id| id == &r.id)) {
        run.cancel.cancel();
        crate::blocking(move || run.wait()).await?;
    }
    Ok(())
}
#[tauri::command]
pub fn log_session_count(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<usize> {
    crate::main_only(&window)?;
    Ok(state.logs.active_count())
}
fn validate_id(id: &str) -> AppResult<()> {
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        Err(AppError::new("validation", "无效的容器 ID"))
    } else {
        Ok(())
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn exercise_jump_archive(context: &SharedContext, root: &std::path::Path) {
        let target = crate::docker::ContainerTarget {
            id: "a".repeat(64),
            name: "fixture".into(),
            endpoint: "unix:///var/run/docker.sock".into(),
        };
        let source = Source::new(context, &target);
        let run = Run::new(source.source_id.clone());
        let output = Channel::new(|_| Ok(()));
        let archive_root = root.join("archives");
        read_logs(
            context,
            &target,
            &source,
            &archive_root,
            true,
            &run,
            &output,
        )
        .unwrap();
        assert!(crate::log_ingest::history_complete(&archive_root, &source.source_id).unwrap());
        let rows = log_store::query(
            &archive_root,
            &source.source_id,
            &log_store::Query {
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap()
        .rows;
        assert!(rows
            .iter()
            .any(|r| r.raw.contains("fixture history stdout")));
        assert!(rows
            .iter()
            .any(|r| r.raw.contains("fixture history stderr")));
        let worker_context = context.clone();
        let worker_run = run.clone();
        let worker_source = source.clone();
        let worker_root = archive_root.clone();
        let worker = std::thread::spawn(move || {
            let result = read_logs(
                &worker_context,
                &target,
                &worker_source,
                &worker_root,
                false,
                &worker_run,
                &output,
            );
            worker_run.finish();
            result
        });
        let deadline = Instant::now() + Duration::from_secs(12);
        while log_store::stats(&archive_root, &source.source_id)
            .unwrap()
            .runs
            < 2
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        run.cancel.cancel();
        run.wait().unwrap();
        let result = worker.join().unwrap();
        assert!(
            result.is_ok()
                || result.is_err_and(|e| matches!(
                    e.kind.as_str(),
                    "cancelled" | "network" | "io" | "ssh"
                ))
        );
        let stats = log_store::stats(&archive_root, &source.source_id).unwrap();
        assert_eq!(
            stats.interrupted_runs, 0,
            "stopping flushes and finishes the archive run"
        );
        assert!(stats.records >= rows.len() as i64);
    }
    #[test]
    fn log_command_quotes_and_resumes_without_tail_truncation() {
        let target = crate::docker::ContainerTarget {
            id: "a".repeat(64),
            name: "ignored; name".into(),
            endpoint: "unix:///tmp/a'b".into(),
        };
        let cmd = command(&target, true, Some("2026-10-14T09:00:00Z")).unwrap();
        assert!(cmd.contains("'unix:///tmp/a'\\''b'"));
        assert!(cmd.contains("--tail all"));
        assert!(cmd.contains("--since"));
        assert!(cmd.contains("--follow"));
        assert!(!cmd.contains(&target.name));
        assert!(!command(&target, false, None).unwrap().contains("--follow"));
    }
    #[test]
    fn retry_backoff_is_bounded_and_does_not_retry_authentication_or_disk_errors() {
        assert_eq!(
            (0..8).map(|n| retry_delay(n).as_secs()).collect::<Vec<_>>(),
            vec![1, 2, 4, 8, 16, 30, 30, 30]
        );
        assert!(retryable(&AppError::new("network", "connection reset")));
        assert!(!retryable(&AppError::from(ssh2::Error::new(
            ssh2::ErrorCode::Session(-18),
            "Authentication failed"
        ))));
        assert!(!retryable(&AppError::new("hostKey", "changed")));
        assert!(!retryable(&AppError::new(
            "logs",
            "日志保存失败：disk full"
        )));
        assert!(!retryable(&AppError::new("docker", "permission denied")));
    }
}

#[tauri::command]
pub fn logs_retry(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<bool> {
    let windows = state.logs.windows.lock().unwrap();
    let w = windows
        .get(window.label())
        .ok_or_else(|| AppError::new("window", "日志窗口已失效"))?;
    if let Some(run) = w
        .run
        .as_ref()
        .filter(|r| !r.terminal.load(std::sync::atomic::Ordering::SeqCst))
    {
        *run.wake.0.lock().unwrap() = true;
        run.wake.1.notify_all();
        Ok(true)
    } else {
        Ok(false)
    }
}
#[tauri::command]
pub async fn logs_events_page(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    after: Option<i64>,
    before: Option<i64>,
) -> AppResult<lifecycle::Timeline> {
    let source = state.logs.source(window.label())?;
    let root = root(&app)?;
    crate::blocking(move || {
        lifecycle::timeline(&root, &source.source_id, after.unwrap_or(0), before)
    })
    .await
}

fn get_snapshot(state: &State, owner: &str, id: &str) -> AppResult<Arc<snapshot::Snapshot>> {
    state
        .logs
        .snapshots
        .lock()
        .unwrap()
        .get(id)
        .filter(|(label, _)| label == owner)
        .map(|(_, s)| s.clone())
        .ok_or_else(|| AppError::new("logs", "搜索快照已失效，请更新搜索结果"))
}
fn cache_root(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new("logs", e))?
        .join("log-snapshots"))
}
fn register_job(state: &State, owner: &str, id: &str) -> AppResult<Cancellation> {
    if id.is_empty() || id.len() > 128 {
        return Err(AppError::new("validation", "无效任务标识"));
    }
    let mut jobs = state.logs.jobs.lock().unwrap();
    if jobs.contains_key(id) {
        return Err(AppError::new("logs", "任务已存在"));
    }
    if jobs.values().filter(|(label, _)| label == owner).count() >= 4 {
        return Err(AppError::new("logs", "日志任务仍在结束，请稍后重试"));
    }
    let c = Cancellation::default();
    jobs.insert(id.into(), (owner.into(), c.clone()));
    Ok(c)
}
#[tauri::command]
pub fn logs_cancel_job(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    request_id: String,
) -> AppResult<()> {
    if let Some((owner, cancel)) = state.logs.jobs.lock().unwrap().get(&request_id) {
        if owner != window.label() {
            return Err(AppError::new("window", "任务不属于当前窗口"));
        }
        cancel.cancel();
    }
    Ok(())
}
#[tauri::command]
pub async fn logs_snapshot_create(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    request_id: String,
) -> AppResult<snapshot::Info> {
    let source = state.logs.source(window.label())?;
    let owner = window.label().to_owned();
    let cancel = register_job(&state, &owner, &request_id)?;
    let root = root(&app)?;
    let cache = cache_root(&app)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let result = (|| {
            let snapshot = snapshot::Snapshot::create(&root, &source.source_id, &cache, &cancel)?;
            cancel.check()?;
            let windows = state.logs.windows.lock().unwrap();
            if !windows.contains_key(&owner) {
                return Err(AppError::new("window", "日志窗口已关闭"));
            }
            let info = snapshot.info.clone();
            state
                .logs
                .snapshots
                .lock()
                .unwrap()
                .insert(info.id.clone(), (owner, snapshot));
            Ok(info)
        })();
        state.logs.jobs.lock().unwrap().remove(&request_id);
        result
    })
    .await
}
#[tauri::command]
pub fn logs_snapshot_release(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    snapshot_id: String,
) -> AppResult<()> {
    let mut snapshots = state.logs.snapshots.lock().unwrap();
    if snapshots
        .get(&snapshot_id)
        .is_some_and(|(owner, _)| owner == window.label())
    {
        snapshots.remove(&snapshot_id);
    }
    Ok(())
}
#[tauri::command]
pub async fn logs_export_start(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    request_id: String,
    scope: export::Scope,
    output: Channel<export::Progress>,
) -> AppResult<Option<String>> {
    let source = state.logs.source(window.label())?;
    let owner = window.label().to_owned();
    let frozen = match &scope {
        export::Scope::Query { snapshot_id, .. }
        | export::Scope::Selection {
            snapshot_id: Some(snapshot_id),
            ..
        } => Some(get_snapshot(&state, &owner, snapshot_id)?),
        _ => None,
    };
    if let export::Scope::Selection { from, to, .. } = &scope {
        if *from < 1 || to < from {
            return Err(AppError::new("validation", "请选择有效的日志序号范围"));
        }
    }
    let root = root(&app)?;
    let cache = cache_root(&app)?;
    let cancel = register_job(&state, &owner, &request_id)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let result = (|| {
            #[cfg(debug_assertions)]
            let test_path = std::env::var_os("SSH_WORKSPACE_QA_LOG_EXPORT_DIR")
                .map(|dir| PathBuf::from(dir).join(format!("logs-{}.log", uuid::Uuid::new_v4())));
            #[cfg(not(debug_assertions))]
            let test_path: Option<PathBuf> = None;
            let path = test_path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title("导出容器日志")
                    .set_file_name("container-logs.log")
                    .save_file()
            });
            let Some(path) = path else {
                return Ok(None);
            };
            cancel.check()?;
            let snapshot = match frozen {
                Some(s) => s,
                None => snapshot::Snapshot::create(&root, &source.source_id, &cache, &cancel)?,
            };
            let (query, from, to) = match scope {
                export::Scope::All => (log_store::Query::default(), None, None),
                export::Scope::Selection { from, to, .. } => {
                    (log_store::Query::default(), Some(from), Some(to))
                }
                export::Scope::Query { query, .. } => (query, None, None),
            };
            export::save(&snapshot, query, from, to, &path, &cancel, &output)?;
            Ok(Some(path.display().to_string()))
        })();
        state.logs.jobs.lock().unwrap().remove(&request_id);
        result
    })
    .await
}
