use crate::{
    model::{AppError, AppResult},
    ssh, State,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    ipc::Channel, Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

#[derive(Default)]
pub struct LogState {
    windows: Mutex<HashMap<String, LogWindow>>,
}
struct LogWindow {
    spec: ssh::ConnectionSpec,
    target: crate::docker::ContainerTarget,
    run: Option<Run>,
}
#[derive(Clone)]
struct Run {
    id: String,
    cancel: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}
impl LogState {
    pub fn active_count(&self) -> usize {
        self.windows
            .lock()
            .unwrap()
            .values()
            .filter(|w| w.run.is_some())
            .count()
    }
    pub fn stop(&self, label: &str, remove: bool) {
        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.get_mut(label) {
            if let Some(run) = window.run.take() {
                run.cancel.store(true, Ordering::Relaxed);
            }
        }
        if remove {
            windows.remove(label);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInfo {
    server_name: String,
    container_name: String,
    container_id: String,
    endpoint: String,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LogEvent {
    Ready {
        following: bool,
        tty: bool,
    },
    Data {
        stream: &'static str,
        data: String,
        bytes: usize,
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
    let title = format!("{} / {} / 日志", spec.server.name, target.name);
    let (label, create) = {
        let mut windows = state.logs.windows.lock().unwrap();
        if let Some((label, _)) = windows.iter().find(|(_, w)| {
            w.spec.server.id == spec.server.id
                && w.target.id == target.id
                && w.target.endpoint == target.endpoint
        }) {
            (label.clone(), false)
        } else {
            let label = format!("logs-{}", uuid::Uuid::new_v4());
            windows.insert(
                label.clone(),
                LogWindow {
                    spec,
                    target,
                    run: None,
                },
            );
            (label, true)
        }
    };
    if !create {
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.unminimize();
            existing
                .set_focus()
                .map_err(|e| AppError::new("window", e))?;
        }
        return Ok(());
    }
    let result =
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html?logs=1".into()))
            .title(title)
            .inner_size(1000.0, 680.0)
            .min_inner_size(650.0, 400.0)
            .build();
    if let Err(error) = result {
        state.logs.stop(&label, true);
        return Err(AppError::new("window", error));
    }
    Ok(())
}

#[tauri::command]
pub fn logs_info(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<LogInfo> {
    let windows = state.logs.windows.lock().unwrap();
    let w = windows
        .get(window.label())
        .ok_or_else(|| AppError::new("window", "日志窗口已失效"))?;
    Ok(LogInfo {
        server_name: w.spec.server.name.clone(),
        container_name: w.target.name.clone(),
        container_id: w.target.id.clone(),
        endpoint: w.target.endpoint.clone(),
    })
}

#[tauri::command]
pub fn logs_start(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    tail: u32,
    output: Channel<LogEvent>,
) -> AppResult<String> {
    if ![100, 500, 1000, 5000].contains(&tail) {
        return Err(AppError::new("validation", "无效的日志行数"));
    }
    let run = Run {
        id: uuid::Uuid::new_v4().to_string(),
        cancel: Arc::new(AtomicBool::new(false)),
        pending: Arc::new(AtomicUsize::new(0)),
    };
    let (spec, target) = {
        let mut windows = state.logs.windows.lock().unwrap();
        let w = windows
            .get_mut(window.label())
            .ok_or_else(|| AppError::new("window", "日志窗口已失效"))?;
        if let Some(old) = w.run.replace(run.clone()) {
            old.cancel.store(true, Ordering::Relaxed);
        }
        (w.spec.clone(), w.target.clone())
    };
    let id = run.id.clone();
    let label = window.label().to_owned();
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let result = read_logs(&spec, &target, tail, &run, &output);
        if !run.cancel.load(Ordering::Relaxed) {
            let _ = output.send(LogEvent::End {
                error: result.err().map(|e| e.message),
            });
        }
        if let Some(w) = state.logs.windows.lock().unwrap().get_mut(&label) {
            if w.run.as_ref().is_some_and(|r| r.id == run.id) {
                w.run = None;
            }
        }
    });
    Ok(id)
}

#[tauri::command]
pub fn logs_ack(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    run_id: String,
    bytes: usize,
) {
    let windows = state.logs.windows.lock().unwrap();
    if let Some(run) = windows
        .get(window.label())
        .and_then(|w| w.run.as_ref())
        .filter(|r| r.id == run_id)
    {
        let _ = run
            .pending
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |pending| {
                Some(pending.saturating_sub(bytes))
            });
    }
}

#[tauri::command]
pub fn logs_stop(window: WebviewWindow, state: TauriState<Arc<State>>) {
    state.logs.stop(window.label(), false);
}

#[tauri::command]
pub fn log_session_count(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<usize> {
    crate::main_only(&window)?;
    Ok(state.logs.active_count())
}

fn validate_id(id: &str) -> AppResult<()> {
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::new("validation", "无效的容器 ID"));
    }
    Ok(())
}
fn command(
    target: &crate::docker::ContainerTarget,
    tail: u32,
    following: bool,
) -> AppResult<String> {
    validate_id(&target.id)?;
    Ok(format!(
        "env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} logs --tail {} --timestamps {} {}",
        ssh::quote(&target.endpoint),
        tail,
        if following { "--follow" } else { "" },
        ssh::quote(&target.id)
    ))
}

fn read_logs(
    spec: &ssh::ConnectionSpec,
    target: &crate::docker::ContainerTarget,
    tail: u32,
    run: &Run,
    output: &Channel<LogEvent>,
) -> AppResult<()> {
    let session = ssh::establish_spec(spec)?;
    if run.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let inspect = ssh::exec(&session, &format!("env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} inspect --type container --format '{{{{json .}}}}' {}", ssh::quote(&target.endpoint), ssh::quote(&target.id)))?;
    if inspect.code != 0 {
        return Err(AppError::new("docker", inspect.stderr.trim()));
    }
    let metadata: serde_json::Value =
        serde_json::from_str(inspect.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
    let following = metadata["State"]["Running"]
        .as_bool()
        .ok_or_else(|| AppError::new("docker", "无法确认容器状态"))?;
    let tty = metadata["Config"]["Tty"].as_bool().unwrap_or(false);
    if run.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut channel = session.channel_session()?;
    channel.exec(&command(target, tail, following)?)?;
    session.set_blocking(false);
    output
        .send(LogEvent::Ready { following, tty })
        .map_err(|e| AppError::new("window", e))?;
    let mut buffers = [Vec::new(), Vec::new()];
    let mut stderr_tail = Vec::new();
    let mut buffer = [0u8; 16384];
    let mut last_flush = Instant::now();
    let mut last_keepalive = Instant::now();
    loop {
        if run.cancel.load(Ordering::Relaxed) {
            break;
        }
        if run.pending.load(Ordering::Relaxed) < 256 * 1024 {
            let mut received = false;
            let mut read_error = None;
            for (index, bytes) in buffers.iter_mut().enumerate() {
                match channel.stream(index as i32).read(&mut buffer) {
                    Ok(n) => {
                        received |= n > 0;
                        bytes.extend_from_slice(&buffer[..n]);
                        if index == 1 {
                            stderr_tail.extend_from_slice(&buffer[..n]);
                            if stderr_tail.len() > 8192 {
                                stderr_tail.drain(..stderr_tail.len() - 8192);
                            }
                        }
                    }
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
                || last_flush.elapsed() >= Duration::from_millis(50)
                || buffers.iter().map(Vec::len).sum::<usize>() >= 64 * 1024
            {
                for (index, bytes) in buffers.iter_mut().enumerate() {
                    if bytes.is_empty() {
                        continue;
                    }
                    run.pending.fetch_add(bytes.len(), Ordering::Relaxed);
                    output
                        .send(LogEvent::Data {
                            stream: if index == 0 { "stdout" } else { "stderr" },
                            data: STANDARD.encode(&*bytes),
                            bytes: bytes.len(),
                        })
                        .map_err(|e| AppError::new("window", e))?;
                    bytes.clear();
                }
                last_flush = Instant::now();
            }
            if let Some(error) = read_error {
                return Err(error.into());
            }
            if eof {
                // EOF can precede exit-status/close. Wait without blocking cancellation.
                let started = Instant::now();
                loop {
                    match channel.wait_close() {
                        Ok(()) => break,
                        Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                            if run.cancel.load(Ordering::Relaxed) {
                                return Ok(());
                            }
                            if started.elapsed() > Duration::from_secs(5) {
                                return Err(AppError::new(
                                    "network",
                                    "日志读取连接已结束，未收到退出状态",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                if channel.exit_status()? != 0 {
                    let detail = String::from_utf8_lossy(&stderr_tail);
                    return Err(AppError::new(
                        "docker",
                        if detail.trim().is_empty() {
                            "Docker 日志读取失败"
                        } else {
                            detail.trim()
                        },
                    ));
                }
                return Ok(());
            }
        }
        if last_keepalive.elapsed() > Duration::from_secs(20) {
            if let Err(e) = session.keepalive_send() {
                if e.code() != ssh2::ErrorCode::Session(-37) {
                    return Err(e.into());
                }
            }
            last_keepalive = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = channel.close();
    let _ = session.disconnect(None, "Log window closed", None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_command_quotes_engine_and_validates_container() {
        let mut target = crate::docker::ContainerTarget {
            id: "a".repeat(64),
            name: "ignored; name".into(),
            endpoint: "unix:///tmp/a'b".into(),
        };
        let cmd = command(&target, 1000, true).unwrap();
        assert!(cmd.contains("'unix:///tmp/a'\\''b'"));
        assert!(cmd.contains("--follow"));
        assert!(!cmd.contains(&target.name));
        assert!(!command(&target, 100, false).unwrap().contains("--follow"));
        target.id = "; bad".into();
        assert!(command(&target, 100, true).is_err());
    }
}
