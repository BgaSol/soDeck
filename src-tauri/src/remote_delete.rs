//! Confirmed, bounded SFTP deletion. Never follows links or automatically retries mutations.
use crate::{
    blocking,
    connection::{self, SharedContext},
    model::{AppError, AppResult},
    remote_path, remote_write,
    ssh_transport::Cancellation,
    transfers::{self, scheduler::Claim},
    State,
};
use serde::{Deserialize, Serialize};
use ssh2::{FileStat, Sftp};
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, State as TauriState, WebviewWindow};

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const DRAFT_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct DeleteState(Mutex<Registry>);
#[derive(Default)]
struct Registry {
    drafts: HashMap<String, Draft>,
    tasks: HashMap<String, Arc<Handle>>,
    recovered: HashMap<String, DeleteTask>,
}
struct Draft {
    owner: String,
    generation: u64,
    session_id: String,
    context: SharedContext,
    entries: Vec<(String, Option<Stamp>)>,
    expires: Instant,
    started: Option<String>,
    parent_task_id: Option<String>,
    require_session: bool,
}
struct Handle {
    context: SharedContext,
    view: Mutex<DeleteTask>,
    stop: AtomicBool,
    reconciling: AtomicBool,
    cancel: Cancellation,
    last_emit: Mutex<Instant>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
struct Stamp {
    kind: &'static str,
    size: Option<u64>,
    modified: Option<u64>,
    mode: Option<u32>,
}
impl Stamp {
    fn from_stat(stat: FileStat) -> AppResult<Self> {
        let kind = if stat.file_type().is_symlink() {
            "symlink"
        } else if stat.is_dir() {
            "directory"
        } else if stat.is_file() {
            "file"
        } else {
            return Err(AppError::new(
                "fileType",
                "不支持删除设备、管道或其他特殊文件",
            ));
        };
        Ok(Self {
            kind,
            size: stat.size,
            modified: stat.mtime,
            mode: stat.perm,
        })
    }
    fn matches_after_children(&self, current: &Self) -> bool {
        self.kind == current.kind
            && if self.kind == "directory" {
                self.mode == current.mode
            } else {
                self == current
            }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreview {
    id: String,
    server_name: String,
    entries: Vec<DeleteEntry>,
    parent_task_id: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEntry {
    pub(crate) path: String,
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) deleted_count: u64,
    pub(crate) missing_count: u64,
    pub(crate) failures: Vec<DeleteFailure>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteFailure {
    pub(crate) path: String,
    pub(crate) message: String,
    pub(crate) uncertain: bool,
    pub(crate) current_state: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTask {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) server_name: String,
    pub(crate) status: String,
    pub(crate) revision: u64,
    pub(crate) server_identity: String,
    pub(crate) server_id: String,
    pub(crate) created_at: u64,
    pub(crate) last_started_at: u64,
    pub(crate) finished_at: Option<u64>,
    pub(crate) sort_order: u64,
    pub(crate) entry_count: usize,
    pub(crate) parent_task_id: Option<String>,
    pub(crate) last_checked_at: Option<u64>,
    pub(crate) current_path: Option<String>,
    pub(crate) deleted_count: u64,
    pub(crate) missing_count: u64,
    pub(crate) entries: Vec<DeleteEntry>,
    pub(crate) affected_directories: Vec<String>,
    pub(crate) error: Option<String>,
}
fn active(status: &str) -> bool {
    matches!(status, "checking" | "scanning" | "running" | "cancelling")
}
fn entry(path: &str, stamp: Option<&Stamp>) -> DeleteEntry {
    DeleteEntry {
        path: path.into(),
        kind: stamp.map_or("missing", |s| s.kind).into(),
        status: "pending".into(),
        deleted_count: 0,
        missing_count: 0,
        failures: vec![],
    }
}
impl DeleteState {
    pub fn discard_window_drafts(&self, owner: &str) {
        self.0
            .lock()
            .unwrap()
            .drafts
            .retain(|_, d| d.owner != owner);
    }
    pub fn snapshot(&self) -> Vec<DeleteTask> {
        let registry = self.0.lock().unwrap();
        registry
            .tasks
            .values()
            .map(|h| h.view.lock().unwrap().clone())
            .chain(registry.recovered.values().cloned())
            .collect()
    }
    pub fn contains(&self, id: &str) -> bool {
        let registry = self.0.lock().unwrap();
        registry.tasks.contains_key(id) || registry.recovered.contains_key(id)
    }
    pub fn busy(&self, id: &str) -> bool {
        self.0
            .lock()
            .unwrap()
            .tasks
            .get(id)
            .is_some_and(|h| h.reconciling.load(Ordering::SeqCst))
    }
    pub fn stop_all(&self) {
        for handle in self.0.lock().unwrap().tasks.values() {
            handle.request_stop();
        }
    }
    pub fn restore(store: &mut crate::file_tasks::store::Store) -> AppResult<Self> {
        let mut registry = Registry::default();
        for mut view in store.load::<DeleteTask>("delete")? {
            let was_active = active(&view.status);
            for step in store.delete_steps(&view.id)? {
                apply_step(&mut view, &step);
            }
            if let Some((root, path)) = store.pending_delete(&view.id)? {
                if let Some(entry) = view.entries.get_mut(root) {
                    if !entry.failures.iter().any(|f| f.path == path && f.uncertain) {
                        entry.failures.push(DeleteFailure {
                            path,
                            message: "上次删除请求可能已发出，结果尚未确认".into(),
                            uncertain: true,
                            current_state: None,
                        });
                    }
                }
            }
            let unknown = view.entries.iter().any(|e| {
                e.failures
                    .iter()
                    .any(|f| f.uncertain && f.current_state.is_none())
            });
            if was_active || unknown {
                for entry in &mut view.entries {
                    if entry.status == "pending" {
                        entry.status = if entry.deleted_count > 0 {
                            "partial"
                        } else if !entry.failures.is_empty() {
                            "failed"
                        } else {
                            "notExecuted"
                        }
                        .into();
                    }
                }
                view.status = if unknown {
                    "unknown"
                } else if view
                    .entries
                    .iter()
                    .all(|e| matches!(e.status.as_str(), "deleted" | "missing"))
                {
                    "completed"
                } else {
                    "interrupted"
                }
                .into();
                view.current_path = None;
                view.revision += 1;
                view.error = (view.status != "completed")
                    .then(|| "上次执行中断；已确认结果已保留，不会自动继续删除".into());
            }
            recount(&mut view);
            store.checkpoint_delete(&view.id, &view, false)?;
            registry.recovered.insert(view.id.clone(), view);
        }
        Ok(Self(Mutex::new(registry)))
    }
}
fn recount(view: &mut DeleteTask) {
    view.deleted_count = view.entries.iter().map(|e| e.deleted_count).sum();
    view.missing_count = view.entries.iter().map(|e| e.missing_count).sum();
}
fn apply_step(view: &mut DeleteTask, step: &crate::file_tasks::store::DeleteStep) {
    let Some(entry) = view.entries.get_mut(step.root) else {
        return;
    };
    match step.outcome.as_str() {
        "deleted" => {
            entry.deleted_count += 1;
            if step.path == entry.path {
                entry.status = "deleted".into();
            }
        }
        "missing" => {
            entry.missing_count += 1;
            if step.path == entry.path {
                entry.status = "missing".into();
            }
        }
        kind => entry.failures.push(DeleteFailure {
            path: step.path.clone(),
            message: step.message.clone().unwrap_or_default(),
            uncertain: kind == "unknown",
            current_state: None,
        }),
    }
    let parent = remote_path::parent(&step.path).to_owned();
    if !view.affected_directories.contains(&parent) {
        view.affected_directories.push(parent);
    }
}

impl Handle {
    fn request_stop(&self) {
        let mut view = self.view.lock().unwrap();
        if active(&view.status) {
            self.stop.store(true, Ordering::SeqCst);
            view.status = "cancelling".into();
            view.revision += 1;
        }
    }
    fn check(&self) -> AppResult<()> {
        self.cancel.check()?;
        if self.stop.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "cancelled",
                "已停止后续删除，已删除的内容无法恢复",
            ));
        }
        Ok(())
    }
    fn update(&self, update: impl FnOnce(&mut DeleteTask)) {
        let mut view = self.view.lock().unwrap();
        update(&mut view);
        if active(&view.status) && self.stop.load(Ordering::SeqCst) {
            view.status = "cancelling".into();
        }
        view.revision += 1;
    }
    fn emit(&self, app: Option<&tauri::AppHandle>, force: bool) {
        let Some(app) = app else {
            return;
        };
        let mut last = self.last_emit.lock().unwrap();
        if force || last.elapsed() >= Duration::from_millis(150) {
            *last = Instant::now();
            let view = self.view.lock().unwrap();
            let _ = app.emit(
                "file-task-updated",
                serde_json::json!({"taskId":view.id,"revision":view.revision}),
            );
            let _ = app.emit(
                "remote-delete-updated",
                serde_json::json!({"taskId": view.id, "revision": view.revision}),
            );
        }
    }
}

pub(crate) fn normalize_paths(paths: &[String]) -> AppResult<Vec<String>> {
    // Existing normalization already rejects root aliases and removes selected descendants.
    transfers::normalize_selection(paths)
        .map_err(|e| AppError::new(&e.kind, e.message.replace("传输项目", "删除项目")))
}
fn safe_parents(fs: &impl DeleteFs, path: &str) -> AppResult<()> {
    let mut parent = String::new();
    for part in remote_path::parent(path)
        .split('/')
        .filter(|p| !p.is_empty())
    {
        parent.push('/');
        parent.push_str(part);
        match fs.stat(&parent)? {
            Some(s) if s.kind == "directory" => {}
            Some(s) if s.kind == "symlink" => {
                return Err(AppError::new(
                    "path",
                    format!("祖先目录是符号链接，请进入真实目录后删除：{parent}"),
                ))
            }
            _ => {
                return Err(AppError::new(
                    "path",
                    format!("祖先目录不存在或类型已变化：{parent}"),
                ))
            }
        }
    }
    Ok(())
}
fn protect_mounts(path: &str, mounts: &[String]) -> AppResult<()> {
    if let Some(mount) = mounts
        .iter()
        .find(|m| *m == path || m.starts_with(&format!("{path}/")))
    {
        return Err(AppError::new(
            "mountBoundary",
            format!("目录包含挂载点，不能递归删除：{mount}"),
        ));
    }
    Ok(())
}
fn mount_snapshot(
    session: &ssh2::Session,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, Vec<String>)> {
    crate::remote_mounts::deletion_boundaries(session, check).map_err(|e| {
        AppError::new(
            &e.kind,
            format!("无法核验挂载边界，未继续递归删除：{}", e.message),
        )
    })
}

#[tauri::command]
pub async fn prepare_remote_delete(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    paths: Vec<String>,
) -> AppResult<DeletePreview> {
    crate::moves::workspace_only(&window)?;
    let context = state.spec(&session_id, window.label())?;
    let owner = state.owner(window.label());
    prepare(
        state.inner().clone(),
        context,
        owner,
        session_id,
        paths,
        None,
        true,
    )
    .await
}
async fn prepare(
    state: Arc<State>,
    context: SharedContext,
    owner: connection::WindowOwner,
    session_id: String,
    paths: Vec<String>,
    parent_task_id: Option<String>,
    require_session: bool,
) -> AppResult<DeletePreview> {
    let paths = normalize_paths(&paths)?;
    let operation = transfers::register_file_operation(state.clone(), context.clone())?;
    blocking(move || {
        let connection = connection::connect_verified(
            &context,
            &operation.cancel,
            crate::traffic::TrafficPurpose::Files,
        )?;
        let session = connection.session();
        let sftp = session.sftp()?;
        let fs = SftpFs(&sftp);
        let mut entries = vec![];
        for path in paths {
            operation.cancel.check()?;
            safe_parents(&fs, &path)?;
            entries.push((path.clone(), fs.stat(&path)?));
        }
        if entries
            .iter()
            .any(|(_, s)| s.as_ref().is_some_and(|s| s.kind == "directory"))
        {
            let (_, mounts) = mount_snapshot(session, &|| operation.cancel.check())?;
            for (path, stamp) in &entries {
                if stamp.as_ref().is_some_and(|s| s.kind == "directory") {
                    protect_mounts(path, &mounts)?;
                }
            }
        }
        if require_session {
            state.spec(&session_id, &owner.label)?;
        }
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new(
                "permission",
                "窗口已变化，请重新选择删除项目",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let preview = DeletePreview {
            id: id.clone(),
            server_name: context.target().name.clone(),
            parent_task_id: parent_task_id.clone(),
            entries: entries.iter().map(|(p, s)| entry(p, s.as_ref())).collect(),
        };
        let mut registry = state.deletes.0.lock().unwrap();
        registry
            .drafts
            .retain(|_, d| d.started.is_some() || d.expires > Instant::now());
        registry.drafts.insert(
            id,
            Draft {
                owner: owner.label,
                generation: owner.generation,
                session_id,
                context,
                entries,
                expires: Instant::now() + DRAFT_TTL,
                started: None,
                parent_task_id,
                require_session,
            },
        );
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub fn start_remote_delete(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    draft_id: String,
) -> AppResult<DeleteTask> {
    crate::moves::workspace_only(&window)?;
    let _action = state.file_tasks.actions.lock().unwrap();
    let mut registry = state.deletes.0.lock().unwrap();
    let draft = registry
        .drafts
        .get(&draft_id)
        .ok_or_else(|| AppError::new("expired", "删除准备已失效，请重新选择"))?;
    if draft.owner != window.label() || draft.generation != state.owner(window.label()).generation {
        return Err(AppError::new("permission", "此窗口无权启动该删除"));
    }
    if let Some(id) = &draft.started {
        return Ok(registry.tasks[id].view.lock().unwrap().clone());
    }
    if draft.expires < Instant::now() {
        return Err(AppError::new("expired", "删除确认已过期，请重新选择"));
    }
    if draft.require_session {
        state.spec(&draft.session_id, window.label())?;
    }
    let operation =
        transfers::register_file_operation(state.inner().clone(), draft.context.clone())?;
    let view = DeleteTask {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: draft.session_id.clone(),
        server_name: draft.context.target().name.clone(),
        status: "checking".into(),
        revision: 0,
        server_identity: draft.context.identity().into(),
        server_id: draft.context.target().id.clone(),
        created_at: crate::file_tasks::now(),
        last_started_at: crate::file_tasks::now(),
        finished_at: None,
        sort_order: state.transfers.next_order(),
        entry_count: draft.entries.len(),
        parent_task_id: draft.parent_task_id.clone(),
        last_checked_at: None,
        current_path: None,
        deleted_count: 0,
        missing_count: 0,
        entries: draft
            .entries
            .iter()
            .map(|(p, s)| entry(p, s.as_ref()))
            .collect(),
        affected_directories: vec![],
        error: None,
    };
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .save("delete", &view.id, &view)?;
    let handle = Arc::new(Handle {
        context: draft.context.clone(),
        view: Mutex::new(view.clone()),
        stop: AtomicBool::new(false),
        reconciling: AtomicBool::new(false),
        cancel: operation.cancel.clone(),
        last_emit: Mutex::new(Instant::now()),
    });
    let entries = draft.entries.clone();
    registry.drafts.get_mut(&draft_id).unwrap().started = Some(view.id.clone());
    registry.tasks.insert(view.id.clone(), handle.clone());
    drop(registry);
    state.file_tasks.emit(&view.id);
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let _operation = operation;
        if let Err(error) = run(&state, Some(&app), &handle, &entries) {
            handle.update(|view| {
                view.error = Some(error.message);
                view.status = if view.entries.iter().any(|e| {
                    e.failures
                        .iter()
                        .any(|f| f.uncertain && f.current_state.is_none())
                }) {
                    "unknown"
                } else if error.kind == "cancelled" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                for item in &mut view.entries {
                    if item.status == "pending" {
                        item.status = "notExecuted".into();
                    }
                }
            });
        }
        handle.update(|v| {
            v.current_path = None;
            v.finished_at = Some(crate::file_tasks::now());
        });
        let view = handle.view.lock().unwrap().clone();
        if let Err(e) = state
            .file_tasks
            .store
            .lock()
            .unwrap()
            .checkpoint_delete(&view.id, &view, false)
        {
            handle.update(|v| {
                v.error = Some(format!("任务结果保存失败：{}", e.message));
                if v.status != "unknown" {
                    v.status = "failed".into();
                }
            });
        }
        for path in &view.affected_directories {
            transfers::directory_changed(&app, handle.context.identity(), path);
        }
        handle.emit(Some(&app), true);
        state.file_tasks.emit(&view.id);
    });
    Ok(view)
}
#[tauri::command]
pub fn list_remote_deletes(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Vec<DeleteTask>> {
    crate::moves::workspace_only(&window)?;
    Ok(state.deletes.snapshot())
}
fn find(state: &State, id: &str) -> AppResult<Arc<Handle>> {
    let view = {
        let registry = state.deletes.0.lock().unwrap();
        if let Some(handle) = registry.tasks.get(id) {
            return Ok(handle.clone());
        }
        registry
            .recovered
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new("notFound", "删除任务不存在"))?
    };
    let context = state.connected_context(&view.server_identity)?;
    let mut registry = state.deletes.0.lock().unwrap();
    if let Some(handle) = registry.tasks.get(id) {
        return Ok(handle.clone());
    }
    let handle = Arc::new(Handle {
        context,
        view: Mutex::new(view),
        stop: AtomicBool::new(false),
        reconciling: AtomicBool::new(false),
        cancel: Cancellation::default(),
        last_emit: Mutex::new(Instant::now()),
    });
    registry.recovered.remove(id);
    registry.tasks.insert(id.into(), handle.clone());
    Ok(handle)
}
#[tauri::command]
pub async fn prepare_delete_retry(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    task_id: String,
) -> AppResult<DeletePreview> {
    crate::moves::workspace_only(&window)?;
    let handle = find(&state, &task_id)?;
    {
        let _action = state.file_tasks.actions.lock().unwrap();
        if handle.reconciling.swap(true, Ordering::SeqCst) {
            return Err(AppError::new("busy", "任务正在处理"));
        }
    }
    let view = handle.view.lock().unwrap().clone();
    let result = async {
        if active(&view.status) {
            return Err(AppError::new("busy", "请等待任务结束"));
        }
        if view.entries.iter().any(|e| {
            e.failures
                .iter()
                .any(|f| f.uncertain && f.current_state.is_none())
        }) {
            return Err(AppError::new("unknown", "请先核对删除结果"));
        }
        let paths: Vec<_> = view
            .entries
            .iter()
            .filter(|e| !matches!(e.status.as_str(), "deleted" | "missing"))
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            return Err(AppError::new("validation", "没有需要重新删除的项目"));
        }
        prepare(
            state.inner().clone(),
            handle.context.clone(),
            state.owner(window.label()),
            view.session_id,
            paths,
            Some(task_id),
            false,
        )
        .await
    }
    .await;
    handle.reconciling.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn cancel_remote_delete(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    task_id: String,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    let handle = find(&state, &task_id)?;
    handle.request_stop();
    handle.emit(Some(&app), true);
    // A stalled request is interrupted after a grace period. Its reply remains uncertain.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        if active(&handle.view.lock().unwrap().status) {
            handle.cancel.cancel();
        }
    });
    Ok(())
}

