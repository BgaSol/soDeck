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
}
impl Run {
    fn new(source_id: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_id,
            cancel: Cancellation::default(),
            done: Arc::new((Mutex::new(false), Condvar::new())),
        }
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
        let mut windows = self.windows.lock().unwrap();
        if let Some(run) = windows.get(label).and_then(|w| w.run.as_ref()) {
            run.cancel.cancel();
        }
        if remove {
            windows.remove(label);
        }
    }
    pub fn shutdown(&self) -> AppResult<()> {
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
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LogEvent {
    Ready {
        following: bool,
        tty: bool,
        history: bool,
    },
    Status {
        status: String,
        error: Option<String>,
    },
    Saved {
        last_seq: i64,
    },
    End {
        error: Option<String>,
    },
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
) -> AppResult<log_store::Page> {
    let source = state.logs.source(window.label())?;
    let archive_root = root(&app)?;
    crate::blocking(move || log_store::query(&archive_root, &source.source_id, &query)).await
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
) -> AppResult<String> {
    let source = state.logs.source(window.label())?;
    let archive_root = root(&app)?;
    crate::blocking(move || {
        if from < 1 || to < from {
            return Err(AppError::new("validation", "请选择有效的日志序号范围"));
        }
        let mut cursor = from - 1;
        let mut text = String::new();
        loop {
            let page = log_store::query(
                &archive_root,
                &source.source_id,
                &log_store::Query {
                    after: Some(cursor),
                    snapshot: Some(to),
                    limit: 500,
                    ..Default::default()
                },
            )?;
            for row in page.rows {
                text.push_str(&row.raw);
                if row.complete {
                    text.push('\n');
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
pub async fn logs_stop(window: WebviewWindow, state: TauriState<'_, Arc<State>>) -> AppResult<()> {
    let run = state
        .logs
        .windows
        .lock()
        .unwrap()
        .get(window.label())
        .and_then(|w| w.run.clone());
    if let Some(run) = run {
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
fn command(
    target: &crate::docker::ContainerTarget,
    following: bool,
    since: Option<&str>,
) -> AppResult<String> {
    validate_id(&target.id)?;
    Ok(format!("env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} logs --tail all --timestamps {} {} {}",ssh::quote(&target.endpoint),if following{"--follow"}else{""},
  since.map(|s|format!("--since {}",ssh::quote(s))).unwrap_or_default(),ssh::quote(&target.id)))
}
fn wait_cancel(run: &Run, duration: Duration) {
    let until = Instant::now() + duration;
    while !run.cancel.is_cancelled() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.min(5)).min(30))
}
fn retryable(error: &AppError) -> bool {
    matches!(error.kind.as_str(), "network" | "ssh" | "timeout")
        || (error.kind == "io" && !error.message.starts_with("日志保存失败"))
}

fn collect_logs(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    source: &Source,
    archive_root: &std::path::Path,
    run: &Run,
    output: &Channel<LogEvent>,
) -> AppResult<()> {
    let mut attempt = 0;
    while !run.cancel.is_cancelled() {
        let history = !crate::log_ingest::history_complete(archive_root, &source.source_id)?;
        match read_logs(spec, target, source, archive_root, history, run, output) {
            Ok(following) => {
                attempt = 0;
                if run.cancel.is_cancelled() {
                    break;
                }
                if history {
                    continue;
                }
                let _ = output.send(LogEvent::Status {
                    status: if following {
                        "日志流已结束，正在检查容器…"
                    } else {
                        "等待容器运行"
                    }
                    .into(),
                    error: None,
                });
                wait_cancel(run, Duration::from_secs(5));
            }
            Err(error) if retryable(&error) => {
                let delay = retry_delay(attempt);
                attempt += 1;
                let _ = output.send(LogEvent::Status {
                    status: format!("连接中断，{} 秒后重试", delay.as_secs()),
                    error: Some(error.message),
                });
                wait_cancel(run, delay);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn read_logs(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    source: &Source,
    archive_root: &std::path::Path,
    history: bool,
    run: &Run,
    output: &Channel<LogEvent>,
) -> AppResult<bool> {
    let connection = connection::connect_verified(spec, &run.cancel, crate::traffic::TrafficPurpose::Logs)?;
    let session = connection.session();
    if run.cancel.is_cancelled() {
        return Ok(false);
    }
    let inspect=ssh::exec(session,&format!("env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} inspect --type container --format '{{{{json .}}}}' {}",ssh::quote(&target.endpoint),ssh::quote(&target.id)))?;
    if inspect.code != 0 {
        return Err(AppError::new("docker", inspect.stderr.trim()));
    }
    let metadata: serde_json::Value =
        serde_json::from_str(inspect.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
    let following = metadata["State"]["Running"]
        .as_bool()
        .ok_or_else(|| AppError::new("docker", "无法确认容器状态"))?;
    let tty = metadata["Config"]["Tty"].as_bool().unwrap_or(false);
    if run.cancel.is_cancelled() {
        return Ok(false);
    }
    let mut saved_source = source.clone();
    saved_source.tty = tty;
    let mut store = log_store::Store::open(
        archive_root,
        &saved_source,
        &uuid::Uuid::new_v4().to_string(),
        history,
    )
    .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
    let mut opened_channel = None;
    let mut chunks = Vec::new();
    let mut count = 0;
    let mut stderr_tail = Vec::new();
    let mut buffer = [0u8; 16384];
    let mut flush = Instant::now();
    let mut keepalive = Instant::now();
    let mut write_failed = false;
    let result = (|| {
        // Once the archive run exists, setup failures and early cancellation
        // must take the same flush/finish path as a running log stream.
        run.cancel.check()?;
        let mut channel = session.channel_session()?;
        channel.exec(&command(
            target,
            following && !history,
            store.since.as_deref(),
        )?)?;
        session.set_blocking(false);
        opened_channel = Some(channel);
        let channel = opened_channel.as_mut().unwrap();
        let _ = output.send(LogEvent::Ready {
            following,
            tty,
            history,
        });
        loop {
            if run.cancel.is_cancelled() {
                break;
            }
            let mut received = false;
            let mut read_error = None;
            for stream in 0..2 {
                match channel.stream(stream as i32).read(&mut buffer) {
                    Ok(n) if n > 0 => {
                        received = true;
                        chunks.push((stream, buffer[..n].to_vec()));
                        count += n;
                        if stream == 1 {
                            stderr_tail.extend_from_slice(&buffer[..n]);
                            if stderr_tail.len() > 8192 {
                                stderr_tail.drain(..stderr_tail.len() - 8192);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => {
                        read_error = Some(e);
                        break;
                    }
                }
            }
            let eof = channel.eof() && !received;
            if eof
                || read_error.is_some()
                || flush.elapsed() >= Duration::from_millis(100)
                || count >= 256 * 1024
            {
                if !chunks.is_empty() {
                    let last_seq = match store.append(&chunks) {
                        Ok(seq) => seq,
                        Err(e) => {
                            write_failed = true;
                            return Err(AppError::new(
                                "logs",
                                format!("日志保存失败：{}", e.message),
                            ));
                        }
                    };
                    chunks.clear();
                    count = 0;
                    let _ = output.send(LogEvent::Saved { last_seq });
                }
                flush = Instant::now();
            }
            if let Some(e) = read_error {
                return Err(AppError::new("network", e));
            }
            if eof {
                let started = Instant::now();
                loop {
                    match channel.wait_close() {
                        Ok(()) => break,
                        Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                            if run.cancel.is_cancelled() {
                                break;
                            }
                            if started.elapsed() > Duration::from_secs(5) {
                                return Err(AppError::new(
                                    "network",
                                    "日志连接已结束，未收到退出状态",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                if !run.cancel.is_cancelled() && channel.exit_status()? != 0 {
                    return Err(AppError::new(
                        "docker",
                        String::from_utf8_lossy(&stderr_tail).trim(),
                    ));
                }
                break;
            }
            if keepalive.elapsed() > Duration::from_secs(20) {
                if let Err(e) = session.keepalive_send() {
                    if e.code() != ssh2::ErrorCode::Session(-37) {
                        return Err(e.into());
                    }
                }
                keepalive = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    // Flush even after cancellation, EOF, transport failure or a closed frontend.
    if !write_failed {
        if !chunks.is_empty() {
            let seq = store
                .append(&chunks)
                .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
            let _ = output.send(LogEvent::Saved { last_seq: seq });
        }
        store
            .finish(if run.cancel.is_cancelled() {
                "用户停止采集"
            } else if result.is_err() {
                "连接中断；下次补采的覆盖无法确认"
            } else {
                "日志读取已结束"
            })
            .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
        if history && result.is_ok() && !run.cancel.is_cancelled() {
            store
                .mark_history_complete()
                .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
        }
        let _ = output.send(LogEvent::Saved {
            last_seq: log_store::stats(archive_root, &source.source_id)?.last_seq,
        });
    }
    if let Some(channel) = &mut opened_channel {
        let _ = channel.close();
    }
    let _ = session.disconnect(None, "Log collection ended", None);
    if run.cancel.is_cancelled() {
        Ok(following)
    } else {
        result.map(|()| following)
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
