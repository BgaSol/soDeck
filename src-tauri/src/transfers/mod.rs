mod direct;
pub mod download;
mod model;
mod pipeline;
mod planner;
mod recovery;
use recovery::{cleanup, verify_stopped};
mod upload;
use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    moves, remote_path,
    ssh_transport::Cancellation,
    State,
};
pub use download::ConflictPolicy;
pub(crate) use model::DirectEndpoint;
use model::{Receipt, WriteHandle};
pub use model::{Source, TaskView};
pub use planner::normalize_selection;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};

#[derive(Default)]
pub struct TransferState {
    downloads: Mutex<Vec<download::DownloadHandle>>,
    writes: Mutex<Vec<WriteHandle>>,
    active: Mutex<usize>,
    slot: Condvar,
    lanes: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    receipts_path: Mutex<Option<PathBuf>>,
    recovered: Mutex<Vec<Receipt>>,
    history: Mutex<Vec<TaskView>>,
    operations: Mutex<HashMap<String, (SharedContext, Cancellation)>>,
}
impl TransferState {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(&path)?;
        let mut recovered = Vec::new();
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|e| e != "json")
                || entry.metadata()?.len() > 2 * 1024 * 1024
            {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if !value.is_object() {
                continue;
            }
            if value.get("formatVersion").and_then(|v| v.as_u64()) != Some(2) {
                value["formatVersion"] = 0.into();
                value["pendingCommit"] = serde_json::Value::Null;
                value["direct"] = serde_json::Value::Null;
            }
            if let Ok(mut receipt) = serde_json::from_value::<Receipt>(value) {
                if let Some(view) = receipt.view.as_mut() {
                    view.status = "unknown".into();
                    view.error = Some(if receipt.format_version == 2 {
                        "上次运行未完成清理。请连接相关服务器后核对状态。"
                    } else { "旧格式任务记录已保留。请人工检查远端遗留任务；不会自动重试或删除远端文件。" }.into());
                    recovered.push(receipt);
                }
            }
        }
        Ok(Self {
            receipts_path: Mutex::new(Some(path)),
            recovered: Mutex::new(recovered),
            ..Self::default()
        })
    }
    fn write_lane(&self, identity: &str) -> Arc<Mutex<()>> {
        let mut lanes = self.lanes.lock().unwrap();
        if let Some(lane) = lanes.get(identity).and_then(Weak::upgrade) {
            return lane;
        }
        let lane = Arc::new(Mutex::new(()));
        lanes.insert(identity.into(), Arc::downgrade(&lane));
        lane
    }
    fn requires_reconciliation(&self, item: &WriteHandle) -> bool {
        let key = item.write_key();
        let id = item.task.lock().unwrap().id.clone();
        let matches =
            |r: &Receipt| r.target_write_key == key || r.target_identity == item.target.identity();
        let live = self
            .writes
            .lock()
            .unwrap()
            .iter()
            .filter(|w| !Arc::ptr_eq(&w.task, &item.task))
            .any(|w| {
                let t = w.task.lock().unwrap();
                let r = w.receipt.lock().unwrap();
                matches(&r)
                    && (t.status == "unknown"
                        || r.has_remote_resources()
                        || !r.stage_dirs.is_empty()
                        || r.pending_commit.is_some())
            });
        live || self
            .recovered
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.view.as_ref().is_none_or(|v| v.id != id) && matches(r))
    }
    fn persist(&self, item: &WriteHandle) -> AppResult<()> {
        let mut view = item.task.lock().unwrap().clone();
        // Receipts carry cleanup metadata, not an unbounded per-item log.
        view.warnings = view
            .warnings
            .iter()
            .take(20)
            .map(|w| w.chars().take(1024).collect())
            .collect();
        let mut receipt = item.receipt.lock().unwrap().clone();
        receipt.view = Some(view.clone());
        if let Some(root) = self.receipts_path.lock().unwrap().as_ref() {
            let mut output = tempfile::NamedTempFile::new_in(root)?;
            serde_json::to_writer(&mut output, &receipt).map_err(|e| AppError::new("io", e))?;
            output.as_file().sync_all()?;
            output
                .persist(root.join(format!("{}.json", view.id)))
                .map_err(|e| AppError::from(e.error))?;
        }
        Ok(())
    }
    fn remove_receipt(&self, id: &str) {
        if let Some(root) = self.receipts_path.lock().unwrap().as_ref() {
            let _ = fs::remove_file(root.join(format!("{id}.json")));
        }
    }
    pub fn has_activity(&self) -> bool {
        self.downloads
            .lock()
            .unwrap()
            .iter()
            .any(|d| model::active(&d.task.lock().unwrap().status))
            || self.writes.lock().unwrap().iter().any(|w| {
                let t = w.task.lock().unwrap();
                let receipt = w.receipt.lock().unwrap();
                t.active()
                    || t.status == "unknown"
                    || receipt.has_remote_resources()
                    || !receipt.stage_dirs.is_empty()
            })
            || !self.recovered.lock().unwrap().is_empty()
            || !self.operations.lock().unwrap().is_empty()
    }
    pub fn has_server_activity(&self, id: &str) -> bool {
        self.downloads
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.spec.uses_server(id) && model::active(&d.task.lock().unwrap().status))
            || self.writes.lock().unwrap().iter().any(|w| {
                w.uses_server(id) && {
                    let t = w.task.lock().unwrap();
                    let receipt = w.receipt.lock().unwrap();
                    t.active()
                        || t.status == "unknown"
                        || receipt.has_remote_resources()
                        || !receipt.stage_dirs.is_empty()
                }
            })
            || self
                .recovered
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.source_server_id == id || r.target_server_id == id)
            || self
                .operations
                .lock()
                .unwrap()
                .values()
                .any(|(context, _)| context.uses_server(id))
    }
    pub fn cancel_all(&self) {
        for (_, cancel) in self.operations.lock().unwrap().values() {
            cancel.cancel();
        }
        for item in self.downloads.lock().unwrap().iter() {
            item.cancel.cancel();
        }
        for item in self.writes.lock().unwrap().iter() {
            if item.task.lock().unwrap().active() {
                item.request_cancel();
            }
        }
        self.slot.notify_all();
    }
    pub fn wait_transfers(&self) -> AppResult<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let pending = self
                .downloads
                .lock()
                .unwrap()
                .iter()
                .any(|d| model::active(&d.task.lock().unwrap().status))
                || self
                    .writes
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|w| w.task.lock().unwrap().active())
                || !self.operations.lock().unwrap().is_empty();
            let active = self.active.lock().unwrap();
            if !pending && *active == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(AppError::new("busy", "传输进程仍在停止，请稍后重试退出"));
            }
            drop(
                self.slot
                    .wait_timeout(active, Duration::from_millis(100))
                    .unwrap(),
            );
        }
        // An unavailable source must not trap the user in the application.
        // The remote leases stop the owned processes, and durable receipts require
        // reconciliation after reopening rather than silently retrying.
        for item in self.writes.lock().unwrap().iter() {
            if item.task.lock().unwrap().status == "unknown" {
                self.persist(item)?;
            }
        }
        Ok(())
    }
}

