use super::{
    event::{self, Context, FrontendEvent},
    snapshot, store, Diagnostics, Status,
};
use crate::model::{AppError, AppResult};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

static CLIENTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ACKS: OnceLock<Mutex<HashMap<String, HashSet<String>>>> = OnceLock::new();
static RATE: OnceLock<Mutex<HashMap<String, (Instant, usize)>>> = OnceLock::new();
fn convert(e: std::io::Error) -> AppError {
    AppError::new("diagnostics", e.to_string())
}
#[tauri::command]
pub fn register_diagnostics_window(window: WebviewWindow) {
    let mut clients = CLIENTS.get_or_init(Default::default).lock().unwrap();
    if clients.len() < 256 {
        clients.insert(window.label().into());
    }
}
#[tauri::command]
pub fn ack_diagnostics_flush(window: WebviewWindow, token: String) {
    if let Some(set) = ACKS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get_mut(&token)
    {
        set.insert(window.label().into());
    }
}
#[tauri::command]
pub fn append_frontend_diagnostics(
    window: WebviewWindow,
    events: Vec<FrontendEvent>,
) -> AppResult<()> {
    if events.len() > 32 {
        return Err(AppError::new("diagnostics", "诊断批次过大"));
    }
    let mut rates = RATE.get_or_init(Default::default).lock().unwrap();
    if rates.len() > 256 {
        rates.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(2));
    }
    let rate = rates
        .entry(window.label().into())
        .or_insert((Instant::now(), 0));
    if rate.0.elapsed() > Duration::from_secs(1) {
        *rate = (Instant::now(), 0);
    }
    rate.1 += events.len();
    if rate.1 > 256 {
        return Err(AppError::new("diagnostics", "诊断事件频率过高"));
    }
    drop(rates);
    for event in events {
        if !matches!(
            event.event.as_str(),
            "invoke.start"
                | "invoke.end"
                | "frontend.error"
                | "frontend.rejection"
                | "frontend.render.error"
                | "frontend.ready"
                | "frontend.dropped"
                | "clipboard.result"
                | "ui.error.shown"
        ) {
            continue;
        }
        let context = Context::from_trace(event.trace_id.as_deref());
        let command = event
            .command
            .as_deref()
            .filter(|c| super::known_command(c))
            .unwrap_or("unknown");
        let outcome = event
            .outcome
            .as_deref()
            .filter(|v| matches!(*v, "ok" | "accepted" | "failed" | "cancelled" | "omitted"))
            .unwrap_or("omitted");
        let code = event
            .error_code
            .as_deref()
            .filter(|v| super::commands_allowlist::known_error_code(v))
            .unwrap_or("unknown");
        let details = json!({"command":command,"outcome":outcome,"durationMs":event.duration_ms.filter(|v|v.is_finite()&&*v>=0.0&&*v<1e12),"dropped":event.dropped,"frontendSeq":event.frontend_seq,"frontendMs":event.frontend_ms.filter(|v|v.is_finite()&&*v>=0.0),"windowRef":super::alias(window.label()),"errorCode":code,"nativeCode":event.native_code,"taskRef":event.subject.as_deref().filter(|s|s.len()<4096).map(super::alias),"detailOmitted":true});
        let at = std::panic::Location::caller();
        let mut data = event::safe_details(details);
        if let Some(stack) = event.stack.filter(|s| s.len() <= 16 * 1024) {
            data["stackFrames"] = json!(event::stack_frames(&stack));
        }
        super::record(event::Event {
            captured_at: event::now(),
            level: if outcome == "failed"
                || event.event.contains("error")
                || event.event.contains("rejection")
            {
                "ERROR"
            } else {
                "INFO"
            },
            module: "frontend".into(),
            event: event.event,
            trace_id: context.trace_id,
            span_id: context.span_id,
            parent_span_id: None,
            source: event::source(at.file(), at.line()),
            details: data,
        });
    }
    Ok(())
}
#[tauri::command]
pub fn get_diagnostics_status(service: State<'_, Arc<Diagnostics>>) -> Status {
    service.status()
}
#[tauri::command]
pub async fn open_diagnostics_directory(
    app: tauri::AppHandle,
    service: State<'_, Arc<Diagnostics>>,
) -> AppResult<()> {
    let s = service.inner().clone();
    crate::blocking(move || s.flush().map_err(convert)).await?;
    if !store::directory(&service.root) {
        return Err(AppError::new("diagnostics", "诊断目录不可用"));
    }
    app.opener()
        .open_path(service.root.to_string_lossy().to_string(), None::<String>)
        .map_err(|_| AppError::new("diagnostics", "无法打开文件夹，请复制日志路径后手动打开"))
}
struct Busy(Arc<Diagnostics>, #[allow(dead_code)] std::fs::File);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.exporting.store(false, Ordering::Release);
        *self.0.export_owner.lock().unwrap() = None;
        self.0.progress("idle", 0, 0);
    }
}
fn busy(service: Arc<Diagnostics>, owner: &str) -> AppResult<Busy> {
    if service.status().unavailable {
        return Err(AppError::new("diagnostics", "本地诊断不可用"));
    }
    if service
        .exporting
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return Err(AppError::new("diagnostics", "另一个日志操作正在进行"));
    }
    let file = store::open(&service.root.join(".export.lock")).and_then(|f| {
        f.try_lock().map_err(std::io::Error::other)?;
        Ok(f)
    });
    match file {
        Ok(file) => {
            service.cancelled.store(false, Ordering::Relaxed);
            *service.export_owner.lock().unwrap() = Some(owner.into());
            Ok(Busy(service, file))
        }
        Err(_) => {
            service.exporting.store(false, Ordering::Release);
            Err(AppError::new("diagnostics", "其他实例正在打包或清理日志"))
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    path: String,
    warnings: usize,
}
#[tauri::command]
pub async fn export_diagnostics(
    window: WebviewWindow,
    app: tauri::AppHandle,
    service: State<'_, Arc<Diagnostics>>,
    scope: snapshot::Scope,
) -> AppResult<Option<ExportResult>> {
    let service = service.inner().clone();
    let _busy = busy(service.clone(), window.label())?;
    service.progress("choosing", 0, 0);
    let name = format!(
        "SoDeck-diagnostics-{}.zip",
        event::timestamp(event::now()).replace([':', '.'], "")
    );
    let selected = rfd::AsyncFileDialog::new()
        .set_title("保存 SoDeck 本地诊断日志")
        .set_file_name(&name)
        .add_filter("ZIP 日志包", &["zip"])
        .save_file()
        .await;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let destination = selected.path().to_path_buf();
    if service.cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    // Never overwrite a file used by the recorder or its staging area.
    if destination
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .is_some_and(|p| service.root.canonicalize().is_ok_and(|r| p.starts_with(r)))
    {
        return Err(AppError::new("diagnostics", "请将日志包保存到诊断目录之外"));
    }
    service.progress("flushing", 0, 0);
    let token = event::id();
    ACKS.get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(token.clone(), HashSet::new());
    let windows: HashSet<_> = app.webview_windows().keys().cloned().collect();
    // A window that never completed registration may have failed during boot;
    // include it so a missing acknowledgement remains visible in the manifest.
    let expected = windows;
    let _ = app.emit("diagnostics-flush-request", &token);
    let wait_token = token.clone();
    let missing = crate::blocking(move || {
        let started = Instant::now();
        loop {
            let acked = ACKS
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .get(&wait_token)
                .cloned()
                .unwrap_or_default();
            if expected.is_subset(&acked) || started.elapsed() > Duration::from_secs(2) {
                ACKS.get().unwrap().lock().unwrap().remove(&wait_token);
                return Ok(expected.difference(&acked).count());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })
    .await?;
    let s = service.clone();
    let path = destination.clone();
    let warnings = crate::blocking(move || {
        snapshot::cleanup(&s.root).map_err(convert)?;
        s.progress("snapshot", 0, 0);
        let snapshot = s.snapshot(scope).map_err(convert)?;
        let missing = if missing > 0 {
            vec![format!(
                "{missing} 个窗口未在时限内确认提交缓冲，前端事件可能不完整"
            )]
        } else {
            vec![]
        };
        snapshot.zip(&path, &s, missing).map_err(convert)
    })
    .await?;
    *service.exported.lock().unwrap() = Some(destination.clone());
    Ok(Some(ExportResult {
        path: destination.to_string_lossy().into(),
        warnings,
    }))
}
#[tauri::command]
pub fn cancel_diagnostics_export(window: WebviewWindow, service: State<'_, Arc<Diagnostics>>) {
    if service.export_owner.lock().unwrap().as_deref() == Some(window.label()) {
        service.cancelled.store(true, Ordering::Relaxed);
    }
}
#[tauri::command]
pub fn reveal_diagnostics_export(
    app: tauri::AppHandle,
    service: State<'_, Arc<Diagnostics>>,
) -> AppResult<()> {
    let path = service
        .exported
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| AppError::new("diagnostics", "尚未保存日志包"))?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| AppError::new("diagnostics", "无法定位日志包，请手动打开保存位置"))
}
#[tauri::command]
pub async fn clear_diagnostics(
    window: WebviewWindow,
    service: State<'_, Arc<Diagnostics>>,
) -> AppResult<()> {
    let service = service.inner().clone();
    let _busy = busy(service.clone(), window.label())?;
    crate::blocking(move || {
        snapshot::cleanup(&service.root).map_err(convert)?;
        service.clear().map_err(convert)
    })
    .await
}
