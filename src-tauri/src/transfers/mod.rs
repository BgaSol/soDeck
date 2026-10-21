mod direct;
pub mod download;
pub(crate) mod model;
mod pipeline;
pub(crate) mod planner;
mod recovery;
use recovery::cleanup;
mod same_server;
pub(crate) mod scheduler;
mod task_store;
mod upload;
mod verify;
use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    moves, remote_path,
    ssh_transport::Cancellation,
    State,
};
pub use download::ConflictPolicy;
pub(crate) use model::AuthInput;
pub(crate) use model::DirectEndpoint;
use model::Receipt;
pub(crate) use model::WriteHandle;
pub(crate) use model::{operation_kind, Intent};
pub use model::{Source, TaskView};
pub use planner::normalize_selection;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};

#[derive(Default)]
pub struct TransferState {
    next_order: AtomicU64,
    downloads: Mutex<Vec<download::DownloadHandle>>,
    recovered_downloads: Mutex<Vec<download::DownloadReceipt>>,
    writes: Mutex<Vec<WriteHandle>>,
    active: Mutex<usize>,
    slot: Condvar,
    store: Mutex<Option<task_store::TaskStore>>,
    pub(crate) paths: scheduler::PathLocks,
    recovered: Mutex<Vec<Receipt>>,
    history: Mutex<Vec<TaskView>>,
    operations: Mutex<HashMap<String, (SharedContext, Cancellation)>>,
}
impl TransferState {
    pub(crate) fn deletion_conflict(
        &self,
        key: &str,
        paths: &[String],
    ) -> Option<(String, String)> {
        for download in self.downloads.lock().unwrap().iter() {
            let task = download.task.lock().unwrap();
            if download.spec.target_public_key() == key
                && model::active(&task.status)
                && paths
                    .iter()
                    .any(|p| scheduler::overlaps(p, &task.remote_path))
            {
                return Some((task.id.clone(), task.remote_path.clone()));
            }
        }
        for write in self.writes.lock().unwrap().iter() {
            let task = write.task.lock().unwrap().clone();
            if !task.active() {
                continue;
            }
            if let Source::Remote {
                context,
                paths: sources,
            } = &write.source
            {
                if context.target_public_key() == key {
                    if let Some(path) = sources
                        .iter()
                        .find(|source| paths.iter().any(|p| scheduler::overlaps(p, source)))
                    {
                        return Some((task.id.clone(), path.clone()));
                    }
                }
            }
            if write.write_key() == key {
                // Before a plan exists, protect the destination while the task discovers its outputs.
                let plan = write.plan.lock().unwrap();
                let destinations: Vec<_> = plan
                    .as_ref()
                    .map(|p| {
                        p.entries
                            .iter()
                            .filter_map(|e| e.target.as_deref())
                            .collect()
                    })
                    .unwrap_or_else(|| vec![task.target_path.as_str()]);
                if let Some(path) = destinations
                    .iter()
                    .find(|target| paths.iter().any(|p| scheduler::overlaps(p, target)))
                {
                    return Some((task.id.clone(), (*path).into()));
                }
            }
        }
        None
    }
    pub fn open(path: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&path)?;
        let store = task_store::TaskStore::open(&path.join("tasks-v3.sqlite"))?;
        let recovered = store.load()?;
        let last_order = recovered
            .iter()
            .filter_map(|r| r.view.as_ref())
            .map(|v| v.presentation.sort_order)
            .max()
            .unwrap_or(0);
        Ok(Self {
            next_order: AtomicU64::new(last_order),
            store: Mutex::new(Some(store)),
            recovered: Mutex::new(recovered),
            ..Self::default()
        })
    }
    pub(crate) fn next_order(&self) -> u64 {
        self.next_order.fetch_add(1, Ordering::SeqCst) + 1
    }
    pub(crate) fn observe_order(&self, order: u64) {
        self.next_order.fetch_max(order, Ordering::SeqCst);
    }
    pub(crate) fn restore_downloads(
        &self,
        history: &crate::file_tasks::FileTaskState,
    ) -> AppResult<()> {
        let store = history.store.lock().unwrap();
        let mut records = store.load::<download::DownloadReceipt>("download")?;
        for record in &mut records {
            self.observe_order(record.task.presentation.sort_order);
            if model::active(&record.task.status) {
                record.task.status = "interrupted".into();
                record.task.error =
                    Some("上次下载中断；已保存的文件保留，重试将重新检查来源和本地目标".into());
                store.save("download", &record.task.id, record)?;
            }
        }
        *self.recovered_downloads.lock().unwrap() = records;
        Ok(())
    }
    pub(crate) fn task_busy(&self, id: &str) -> bool {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .find(|w| w.task.lock().unwrap().id == id)
            .is_some_and(|w| w.lifecycle.try_lock().is_err())
    }
    pub(crate) fn has_pending_task(&self, id: &str) -> bool {
        self.writes.lock().unwrap().iter().any(|w| {
            w.task.lock().unwrap().id == id && w.receipt.lock().unwrap().pending_commit.is_some()
        }) || self
            .recovered
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.view.as_ref().is_some_and(|v| v.id == id) && r.pending_commit.is_some())
    }
    fn conflicting_commit(&self, item: &WriteHandle, path: &str) -> Option<String> {
        let key = item.write_key();
        let id = item.task.lock().unwrap().id.clone();
        self.pending_path(&key, path, Some(&id))
    }
    pub(crate) fn pending_path(
        &self,
        key: &str,
        path: &str,
        exclude: Option<&str>,
    ) -> Option<String> {
        let conflict = |r: &Receipt| {
            r.view
                .as_ref()
                .is_none_or(|v| Some(v.id.as_str()) != exclude)
                && ((r.target_write_key == key
                    && (r.pending_commit.as_ref().is_some_and(|c| {
                        scheduler::overlaps(&c.destination, path)
                            || (r.intent == Intent::Move
                                && c.stage_path.as_ref().is_some_and(|p| {
                                    r.source_paths
                                        .iter()
                                        .any(|root| p == root || p.starts_with(&format!("{root}/")))
                                        && scheduler::overlaps(p, path)
                                }))
                    }) || r.attempts.iter().any(|a| {
                        !a.target_root.is_empty() && scheduler::overlaps(&a.target_root, path)
                    })))
                    || (r.source_write_key == key
                        && r.attempts.iter().any(|a| {
                            a.source_root
                                .as_ref()
                                .is_some_and(|p| scheduler::overlaps(p, path))
                        })))
        };
        for w in self.writes.lock().unwrap().iter() {
            if Some(w.task.lock().unwrap().id.as_str()) == exclude {
                continue;
            }
            let r = w.receipt.lock().unwrap();
            if conflict(&r) {
                return Some(w.task.lock().unwrap().id.clone());
            }
        }
        self.recovered
            .lock()
            .unwrap()
            .iter()
            .find(|r| conflict(r))
            .and_then(|r| r.view.as_ref().map(|v| v.id.clone()))
    }
    fn persist(&self, item: &WriteHandle) -> AppResult<()> {
        let mut r = item.receipt.lock().unwrap().clone();
        let mut view = item.task.lock().unwrap().clone();
        r.update_resources(&mut view);
        crate::diagnostics::state_event("transfer", &view.id, view.status.as_str());
        r.view = Some(view);
        if let Some(store) = self.store.lock().unwrap().as_ref() {
            store.save(&r)?;
        }
        Ok(())
    }
    fn save_plan(&self, item: &WriteHandle, plan: &planner::Plan) -> AppResult<()> {
        let id = item.task.lock().unwrap().id.clone();
        if let Some(store) = self.store.lock().unwrap().as_mut() {
            store.plan(&id, plan)?;
        }
        *item.plan.lock().unwrap() = Some(plan.clone());
        item.receipt.lock().unwrap().skipped_paths = plan.skipped.clone();
        Ok(())
    }
    fn save_entry(
        &self,
        item: &WriteHandle,
        index: usize,
        entry: &planner::Entry,
    ) -> AppResult<()> {
        let id = item.task.lock().unwrap().id.clone();
        if let Some(store) = self.store.lock().unwrap().as_ref() {
            store.entry(&id, index, entry)?;
        }
        if let Some(plan) = item.plan.lock().unwrap().as_mut() {
            plan.entries[index] = entry.clone();
        }
        Ok(())
    }
    pub fn has_activity(&self) -> bool {
        self.downloads
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
            || !self.operations.lock().unwrap().is_empty()
    }
    pub fn has_server_activity(&self, id: &str) -> bool {
        self.downloads
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.spec.uses_server(id) && model::active(&d.task.lock().unwrap().status))
            || self
                .writes
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.uses_server(id) && w.task.lock().unwrap().active())
            || self
                .operations
                .lock()
                .unwrap()
                .values()
                .any(|(c, _)| c.uses_server(id))
    }
    pub fn cancel_all(&self) {
        for (_, cancel) in self.operations.lock().unwrap().values() {
            cancel.cancel();
        }
        self.stop_transfers();
    }
    pub fn interrupt_operations_after_grace(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !self.operations.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.cancel_all();
    }
    pub fn stop_transfers(&self) {
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
                return Err(AppError::new("busy", "文件任务仍在停止，请稍后重试退出"));
            }
            drop(
                self.slot
                    .wait_timeout(active, Duration::from_millis(100))
                    .unwrap(),
            );
        }
        // An unavailable source must not trap the user in the application.
        // Unconfirmed executions can only write isolated staging directories.
        // Durable receipts require reconciliation instead of automatic commit.
        for item in self.writes.lock().unwrap().iter() {
            self.persist(item)?;
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
    auth_input: Option<model::AuthInput>,
    verify_sha256: bool,
    intent: Intent,
    clipboard_id: Option<String>,
) -> AppResult<String> {
    remote_path::absolute(&destination)?;
    let _gate = state.configuration.lock().unwrap();
    state.check_context(&target)?;
    let operation = operation_kind(&source, &target, intent)?;
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
            let endpoint = if operation == "direct" {
                Some(endpoint_snapshot.ok_or_else(|| {
                    AppError::new("validation", "请先为目标服务器填写真实地址和直连端口")
                })?)
            } else {
                None
            };
            if let Some(endpoint) = &endpoint {
                remote_path::validate_host(&endpoint.host)?;
                if endpoint.username != target.target().username
                    || endpoint.public_key != target.target_public_key()
                {
                    return Err(AppError::new("validation", "直连目标与已验证会话不一致"));
                }
            }
            (
                operation,
                context.target().name.clone(),
                paths.iter().take(4).cloned().collect::<Vec<_>>().join("\n"),
                endpoint,
            )
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let task = TaskView {
        id: id.clone(),
        run_id: uuid::Uuid::new_v4().to_string(),
        presentation: model::TaskPresentation::new(
            state.transfers.next_order(),
            match &source {
                Source::Local(paths) => paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                Source::Remote { paths, .. } => paths.clone(),
            },
        ),
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
        items: vec![],
        temporary_paths: vec![],
        can_resume: false,
        direct_address: endpoint
            .as_ref()
            .map(|e| format!("{}@{}:{}", e.username, e.host, e.port)),
    };
    let mut receipt = Receipt {
        intent,
        clipboard_id,
        format_version: 3,
        target_server_id: target.target().id.clone(),
        target_identity: target.identity().into(),
        target_write_key: target.target_public_key().into(),
        endpoint: endpoint.clone(),
        verify_sha256: verify_sha256 || intent == Intent::Move,
        source_paths: match &source {
            Source::Local(p) => p.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            Source::Remote { paths, .. } => paths.clone(),
        },
        ..Receipt::default()
    };
    if let Source::Remote { context, .. } = &source {
        receipt.source_server_id = context.target().id.clone();
        receipt.source_identity = context.identity().into();
        receipt.source_write_key = context.target_public_key().into();
    }
    let auth = if operation == "direct" {
        Some(resolve_auth(&state, &target, auth_input.as_ref())?)
    } else {
        None
    };
    if let Some(a) = &auth {
        match a {
            model::DirectAuth::Password(_) => receipt.auth_mode = "password".into(),
            model::DirectAuth::Key(p) => {
                receipt.auth_mode = "key".into();
                receipt.key_path = Some(p.clone());
            }
        }
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
        auth: Arc::new(Mutex::new(auth)),
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
        let mut active = state.transfers.active.lock().unwrap();
        while *active >= 3 && !item.stop.load(Ordering::SeqCst) {
            item.task.lock().unwrap().waiting_reason = Some("等待传输名额（最多 3 项）".into());
            active = state
                .transfers
                .slot
                .wait_timeout(active, Duration::from_millis(100))
                .unwrap()
                .0;
        }
        *active += 1;
        drop(active);
        item.task.lock().unwrap().waiting_reason = None;
        let result = pipeline::run(&state, Some(&app), &item);
        if result.is_ok() {
            if let Err(e) = cleanup(&item) {
                item.warn(format!("临时资源仍需清理：{}", e.message));
            }
        }
        {
            let mut task = item.task.lock().unwrap();
            match result {
                Ok(()) => task.status = "completed".into(),
                Err(e) => {
                    crate::diagnostics::task_failure("transfer", &task.id, &e);
                    task.status = if e.kind == "unknown" || e.kind == "conflictPending" {
                        "needsAttention"
                    } else if e.kind == "interrupted" {
                        "interrupted"
                    } else if item.stop.load(Ordering::SeqCst) || e.kind == "cancelled" {
                        "cancelled"
                    } else if e.kind == "sourceRetained" {
                        "sourceRetained"
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
        item.task.lock().unwrap().presentation.finished_at = Some(crate::file_tasks::now());
        item.task.lock().unwrap().waiting_reason = None;
        *item.auth.lock().unwrap() = None;
        state.drags.clipboard.finish(&app, &item);
        if let Err(e) = state.transfers.persist(&item) {
            item.warn(format!("任务保存失败：{}", e.message));
        }
        let view = item.task.lock().unwrap().clone();
        let _ = app.emit("transfer-updated", &view);
        state.file_tasks.emit(&view.id);
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
    Ok(snapshot(&state))
}
pub(crate) fn snapshot(state: &State) -> Vec<TaskView> {
    let mut tasks: Vec<_> = state
        .transfers
        .downloads
        .lock()
        .unwrap()
        .iter()
        .map(|d| download::view(&d.task.lock().unwrap()))
        .collect();
    tasks.extend(state.transfers.writes.lock().unwrap().iter().map(|w| {
        let mut view = w.task.lock().unwrap().clone();
        let receipt = w.receipt.lock().unwrap();
        receipt.update_resources(&mut view);
        if let Some(plan) = w.plan.lock().unwrap().as_ref() {
            view.items = plan
                .entries
                .iter()
                .filter(|e| e.error.is_some())
                .take(200)
                .map(|e| model::ItemView {
                    source: e.source.clone(),
                    target: e.target.clone(),
                    status: e.outcome.clone(),
                    error: e.error.clone(),
                })
                .collect();
        }
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
    tasks.extend(state.transfers.history.lock().unwrap().iter().cloned());
    tasks.extend(
        state
            .transfers
            .recovered_downloads
            .lock()
            .unwrap()
            .iter()
            .map(|r| download::view(&r.task)),
    );
    tasks.extend(state.docker.transfer_snapshot());
    model::sort_tasks(&mut tasks);
    tasks
}
#[tauri::command]
pub fn cancel_transfer(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    if state.docker.cancel_transfer(&id) {
        state.file_tasks.emit(&id);
        return Ok(());
    }
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

fn resolve_auth(
    state: &State,
    target: &SharedContext,
    input: Option<&model::AuthInput>,
) -> AppResult<model::DirectAuth> {
    let input = input.ok_or_else(|| AppError::new("authentication", "请选择密码或 A 上的私钥"))?;
    match input.mode.as_str() {
        "savedPassword" => {
            let config = state.database.lock().unwrap().get(&target.target().id)?;
            if config.host != target.target().host
                || config.port != target.target().port
                || config.username != target.target().username
                || config.jump_server_id != target.target().jump_server_id
            {
                return Err(AppError::new(
                    "changed",
                    "服务器连接配置已变化，请重新连接并创建任务",
                ));
            }
            let p = config.password;
            if p.is_empty() {
                Err(AppError::new(
                    "authentication",
                    "目标没有可用密码，请输入本次密码",
                ))
            } else {
                Ok(model::DirectAuth::Password(p))
            }
        }
        "password" => Ok(model::DirectAuth::Password(
            input
                .password
                .clone()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| AppError::new("authentication", "请输入密码"))?,
        )),
        "key" => {
            let p = input
                .key_path
                .clone()
                .ok_or_else(|| AppError::new("authentication", "请输入 A 上的私钥绝对路径"))?;
            remote_path::absolute(&p)?;
            Ok(model::DirectAuth::Key(p))
        }
        _ => Err(AppError::new("authentication", "不支持的直传认证方式")),
    }
}
fn recover_handle(state: &State, id: &str) -> AppResult<WriteHandle> {
    if let Ok(item) = find_write(state, id) {
        return Ok(item);
    }
    let r = state
        .transfers
        .recovered
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.view.as_ref().is_some_and(|v| v.id == id))
        .cloned()
        .ok_or_else(|| AppError::new("notFound", "任务不存在"))?;
    let context = |identity: &str| -> AppResult<SharedContext> {
        state
            .terminals
            .lock()
            .unwrap()
            .values()
            .filter(|t| !t.sender.cancel.is_cancelled())
            .filter_map(|t| t.spec.clone())
            .find(|s| s.identity() == identity)
            .ok_or_else(|| AppError::new("disconnected", "请先重新连接任务涉及的服务器"))
    };
    let target = context(&r.target_identity)?;
    let source = if r.source_identity.is_empty() {
        Source::Local(r.source_paths.iter().map(PathBuf::from).collect())
    } else {
        Source::Remote {
            context: context(&r.source_identity)?,
            paths: r.source_paths.clone(),
        }
    };
    let item = WriteHandle {
        lifecycle: Arc::new(Mutex::new(())),
        source,
        target,
        endpoint: r.endpoint.clone(),
        task: Arc::new(Mutex::new(r.view.clone().unwrap())),
        cancel: Cancellation::default(),
        stop: Arc::new(AtomicBool::new(false)),
        plan: Arc::new(Mutex::new(Some(r.plan.clone()))),
        receipt: Arc::new(Mutex::new(r)),
        auth: Arc::new(Mutex::new(None)),
    };
    let mut writes = state.transfers.writes.lock().unwrap();
    if let Some(existing) = writes.iter().find(|w| w.task.lock().unwrap().id == id) {
        return Ok(existing.clone());
    }
    writes.push(item.clone());
    state
        .transfers
        .recovered
        .lock()
        .unwrap()
        .retain(|r| r.view.as_ref().is_none_or(|v| v.id != id));
    Ok(item)
}
#[tauri::command]
pub fn retry_transfer(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    id: String,
    auth: Option<model::AuthInput>,
    resume: Option<bool>,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let action = state.file_tasks.actions.lock().unwrap();
    let item = match recover_handle(&state, &id) {
        Ok(item) => item,
        Err(e) if e.kind == "notFound" => {
            drop(action);
            return download::retry_download(window, state, id);
        }
        Err(e) => return Err(e),
    };
    let _life = item
        .lifecycle
        .try_lock()
        .map_err(|_| AppError::new("busy", "任务仍在运行"))?;
    if item.task.lock().unwrap().active() {
        return Err(AppError::new("busy", "任务仍在运行"));
    }
    if item.receipt.lock().unwrap().pending_commit.is_some() {
        return Err(AppError::new(
            "conflictPending",
            "请先核对本任务的待确认目标文件",
        ));
    }
    state.ensure_running()?;
    state.check_context(&item.target)?;
    if let Source::Remote { context, .. } = &item.source {
        state.check_context(context)?;
        if !item.same_server() {
            *item.auth.lock().unwrap() = Some(resolve_auth(&state, &item.target, auth.as_ref())?);
        }
    }
    if resume.unwrap_or(false) {
        let r = item.receipt.lock().unwrap();
        let a = r
            .attempts
            .last()
            .ok_or_else(|| AppError::new("resume", "没有可复用数据"))?;
        if !a.source_finished || !a.receiver_finished {
            return Err(AppError::new(
                "resume",
                "旧执行未确认结束，请先核对；也可以使用新目录重试",
            ));
        }
    } else if let Some(plan) = item.plan.lock().unwrap().as_mut() {
        for e in &mut plan.entries {
            if e.outcome != "completed" && e.outcome != "skipped" {
                e.staged = None;
            }
        }
    }
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .restore_visibility(&id)?;
    item.cancel.reset()?;
    if let Some(id) = item.receipt.lock().unwrap().clipboard_id.as_deref() {
        state.drags.clipboard.reserve_retry(id)?;
    }
    item.stop.store(false, Ordering::SeqCst);
    let previous = {
        let mut t = item.task.lock().unwrap();
        let previous = t.clone();
        t.presentation.restart(
            state.transfers.next_order(),
            previous.run_id.clone(),
            previous.status.clone(),
            previous.error.clone(),
        );
        t.run_id = uuid::Uuid::new_v4().to_string();
        t.status = "queued".into();
        t.error = None;
        t.waiting_reason = None;
        previous
    };
    if let Err(error) = state.transfers.persist(&item) {
        *item.task.lock().unwrap() = previous;
        *item.auth.lock().unwrap() = None;
        state.drags.clipboard.finish(&app, &item);
        return Err(error);
    }
    drop(_life);
    launch_write(app, state.inner().clone(), item);
    Ok(())
}
#[tauri::command]
pub async fn reconcile_transfer(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let item = recover_handle(&state, &id)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let action = state.file_tasks.actions.lock().unwrap();
        let _life = item
            .lifecycle
            .try_lock()
            .map_err(|_| AppError::new("busy", "任务仍在运行"))?;
        if item.task.lock().unwrap().active() {
            return Err(AppError::new("busy", "任务仍在运行"));
        }
        drop(action);
        recovery::reconcile(&state, &item)?;
        state.drags.clipboard.finish(&app, &item);
        state.transfers.persist(&item)?;
        directory_changed(
            &app,
            item.target.identity(),
            &item.task.lock().unwrap().target_path,
        );
        if item.moving() {
            if let Source::Remote { context, paths } = &item.source {
                for p in paths {
                    directory_changed(&app, context.identity(), remote_path::parent(p));
                }
            }
        }
        let _ = app.emit("transfer-updated", item.task.lock().unwrap().clone());
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn cleanup_transfer(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let item = recover_handle(&state, &id)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let action = state.file_tasks.actions.lock().unwrap();
        let _life = item
            .lifecycle
            .try_lock()
            .map_err(|_| AppError::new("busy", "任务仍在运行"))?;
        if item.task.lock().unwrap().active() {
            return Err(AppError::new("busy", "任务仍在运行"));
        }
        drop(action);
        let result = cleanup(&item);
        state.transfers.persist(&item)?;
        state.file_tasks.emit(&id);
        result
    })
    .await
}
#[cfg(test)]
mod tests;