trait DeleteFs {
    fn stat(&self, path: &str) -> AppResult<Option<Stamp>>;
    fn children(&self, path: &str) -> AppResult<Vec<String>>;
    fn remove(&self, path: &str, directory: bool) -> AppResult<()>;
}
struct SftpFs<'a>(&'a Sftp);
fn sftp_error(error: ssh2::Error, mutation: bool) -> AppError {
    match error.code() {
        ssh2::ErrorCode::SFTP(2) => AppError::new("missing", "路径已不存在"),
        ssh2::ErrorCode::SFTP(3) => AppError::new("permission", "权限不足，无法删除此项目"),
        ssh2::ErrorCode::SFTP(code) if code != 6 && code != 7 => AppError::new("remote", error),
        _ => AppError::new(if mutation { "unknown" } else { "disconnected" }, error),
    }
}
impl DeleteFs for SftpFs<'_> {
    fn stat(&self, path: &str) -> AppResult<Option<Stamp>> {
        match self.0.lstat(Path::new(path)) {
            Ok(s) => Stamp::from_stat(s).map(Some),
            Err(e) if e.code() == ssh2::ErrorCode::SFTP(2) => Ok(None),
            Err(e) => Err(sftp_error(e, false)),
        }
    }
    fn children(&self, path: &str) -> AppResult<Vec<String>> {
        let mut dir = self
            .0
            .opendir(Path::new(path))
            .map_err(|e| sftp_error(e, false))?;
        let mut result = Vec::new();
        loop {
            let (name, _) = match dir.readdir() {
                Ok(entry) => entry,
                Err(e) if e.code() == ssh2::ErrorCode::Session(-16) => break,
                Err(e) => return Err(sftp_error(e, false)),
            };
            let name = name
                .to_str()
                .ok_or_else(|| AppError::new("path", "目录包含非 UTF-8 文件名，未删除此目录"))?;
            if name == "." || name == ".." {
                continue;
            }
            result.push(remote_path::join(path, name)?);
            if result.len() > MAX_ENTRIES {
                return Err(AppError::new("limit", "目录项目过多，请分批删除"));
            }
        }
        Ok(result)
    }
    fn remove(&self, path: &str, directory: bool) -> AppResult<()> {
        if directory {
            self.0.rmdir(Path::new(path))
        } else {
            self.0.unlink(Path::new(path))
        }
        .map_err(|e| sftp_error(e, true))
    }
}
#[derive(Clone, Serialize)]
struct Planned {
    path: String,
    stamp: Stamp,
}
fn scan(
    fs: &impl DeleteFs,
    path: &str,
    plan: &mut Vec<Planned>,
    depth: usize,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<()> {
    check()?;
    if depth > MAX_DEPTH || plan.len() >= MAX_ENTRIES {
        return Err(AppError::new("limit", "目录层级或项目过多，请分批删除"));
    }
    safe_parents(fs, path)?;
    let Some(stamp) = fs.stat(path)? else {
        return Ok(());
    };
    if stamp.kind == "directory" {
        for child in fs.children(path)? {
            scan(fs, &child, plan, depth + 1, check)?;
        }
    }
    if plan.len() >= MAX_ENTRIES {
        return Err(AppError::new("limit", "目录项目过多，请分批删除"));
    }
    plan.push(Planned {
        path: path.into(),
        stamp,
    });
    Ok(())
}
fn remove_planned(fs: &impl DeleteFs, planned: &Planned) -> AppResult<bool> {
    safe_parents(fs, &planned.path)?;
    let Some(current) = fs.stat(&planned.path)? else {
        return Ok(false);
    };
    if !planned.stamp.matches_after_children(&current) {
        return Err(AppError::new(
            "changed",
            "扫描后项目发生变化，已保留，请刷新后重新确认",
        ));
    }
    match fs.remove(&planned.path, current.kind == "directory") {
        Ok(()) => Ok(true),
        Err(e) if e.kind == "missing" => Ok(false),
        Err(e) => Err(e),
    }
}
fn remove_recorded(
    state: &State,
    id: &str,
    root: usize,
    fs: &impl DeleteFs,
    planned: &Planned,
) -> AppResult<bool> {
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .begin_delete(id, root, &planned.path)
        .map_err(|e| {
            AppError::new(
                "taskStore",
                format!("无法保存删除请求，已停止后续删除：{}", e.message),
            )
        })?;
    let result = remove_planned(fs, planned);
    let (outcome, message) = match &result {
        Ok(true) => ("deleted", None),
        Ok(false) => ("missing", None),
        Err(e) => (e.kind.as_str(), Some(e.message.clone())),
    };
    let step = crate::file_tasks::store::DeleteStep {
        root,
        path: planned.path.clone(),
        outcome: outcome.into(),
        message,
    };
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .finish_delete(id, &step)
        .map_err(|e| {
            AppError::new(
                "unknown",
                format!("删除回复无法保存，请核对结果：{}", e.message),
            )
        })?;
    result
}
fn final_status(entries: &[DeleteEntry]) -> &'static str {
    if entries.iter().any(|e| {
        e.failures
            .iter()
            .any(|f| f.uncertain && f.current_state.is_none())
    }) {
        "unknown"
    } else if entries
        .iter()
        .all(|e| matches!(e.status.as_str(), "deleted" | "missing"))
    {
        "completed"
    } else if entries.iter().any(|e| e.deleted_count > 0) {
        "partialFailed"
    } else {
        "failed"
    }
}
fn run(
    state: &Arc<State>,
    app: Option<&tauri::AppHandle>,
    handle: &Handle,
    entries: &[(String, Option<Stamp>)],
) -> AppResult<()> {
    let paths: Vec<_> = entries.iter().map(|(p, _)| p.clone()).collect();
    let id = handle.view.lock().unwrap().id.clone();
    let claims: Vec<_> = paths
        .iter()
        .map(|path| Claim {
            key: handle.context.target_public_key().into(),
            path: path.clone(),
            write: true,
        })
        .collect();
    let _locks = state
        .transfers
        .paths
        .acquire_many(&claims, &id, |conflict| {
            handle.check()?;
            if let Some((path, task)) = conflict {
                return Err(AppError::new(
                    "busy",
                    format!("任务 {task} 正在使用 {path}，请等待其结束后删除"),
                ));
            }
            Ok(())
        })?;
    if let Some((task, path)) = state
        .transfers
        .deletion_conflict(handle.context.target_public_key(), &paths)
    {
        return Err(AppError::new(
            "busy",
            format!("任务 {task} 正在使用 {path}，请等待其结束后删除"),
        ));
    }
    let _editors =
        state
            .files
            .begin_mutation(handle.context.target_public_key(), &paths, "删除")?;
    for path in &paths {
        if let Some(task) =
            state
                .transfers
                .pending_path(handle.context.target_public_key(), path, None)
        {
            return Err(AppError::new(
                "conflictPending",
                format!("路径 {path} 涉及待核对任务 {task}，请先核对结果"),
            ));
        }
    }
    let connection = connection::connect_verified(
        &handle.context,
        &handle.cancel,
        crate::traffic::TrafficPurpose::Files,
    )?;
    let session = connection.session();
    let sftp = session.sftp()?;
    let fs = SftpFs(&sftp);
    let mut affected = BTreeSet::new();
    let mut removed = Vec::new();
    let mut refreshed = Instant::now();
    let mut pending_directories = BTreeSet::new();
    let mut published_removed = 0;
    let mut stopped = false;
    for (index, (path, expected)) in entries.iter().enumerate() {
        if handle.check().is_err() {
            break;
        }
        handle.update(|v| {
            v.status = "scanning".into();
            v.current_path = Some(path.clone());
        });
        handle.emit(app, true);
        let mut result = entry(path, expected.as_ref());
        let preparation = (|| {
            safe_parents(&fs, path)?;
            let now = fs.stat(path)?;
            if now.is_none() {
                return Ok((vec![], None));
            }
            if &now != expected {
                return Err(AppError::new(
                    "changed",
                    "确认后项目发生变化，请刷新后重新确认删除",
                ));
            }
            let mounts = if now.as_ref().is_some_and(|s| s.kind == "directory") {
                let (version, boundaries) = mount_snapshot(session, &|| handle.check())?;
                protect_mounts(path, &boundaries)?;
                Some(version)
            } else {
                None
            };
            let mut plan = vec![];
            scan(&fs, path, &mut plan, 0, &|| handle.check())?;
            state
                .file_tasks
                .store
                .lock()
                .unwrap()
                .delete_plan(&id, index, &plan)
                .map_err(|e| AppError::new("taskStore", e.message))?;
            Ok((plan, mounts))
        })();
        match preparation {
            Err(e) => {
                stopped = matches!(e.kind.as_str(), "cancelled" | "disconnected" | "taskStore");
                result.status = if e.kind == "cancelled" {
                    "notExecuted"
                } else {
                    "failed"
                }
                .into();
                result.failures.push(DeleteFailure {
                    path: path.clone(),
                    message: e.message,
                    uncertain: false,
                    current_state: None,
                });
            }
            Ok((plan, mount_version)) => {
                if plan.is_empty() {
                    result.status = "missing".into();
                    result.missing_count = 1;
                    removed.push(path.clone());
                    affected.insert(remote_path::parent(path).to_owned());
                } else {
                    handle.update(|v| v.status = "running".into());
                    let mut mount_parent = String::new();
                    let mut root_removed = None;
                    for planned in plan {
                        if handle.check().is_err() {
                            stopped = true;
                            break;
                        }
                        handle.update(|v| v.current_path = Some(planned.path.clone()));
                        let parent = remote_path::parent(&planned.path);
                        // Recheck boundaries when entering another directory during deletion.
                        let outcome = (|| {
                            if let Some(version) = &mount_version {
                                if mount_parent != parent {
                                    let (current, _) = mount_snapshot(session, &|| handle.check())?;
                                    if &current != version {
                                        return Err(AppError::new(
                                            "mountChanged",
                                            "挂载信息已变化，已停止此目录的删除",
                                        ));
                                    }
                                    mount_parent = parent.into();
                                }
                            }
                            handle.check()?;
                            remove_recorded(state, &id, index, &fs, &planned)
                        })();
                        affected.insert(parent.to_owned());
                        pending_directories.insert(parent.to_owned());
                        match outcome {
                            Ok(deleted) => {
                                if deleted {
                                    result.deleted_count += 1;
                                } else {
                                    result.missing_count += 1;
                                }
                                if planned.path == *path {
                                    root_removed = Some(deleted);
                                }
                                removed.push(planned.path.clone());
                            }
                            Err(e) => {
                                let uncertain = e.kind == "unknown";
                                let halt = matches!(
                                    e.kind.as_str(),
                                    "unknown"
                                        | "disconnected"
                                        | "cancelled"
                                        | "mountChanged"
                                        | "taskStore"
                                );
                                result.failures.push(DeleteFailure {
                                    path: planned.path.clone(),
                                    message: e.message,
                                    uncertain,
                                    current_state: None,
                                });
                                if halt {
                                    stopped = true;
                                    break;
                                }
                            }
                        }
                        handle.update(|v| {
                            v.entries[index] = result.clone();
                            v.deleted_count = v.entries.iter().map(|e| e.deleted_count).sum();
                            v.missing_count = v.entries.iter().map(|e| e.missing_count).sum();
                        });
                        handle.emit(app, false);
                        if refreshed.elapsed() >= Duration::from_millis(400) {
                            if let Some(app) = app {
                                state.drags.clipboard.remove_deleted(
                                    app,
                                    handle.context.identity(),
                                    &removed[published_removed..],
                                );
                                for path in &pending_directories {
                                    transfers::directory_changed(
                                        app,
                                        handle.context.identity(),
                                        path,
                                    );
                                }
                            }
                            published_removed = removed.len();
                            pending_directories.clear();
                            refreshed = Instant::now();
                        }
                    }
                    result.status = if root_removed == Some(true) {
                        "deleted"
                    } else if root_removed == Some(false) {
                        "missing"
                    } else if result.deleted_count > 0 {
                        "partial"
                    } else if stopped && result.failures.is_empty() {
                        "notExecuted"
                    } else {
                        "failed"
                    }
                    .into();
                }
            }
        }
        handle.update(|v| {
            v.entries[index] = result;
            v.deleted_count = v.entries.iter().map(|e| e.deleted_count).sum();
            v.missing_count = v.entries.iter().map(|e| e.missing_count).sum();
        });
        handle.update(|v| v.affected_directories = affected.iter().cloned().collect());
        if let Some(app) = app {
            state
                .drags
                .clipboard
                .remove_deleted(app, handle.context.identity(), &removed);
            for path in &affected {
                transfers::directory_changed(app, handle.context.identity(), path);
            }
        }
        state.file_tasks.store.lock().unwrap().checkpoint_delete(
            &id,
            &*handle.view.lock().unwrap(),
            false,
        )?;
        state.file_tasks.emit(&id);
        if stopped {
            break;
        }
    }
    // Publish known mutations even when a later item fails or the user stops the batch.
    if let Some(app) = app {
        state
            .drags
            .clipboard
            .remove_deleted(app, handle.context.identity(), &removed);
        for path in &affected {
            transfers::directory_changed(app, handle.context.identity(), path);
        }
    } else {
        state
            .drags
            .clipboard
            .remove_deleted_paths(handle.context.identity(), &removed);
    }
    handle.update(|v| {
        for item in &mut v.entries {
            if item.status == "pending" {
                item.status = "notExecuted".into();
            }
        }
        v.affected_directories = affected.into_iter().collect();
        v.status = final_status(&v.entries).into();
        if v.status != "unknown"
            && v.status != "completed"
            && (handle.stop.load(Ordering::SeqCst) || handle.cancel.is_cancelled())
        {
            v.status = "cancelled".into();
        }
    });
    Ok(())
}