pub struct FileOperation {
    state: Arc<State>,
    id: String,
    pub cancel: Cancellation,
}
impl Drop for FileOperation {
    fn drop(&mut self) {
        self.state
            .transfers
            .operations
            .lock()
            .unwrap()
            .remove(&self.id);
        self.state.transfers.slot.notify_all();
    }
}
pub fn register_file_operation(
    state: Arc<State>,
    context: SharedContext,
) -> AppResult<FileOperation> {
    let _gate = state.configuration.lock().unwrap();
    state.check_context(&context)?;
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = Cancellation::default();
    state
        .transfers
        .operations
        .lock()
        .unwrap()
        .insert(id.clone(), (context, cancel.clone()));
    drop(_gate);
    Ok(FileOperation { state, id, cancel })
}

pub fn directory_changed(app: &tauri::AppHandle, identity: &str, path: &str) {
    if let Some(state) = app.try_state::<Arc<State>>() {
        let targets: Vec<_> = state
            .terminals
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| t.spec.as_ref().is_some_and(|s| s.identity() == identity))
            .map(|(id, t)| (id.clone(), t.window.clone()))
            .collect();
        for (id, window) in targets {
            let _ = app.emit_to(
                &window,
                "remote-directory-changed",
                serde_json::json!({"sessionId":id,"path":path}),
            );
        }
    }
}
fn find_write(state: &State, id: &str) -> AppResult<WriteHandle> {
    state
        .transfers
        .writes
        .lock()
        .unwrap()
        .iter()
        .find(|w| w.task.lock().unwrap().id == id)
        .cloned()
        .ok_or_else(|| AppError::new("notFound", "传输任务不存在"))
}

