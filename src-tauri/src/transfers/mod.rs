mod direct;
pub mod download;
mod model;
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
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
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
    store: Mutex<Option<task_store::TaskStore>>,
    pub(crate) paths: scheduler::PathLocks,
    recovered: Mutex<Vec<Receipt>>,
    history: Mutex<Vec<TaskView>>,
    operations: Mutex<HashMap<String, (SharedContext, Cancellation)>>,
}
impl TransferState {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&path)?;
        let store = task_store::TaskStore::open(&path.join("tasks-v3.sqlite"))?;
        let recovered = store.load()?;
        Ok(Self {
            store: Mutex::new(Some(store)),
            recovered: Mutex::new(recovered),
            ..Self::default()
        })
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
        item.task.lock().unwrap().waiting_reason = None;
        *item.auth.lock().unwrap() = None;
        state.drags.clipboard.finish(&app, &item);
        if let Err(e) = state.transfers.persist(&item) {
            item.warn(format!("任务保存失败：{}", e.message));
        }
        let view = item.task.lock().unwrap().clone();
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
                items: vec![],
                direct_address: None,
                temporary_paths: vec![],
                can_resume: false,
            }
        })
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
    let item = match recover_handle(&state, &id) {
        Ok(item) => item,
        Err(e) if e.kind == "notFound" => return download::retry_download(window, state, id),
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
    item.cancel.reset()?;
    if let Some(id) = item.receipt.lock().unwrap().clipboard_id.as_deref() {
        state.drags.clipboard.reserve_retry(id)?;
    }
    item.stop.store(false, Ordering::SeqCst);
    {
        let mut t = item.task.lock().unwrap();
        t.run_id = uuid::Uuid::new_v4().to_string();
        t.status = "queued".into();
        t.error = None;
        t.waiting_reason = None;
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
        let _life = item
            .lifecycle
            .try_lock()
            .map_err(|_| AppError::new("busy", "任务仍在运行"))?;
        if item.task.lock().unwrap().active() {
            return Err(AppError::new("busy", "任务仍在运行"));
        }
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
        let _life = item
            .lifecycle
            .try_lock()
            .map_err(|_| AppError::new("busy", "任务仍在运行"))?;
        if item.task.lock().unwrap().active() {
            return Err(AppError::new("busy", "任务仍在运行"));
        }
        let result = cleanup(&item);
        state.transfers.persist(&item)?;
        result
    })
    .await
}
#[cfg(test)]
mod tests;