fn reconcile_view(
    sftp: &Sftp,
    view: &mut DeleteTask,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<String>> {
    let mut missing = vec![];
    for item in &mut view.entries {
        for failure in item.failures.iter_mut().filter(|f| f.uncertain) {
            check()?;
            safe_parents(&SftpFs(sftp), &failure.path)?;
            // Existence is an observation, never proof that a same-named file is the original.
            if remote_write::metadata(sftp, &failure.path)?.is_none() {
                failure.current_state = Some("missing".into());
                missing.push(failure.path.clone());
            } else {
                failure.current_state = Some("exists".into());
            }
        }
        if let Some(root) = item
            .failures
            .iter()
            .find(|f| f.uncertain && f.path == item.path)
        {
            if root.current_state.as_deref() == Some("missing") && item.status != "missing" {
                item.status = "missing".into();
                item.missing_count += 1;
            } else if root.current_state.as_deref() == Some("exists") && item.status == "missing" {
                item.status = if item.deleted_count > 0 {
                    "partial"
                } else {
                    "failed"
                }
                .into();
                item.missing_count = item.missing_count.saturating_sub(1);
            }
        }
    }
    view.missing_count = view.entries.iter().map(|item| item.missing_count).sum();
    view.status = final_status(&view.entries).into();
    view.revision += 1;
    view.last_checked_at = Some(crate::file_tasks::now());
    Ok(missing)
}

#[tauri::command]
pub async fn reconcile_remote_delete(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    task_id: String,
) -> AppResult<DeleteTask> {
    crate::moves::workspace_only(&window)?;
    let handle = find(&state, &task_id)?;
    if active(&handle.view.lock().unwrap().status) {
        return Err(AppError::new("busy", "请等待删除操作结束后核对"));
    }
    {
        let _action = state.file_tasks.actions.lock().unwrap();
        if handle.reconciling.swap(true, Ordering::SeqCst) {
            return Err(AppError::new("busy", "正在核对删除结果"));
        }
    }
    let state = state.inner().clone();
    let reconcile_handle = handle.clone();
    let result = blocking(move || {
        let operation =
            transfers::register_file_operation(state.clone(), reconcile_handle.context.clone())?;
        let connection = connection::connect_verified(
            &reconcile_handle.context,
            &operation.cancel,
            crate::traffic::TrafficPurpose::Files,
        )?;
        let sftp = connection.session().sftp()?;
        let mut view = reconcile_handle.view.lock().unwrap().clone();
        let missing = reconcile_view(&sftp, &mut view, &|| operation.cancel.check())?;
        state
            .file_tasks
            .store
            .lock()
            .unwrap()
            .checkpoint_delete(&view.id, &view, true)?;
        *reconcile_handle.view.lock().unwrap() = view.clone();
        state
            .drags
            .clipboard
            .remove_deleted(&app, reconcile_handle.context.identity(), &missing);
        for path in &view.affected_directories {
            transfers::directory_changed(&app, reconcile_handle.context.identity(), path);
        }
        reconcile_handle.emit(Some(&app), true);
        Ok(view)
    })
    .await;
    handle.reconciling.store(false, Ordering::SeqCst);
    result
}

#[cfg(test)]
pub(crate) mod tests;