pub fn start_write(
    app: tauri::AppHandle,
    state: Arc<State>,
    source: Source,
    target: SharedContext,
    destination: String,
    conflict: ConflictPolicy,
    endpoint_snapshot: Option<DirectEndpoint>,
) -> AppResult<String> {
    remote_path::absolute(&destination)?;
    let _gate = state.configuration.lock().unwrap();
    state.check_context(&target)?;
    let (kind, source_name, source_path, endpoint) = match &source {
        Source::Local(paths) => {
            if paths.is_empty() {
                return Err(AppError::new("validation", "没有选择上传文件"));
            }
            (
                "upload",
                "本机".to_owned(),
                paths
                    .iter()
                    .take(4)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
                None,
            )
        }
        Source::Remote { context, paths } => {
            state.check_context(context)?;
            planner::normalize_selection(paths)?;
            if context.target().id == target.target().id || context.identity() == target.identity()
            {
                return Err(AppError::new(
                    "validation",
                    "服务器间传输请选择另一台服务器",
                ));
            }
            let endpoint = endpoint_snapshot.ok_or_else(|| {
                AppError::new("validation", "请先为目标服务器填写真实地址和直连端口")
            })?;
            remote_path::validate_host(&endpoint.host)?;
            if endpoint.username != target.target().username
                || endpoint.public_key != target.target_public_key()
            {
                return Err(AppError::new("validation", "直连目标与已验证会话不一致"));
            }
            (
                "direct",
                context.target().name.clone(),
                paths.iter().take(4).cloned().collect::<Vec<_>>().join("\n"),
                Some(endpoint),
            )
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let task = TaskView {
        id: id.clone(),
        run_id: uuid::Uuid::new_v4().to_string(),
        kind: kind.into(),
        source_name,
        source_path,
        target_name: target.target().name.clone(),
        target_path: destination,
        conflict,
        status: "queued".into(),
        bytes_done: 0,
        bytes_total: 0,
        current_file: None,
        current_bytes: 0,
        committed_bytes: 0,
        waiting_reason: None,
        files_done: 0,
        files_total: 0,
        skipped: 0,
        error: None,
        warnings: Vec::new(),
        cleanup_pending: false,
    };
    let mut receipt = Receipt {
        format_version: 2,
        target_server_id: target.target().id.clone(),
        target_identity: target.identity().into(),
        target_write_key: format!(
            "{}|{}",
            target.target_public_key(),
            target.target().username
        ),
        ..Receipt::default()
    };
    if let Source::Remote { context, .. } = &source {
        receipt.source_server_id = context.target().id.clone();
        receipt.source_identity = context.identity().into();
    }
    let item = WriteHandle {
        lifecycle: Arc::new(Mutex::new(())),
        source,
        target,
        endpoint,
        task: Arc::new(Mutex::new(task)),
        cancel: Cancellation::default(),
        stop: Arc::new(AtomicBool::new(false)),
        plan: Arc::new(Mutex::new(None)),
        receipt: Arc::new(Mutex::new(receipt)),
    };
    state.transfers.persist(&item)?;
    state.transfers.writes.lock().unwrap().push(item.clone());
    drop(_gate);
    launch_write(app, state, item);
    Ok(id)
}
fn launch_write(app: tauri::AppHandle, state: Arc<State>, item: WriteHandle) {
    std::thread::spawn(move || {
        let _lifecycle = item.lifecycle.lock().unwrap();
        let result = loop {
            while state.transfers.requires_reconciliation(&item)
                && !item.stop.load(Ordering::SeqCst)
            {
                let mut task = item.task.lock().unwrap();
                task.status = "queued".into();
                task.waiting_reason = Some("等待目标上的任务结束或完成状态核对".into());
                drop(task);
                std::thread::sleep(Duration::from_millis(200));
            }
            item.task.lock().unwrap().waiting_reason = None;
            let mut active = state.transfers.active.lock().unwrap();
            while *active >= 3 && !item.stop.load(Ordering::SeqCst) {
                active = state
                    .transfers
                    .slot
                    .wait_timeout(active, Duration::from_millis(200))
                    .unwrap()
                    .0;
            }
            *active += 1;
            drop(active);
            let result = pipeline::run(&state, Some(&app), &item);
            if result.as_ref().is_err_and(|e| e.kind == "quarantined") {
                // A previous writer may have become uncertain while this task
                // waited for the lane. Return its global slot before waiting.
                *state.transfers.active.lock().unwrap() -= 1;
                state.transfers.slot.notify_all();
                continue;
            }
            break result;
        };
        if !result.as_ref().is_err_and(|e| e.kind == "unknown") {
            if let Err(e) = cleanup(&item) {
                item.warn(format!("临时资源仍需清理：{}", e.message));
            }
        }
        {
            let mut task = item.task.lock().unwrap();
            match result {
                Ok(()) => task.status = "completed".into(),
                Err(e) => {
                    task.status = if e.kind == "unknown" {
                        "unknown"
                    } else if item.stop.load(Ordering::SeqCst) || e.kind == "cancelled" {
                        "cancelled"
                    } else if task.files_done > 0 {
                        "partialFailed"
                    } else {
                        "failed"
                    }
                    .into();
                    task.error = Some(e.message);
                }
            }
        }
        let receipt = item.receipt.lock().unwrap().clone();
        let view = item.task.lock().unwrap().clone();
        if view.status != "unknown"
            && !receipt.has_remote_resources()
            && receipt.stage_dirs.is_empty()
        {
            state.transfers.remove_receipt(&view.id);
        } else if let Err(e) = state.transfers.persist(&item) {
            item.warn(format!("任务回执保存失败：{}", e.message));
        }
        let _ = app.emit("transfer-updated", &view);
        *state.transfers.active.lock().unwrap() -= 1;
        state.transfers.slot.notify_all();
    });
}

#[tauri::command]
pub fn list_transfers(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Vec<TaskView>> {
    moves::workspace_only(&window)?;
    let mut tasks: Vec<_> = state
        .transfers
        .downloads
        .lock()
        .unwrap()
        .iter()
        .map(|d| {
            let t = d.task.lock().unwrap();
            TaskView {
                id: t.id.clone(),
                run_id: t.run_id.clone(),
                kind: "download".into(),
                source_name: t.server_name.clone(),
                source_path: t.remote_path.clone(),
                target_name: "本机".into(),
                target_path: t.local_path.clone(),
                conflict: t.conflict,
                status: t.status.as_str().into(),
                bytes_done: t.bytes_done,
                bytes_total: t.bytes_total,
                current_file: None,
                current_bytes: 0,
                committed_bytes: t.bytes_done,
                waiting_reason: None,
                files_done: t.files_done,
                files_total: t.files_total,
                skipped: t.skipped,
                error: t.error.clone(),
                warnings: Vec::new(),
                cleanup_pending: false,
            }
        })
        .collect();
    tasks.extend(state.transfers.writes.lock().unwrap().iter().map(|w| {
        let mut view = w.task.lock().unwrap().clone();
        let receipt = w.receipt.lock().unwrap();
        view.cleanup_pending = receipt.has_remote_resources() || !receipt.stage_dirs.is_empty();
        view
    }));
    tasks.extend(
        state
            .transfers
            .recovered
            .lock()
            .unwrap()
            .iter()
            .filter_map(|r| r.view.clone()),
    );
    tasks.reverse();
    tasks.extend(state.transfers.history.lock().unwrap().iter().cloned());
    Ok(tasks)
}
#[tauri::command]
pub fn cancel_transfer(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    if let Ok(item) = find_write(&state, &id) {
        let mut task = item.task.lock().unwrap();
        if task.active() {
            task.status = "cancelling".into();
            item.request_cancel();
        }
        state.transfers.slot.notify_all();
        Ok(())
    } else {
        download::cancel_download(window, state, id)
    }
}
#[tauri::command]
pub fn retry_transfer(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    if let Ok(item) = find_write(&state, &id) {
        let _gate = state.configuration.lock().unwrap();
        let lifecycle = item.lifecycle.clone();
        let _lifecycle = lifecycle
            .try_lock()
            .map_err(|_| AppError::new("busy", "上次任务仍在收尾，请稍后重试"))?;
        state.ensure_running()?;
        state.check_context(&item.target)?;
        if let Source::Remote { context, .. } = &item.source {
            state.check_context(context)?;
        }
        {
            let mut task = item.task.lock().unwrap();
            if !matches!(
                task.status.as_str(),
                "failed" | "partialFailed" | "cancelled"
            ) {
                return Err(AppError::new("busy", "请先确认上一次任务已结束"));
            }
            let receipt = item.receipt.lock().unwrap();
            if receipt.has_remote_resources()
                || !receipt.stage_dirs.is_empty()
                || receipt.pending_commit.is_some()
            {
                return Err(AppError::new("busy", "请先核对并清理上次任务的临时资源"));
            }
            item.cancel.reset()?;
            item.stop.store(false, Ordering::SeqCst);
            task.run_id = uuid::Uuid::new_v4().to_string();
            task.status = "queued".into();
            task.error = None;
            task.current_file = None;
            task.current_bytes = 0;
            task.bytes_done = task.committed_bytes;
        }
        drop(_gate);
        launch_write(app, state.inner().clone(), item);
        Ok(())
    } else {
        download::retry_download(window, state, id)
    }
}

#[tauri::command]
pub async fn reconcile_transfer(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let recovered = find_write(&state, &id).is_err();
    let item = match find_write(&state, &id) {
        Ok(item) => {
            if item.task.lock().unwrap().active() {
                return Err(AppError::new("busy", "任务仍在运行"));
            }
            item
        }
        Err(_) => {
            let receipt = state
                .transfers
                .recovered
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.view.as_ref().is_some_and(|v| v.id == id))
                .cloned()
                .ok_or_else(|| AppError::new("notFound", "任务回执不存在"))?;
            let find_context = |identity: &str| -> AppResult<SharedContext> {
                state
                    .terminals
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|t| !t.sender.cancel.is_cancelled())
                    .filter_map(|t| t.spec.clone())
                    .find(|s| s.identity() == identity)
                    .ok_or_else(|| {
                        AppError::new(
                            "disconnected",
                            "请先重新连接任务涉及的服务器（账号、路径和主机指纹需保持一致）",
                        )
                    })
            };
            let target = find_context(&receipt.target_identity)?;
            let source = if receipt.source_identity.is_empty() {
                Source::Local(Vec::new())
            } else {
                Source::Remote {
                    context: find_context(&receipt.source_identity)?,
                    paths: vec![],
                }
            };
            WriteHandle {
                lifecycle: Arc::new(Mutex::new(())),
                source,
                target,
                endpoint: None,
                task: Arc::new(Mutex::new(receipt.view.clone().unwrap())),
                cancel: Cancellation::default(),
                stop: Arc::new(AtomicBool::new(false)),
                plan: Arc::new(Mutex::new(None)),
                receipt: Arc::new(Mutex::new(receipt)),
            }
        }
    };
    let state = state.inner().clone();
    crate::blocking(move || {
        let reconciliation = state.transfers.write_lane(&format!("reconcile:{id}"));
        let _reconciling = reconciliation.try_lock().map_err(|_| AppError::new("busy", "此任务正在核对中"))?;
        let _lifecycle = item.lifecycle.try_lock().map_err(|_| AppError::new("busy", "任务仍在运行或核对中"))?;
        if item.task.lock().unwrap().active() {
            return Err(AppError::new("busy", "任务仍在运行"));
        }
        if item.receipt.lock().unwrap().format_version != 2 {
            return Err(AppError::new("unsupported", "旧格式回执需人工核对远端状态，应用不会据此执行清理"));
        }
        verify_stopped(&item)?;
        state.transfers.persist(&item)?;
        let pending_commit = item.receipt.lock().unwrap().pending_commit.clone();
        cleanup(&item)?;
        item.receipt.lock().unwrap().pending_commit = None;
        {
            let mut task = item.task.lock().unwrap();
            if let Some(commit) = pending_commit {
                task.status = "interrupted".into();
                task.error = Some(format!("已确认远端停止。提交结果未获确认，请检查目标项目 {}。为避免重复写入，此任务不自动重试；确认后可重新选择需要的项目。", commit.destination));
            } else if task.status == "unknown" {
                task.status = if recovered { "interrupted" } else { "cancelled" }.into();
                task.error = Some("已确认远端停止并清理未提交内容。已完成文件保留。".into());
            }
            task.cleanup_pending = false;
            if recovered { state.transfers.history.lock().unwrap().push(task.clone()); }
        }
        state.transfers.remove_receipt(&id);
        state
            .transfers
            .recovered
            .lock()
            .unwrap()
            .retain(|r| r.view.as_ref().is_none_or(|v| v.id != id));
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests;
