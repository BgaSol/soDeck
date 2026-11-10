//! Bounded single-file primitives. Recursion, conflicts and batch policy live in the plugin.
use super::*;
use std::{collections::HashSet, time::Instant};
use tokio::io::AsyncReadExt;

mod copy;
mod cursors;

const MAX_OPERATIONS: usize = 16;
const PAGE_BYTES: usize = 16000;
#[derive(Default)]
pub(super) struct Operations {
    session: tokio::sync::Mutex<State>,
    registry: Mutex<Registry>,
    inputs: Mutex<HashMap<String, Input>>,
    batches: Mutex<HashMap<String, Context>>,
    cancelled_batches: Mutex<HashSet<String>>,
    cursors: Mutex<HashMap<String, Arc<tokio::sync::Mutex<cursors::Cursor>>>>,
}
impl Operations {
    pub(super) fn new(transports: Arc<crate::resource_tasks::Tasks>) -> Self {
        Self {
            session: tokio::sync::Mutex::new(State {
                transports,
                ..State::default()
            }),
            ..Self::default()
        }
    }
    pub(super) async fn clear(&self) {
        *self.session.lock().await = State::default();
        self.registry.lock().unwrap().entries.clear();
        self.inputs.lock().unwrap().clear();
        self.batches.lock().unwrap().clear();
        self.cancelled_batches.lock().unwrap().clear();
        self.cursors.lock().unwrap().clear();
    }
    pub(super) fn cancel_pending(&self) -> bool {
        let mut cancelled = self.cancelled_batches.lock().unwrap();
        cancelled.extend(self.batches.lock().unwrap().keys().cloned());
        let registry = self.registry.lock().unwrap();
        let mut running = false;
        for operation in registry.entries.values() {
            operation.cancel.cancel();
            if ["running", "committing"].contains(
                &operation.state.lock().unwrap()["state"]
                    .as_str()
                    .unwrap_or(""),
            ) {
                running = true;
            }
        }
        running
    }
}
#[derive(Default)]
struct Registry {
    entries: HashMap<String, Arc<Operation>>,
    last: u64,
}
struct Operation {
    input_task: Mutex<Option<crate::resource_tasks::Task>>,
    _slot: tokio::sync::OwnedSemaphorePermit,
    id: String,
    request: Value,
    state: Mutex<Value>,
    cancel: CancellationToken,
    context: Context,
}
struct Input {
    roots: Vec<PathBuf>,
    context: Context,
    created: Instant,
    claimed: bool,
    allow_sibling: bool,
}
#[derive(Clone, PartialEq)]
enum Endpoint {
    Local(PathBuf),
    Remote(String),
}
struct Io {
    core: Arc<AppCore>,
    owner: Owner,
    resource: Arc<Resource>,
    session: Arc<Sftp>,
    operation: Arc<Operation>,
}
#[derive(Debug, Clone)]
pub struct OperationError {
    pub code: String,
    pub message: String,
}
impl OperationError {
    pub fn value(&self) -> Value {
        json!({"code":self.code,"message":self.message,"effect":if self.code=="unknown"{"unknown"}else{"none"},"retryable":!["unknown","invalid","unsupported","expired","cancelled","contextChanged"].contains(&self.code.as_str())})
    }
}
impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl From<String> for OperationError {
    fn from(message: String) -> Self {
        failure("io", message)
    }
}
impl From<&str> for OperationError {
    fn from(message: &str) -> Self {
        failure("invalid", message)
    }
}
impl From<OperationError> for String {
    fn from(error: OperationError) -> Self {
        error.to_string()
    }
}
fn err(error: impl std::fmt::Display) -> OperationError {
    failure("io", error)
}
fn failure(code: &str, message: impl std::fmt::Display) -> OperationError {
    OperationError {
        code: code.into(),
        message: message.to_string(),
    }
}
fn mutation_error(error: Error) -> OperationError {
    match error {
        Error::Status(status) => failure(
            match status.status_code {
                StatusCode::PermissionDenied => "permissionDenied",
                StatusCode::NoConnection | StatusCode::ConnectionLost => "unknown",
                StatusCode::NoSuchFile => "missing",
                StatusCode::OpUnsupported => "unsupported",
                _ if status
                    .error_message
                    .to_ascii_lowercase()
                    .contains("cross-device") =>
                {
                    "crossDevice"
                }
                _ => "io",
            },
            status.error_message,
        ),
        error => failure(
            "unknown",
            format!("服务器未确认操作结果，请刷新核对：{error}"),
        ),
    }
}
fn local_error(error: std::io::Error) -> OperationError {
    failure(
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => "permissionDenied",
            std::io::ErrorKind::StorageFull => "noSpace",
            std::io::ErrorKind::AlreadyExists => "targetExists",
            std::io::ErrorKind::NotFound => "missing",
            _ => "io",
        },
        error,
    )
}
fn error_value(error: OperationError) -> Value {
    error.value()
}
fn local_attrs(m: &std::fs::Metadata) -> Value {
    let kind = if m.file_type().is_symlink() {
        "link"
    } else if m.is_dir() {
        "directory"
    } else if m.is_file() {
        "file"
    } else {
        "other"
    };
    let modified = m
        .modified()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok());
    json!({"kind":kind,"size":m.len().to_string(),"mtime":modified.map(|v|v.as_secs()),"stamp":format!("{kind}:{}:{:?}:{}",m.len(),modified,m.permissions().readonly())})
}
fn remote_attrs(a: &FileAttributes) -> Value {
    let mut value = attrs(a);
    value["stamp"] = json!(format!(
        "{}:{}:{}:{}",
        value["kind"],
        value["size"],
        value["mtime"],
        a.permissions.unwrap_or(0)
    ));
    value
}
fn valid_name(name: &str) -> Result<(), OperationError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['\0', '/']) {
        return Err(failure("invalid", "文件名无效"));
    }
    #[cfg(windows)]
    {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        if name.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
            || name.chars().any(|c| c < ' ')
            || name.ends_with(['.', ' '])
            || [
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&stem.as_str())
        {
            return Err(failure(
                "invalidName",
                "目标系统不支持此文件名，请改名或跳过",
            ));
        }
    }
    Ok(())
}
impl AppCore {
    pub fn files_local_directory(
        &self,
        window: &str,
        token: &str,
        file: &str,
        input: &str,
    ) -> Result<PathBuf, OperationError> {
        let view = self.ui_data_writable(window, token)?;
        let resource = self.file_resource(&view.plugin_id, &view.plugin_instance_id, file)?;
        if resource.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        let inputs = resource.operations.inputs.lock().unwrap();
        let input = inputs
            .get(input)
            .filter(|i| i.allow_sibling && i.claimed)
            .ok_or("本地下载目标已清理")?;
        let path = input.roots.first().ok_or("没有本地目标")?;
        Ok(if path.is_dir() {
            path.clone()
        } else {
            path.parent().ok_or("目标没有父目录")?.to_path_buf()
        })
    }
    pub fn files_register_input(
        &self,
        window: &str,
        token: &str,
        file: &str,
        mut roots: Vec<PathBuf>,
        allow_sibling: bool,
    ) -> Result<Value, OperationError> {
        let view = self.ui_data_writable(window, token)?;
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, file)?;
        let _task = r.tasks.enter()?;
        if r.owner.view != view.id || roots.is_empty() || roots.len() > 4096 {
            return Err(failure("invalid", "本地选择无效或超过 4096 项"));
        }
        // Preserve native path spelling, but collapse duplicate and selected descendants.
        let selected: HashSet<PathBuf> = roots.iter().cloned().collect();
        let mut seen = HashSet::new();
        roots.retain(|path| {
            seen.insert(path.clone())
                && !path
                    .ancestors()
                    .skip(1)
                    .any(|parent| selected.contains(parent))
        });
        let mut inputs = r.operations.inputs.lock().unwrap();
        inputs.retain(|_, v| v.claimed || v.created.elapsed() < Duration::from_secs(60));
        if inputs.len() >= 64 {
            return Err(failure("busy", "本地文件选择过多，请先处理现有任务"));
        }
        let id = self.next_id();
        inputs.insert(
            id.clone(),
            Input {
                roots,
                context: view.context,
                created: Instant::now(),
                claimed: false,
                allow_sibling,
            },
        );
        Ok(json!({"inputId":id}))
    }
    pub fn files_begin_input(
        &self,
        window: &str,
        token: &str,
        file: &str,
        download: bool,
    ) -> Result<Value, OperationError> {
        let view = self.ui_data_writable(window, token)?;
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, file)?;
        let _task = r.tasks.enter()?;
        if r.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        let id = self.next_id();
        let mut registry = r.operations.registry.lock().unwrap();
        if registry.entries.len() >= MAX_OPERATIONS {
            return Err(failure("busy", "文件操作已满"));
        }
        registry.entries.insert(
            id.clone(),
            Arc::new(Operation {
                input_task: Mutex::new(Some(r.tasks.enter()?)),
                _slot: self
                    .files
                    .operation_slots
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| failure("busy", "核心文件操作已满"))?,
                id: id.clone(),
                request: json!({"download":download}),
                state: Mutex::new(json!({"id":id,"state":"waitingInput"})),
                cancel: r.cancel.child_token(),
                context: view.context,
            }),
        );
        Ok(json!({"id":id,"state":"waitingInput"}))
    }
    pub fn files_finish_input(
        &self,
        window: &str,
        token: &str,
        file: &str,
        operation: &str,
        paths: Result<Vec<PathBuf>, OperationError>,
    ) {
        let Some(r) = self.files.entries.lock().unwrap().get(file).cloned() else {
            return;
        };
        let op = r
            .operations
            .registry
            .lock()
            .unwrap()
            .entries
            .values()
            .find(|o| o.id == operation)
            .cloned();
        let Some(op) = op else {
            return;
        };
        let input_task = op.input_task.lock().unwrap().take();
        let _task = match r.tasks.enter() {
            Ok(task) => task,
            Err(_) => {
                drop(input_task);
                return;
            }
        };
        let result = paths.and_then(|paths| {
            let view = self.ui_data_writable(window, token)?;
            if view.context != op.context || op.cancel.is_cancelled() {
                return Err(failure("contextChanged", "选择期间目标已切换，请重新选择"));
            }
            if paths.is_empty() {
                Ok(json!({"cancelled":true}))
            } else {
                self.files_register_input(
                    window,
                    token,
                    file,
                    paths,
                    op.request["download"] == true,
                )
            }
        });
        *op.state.lock().unwrap() = match result {
            Ok(value) => json!({"id":operation,"state":"completed","result":value}),
            Err(error) => json!({"id":operation,"state":"failed","error":error_value(error)}),
        };
    }
    pub(super) fn file_operation_control(
        self: &Arc<Self>,
        r: Arc<Resource>,
        p: &Value,
    ) -> Result<Value, OperationError> {
        let action = string(p, "action")?;
        if action == "op.start" {
            let registry = r.operations.registry.lock().unwrap();
            if let Some(op) = registry
                .entries
                .values()
                .find(|op| op.request["requestId"] == p["requestId"] && p["requestId"].is_string())
            {
                if op.request["operation"] != p["operation"]
                    || op.request["sequence"] != p["sequence"]
                    || op.request["batchId"] != p["batchId"]
                    || op.request["expectedContext"] != p["expectedContext"]
                {
                    return Err(failure("invalid", "重复操作参数不一致"));
                }
                return Ok(op.state.lock().unwrap().clone());
            }
        }
        let _task = if matches!(action, "op.start" | "batch.begin") {
            Some(r.tasks.enter()?)
        } else {
            None
        };
        if action == "batch.begin" {
            let owner = self.resource_owner(&r.owner.plugin, &r.owner.instance, &r.owner.view)?;
            if serde_json::to_value(&owner.context).map_err(err)? != p["context"] {
                return Err(failure("contextChanged", "连接已更换，请重新提交"));
            }
            let cancelled = r.operations.cancelled_batches.lock().unwrap();
            if cancelled.contains(string(p, "batchId")?) {
                return Err(failure("cancelled", "批次已取消"));
            }
            let mut batches = r.operations.batches.lock().unwrap();
            if batches.len() >= 64 {
                return Err(failure("busy", "文件任务已满"));
            }
            batches.insert(string(p, "batchId")?.into(), owner.context);
            return Ok(json!({}));
        }
        if action == "batch.cancel" {
            let id = string(p, "batchId")?;
            let mut cancelled = r.operations.cancelled_batches.lock().unwrap();
            if r.operations.batches.lock().unwrap().contains_key(id) {
                cancelled.insert(id.into());
            }
            for op in r
                .operations
                .registry
                .lock()
                .unwrap()
                .entries
                .values()
                .filter(|op| op.request["batchId"] == id)
            {
                op.cancel.cancel();
            }
            return Ok(json!({}));
        }
        if action == "batch.end" {
            let id = string(p, "batchId")?;
            r.operations
                .cursors
                .lock()
                .unwrap()
                .retain(|_, cursor| cursor.try_lock().map_or(true, |cursor| cursor.batch != id));
            let mut cancelled = r.operations.cancelled_batches.lock().unwrap();
            r.operations.batches.lock().unwrap().remove(id);
            cancelled.remove(id);
            return Ok(json!({}));
        }
        if action.starts_with("input.") {
            let id = string(p, "inputId")?;
            let mut inputs = r.operations.inputs.lock().unwrap();
            if action == "input.release" {
                inputs.remove(id);
                return Ok(json!({}));
            }
            let input = inputs
                .get_mut(id)
                .ok_or_else(|| failure("expired", "本地文件选择已失效"))?;
            let current = self
                .ui_state()
                .resource_view(&r.owner.view)?
                .context
                .clone();
            if action == "input.claim"
                && p["renew"] == true
                && input.claimed
                && input.context.workspace_id == current.workspace_id
            {
                // Explicit failed-item retry can reuse the retained native paths after reconnect.
                // This does not migrate the old batch or its remote operation to the new session.
                input.context = current.clone();
            }
            if input.context != current
                || (!input.claimed && input.created.elapsed() > Duration::from_secs(60))
            {
                return Err(failure("expired", "连接已更换或本地选择已过期，请重新选择"));
            }
            if action == "input.claim" {
                input.claimed = true;
                return Ok(json!({}));
            }
            if action != "input.list" {
                return Err(failure("invalid", "未知本地来源操作"));
            }
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            let mut entries = Vec::new();
            let mut bytes = 0;
            for (index, path) in input.roots.iter().enumerate().skip(offset).take(100) {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_str()
                    .ok_or_else(|| failure("invalidName", "文件名无法用 UTF-8 表示"))?;
                let value = json!({"rootId":index.to_string(),"name":name});
                bytes += serde_json::to_vec(&value).map_err(err)?.len();
                if bytes > PAGE_BYTES {
                    break;
                }
                entries.push(value);
            }
            let next = offset + entries.len();
            return Ok(
                json!({"entries":entries,"next":if next<input.roots.len(){Some(next)}else{None}}),
            );
        }
        if action != "op.start" {
            let cancelled = r.operations.cancelled_batches.lock().unwrap();
            let mut registry = r.operations.registry.lock().unwrap();
            let key = registry
                .entries
                .iter()
                .find(|(_, op)| {
                    p["operationId"].as_str() == Some(&op.id)
                        || (action == "op.lookup" && p["requestId"] == op.request["requestId"])
                })
                .map(|(key, _)| key.clone());
            let Some(key) = key else {
                if action == "op.lookup"
                    && p["protocolVersion"] == 2
                    && cancelled.contains(p["batchId"].as_str().unwrap_or(""))
                {
                    return Ok(
                        json!({"state":"cancelled","error":{"code":"cancelled","effect":"none","retryable":false,"message":"批次已封闭；此请求未被接纳"}}),
                    );
                }
                return Err(failure("expired", "文件操作结果已释放或不存在"));
            };
            let op = registry.entries[&key].clone();
            if action == "op.cancel" {
                op.cancel.cancel();
                return Ok(json!({}));
            }
            let state = op.state.lock().unwrap().clone();
            if action == "op.release" {
                if ["running", "committing", "waitingInput"]
                    .contains(&state["state"].as_str().unwrap_or(""))
                {
                    return Err(failure("busy", "文件操作仍在进行"));
                }
                registry.entries.remove(&key);
                return Ok(json!({}));
            }
            if !["op.poll", "op.result", "op.lookup"].contains(&action) {
                return Err(failure("invalid", "未知文件操作"));
            }
            return Ok(state);
        }
        let owner = match self.resource_owner(&r.owner.plugin, &r.owner.instance, &r.owner.view) {
            Ok(owner) => owner,
            Err(error) => {
                let context = r
                    .operations
                    .batches
                    .lock()
                    .unwrap()
                    .get(p["batchId"].as_str().unwrap_or(""))
                    .cloned();
                let ui = self.ui_state();
                let view = ui.resource_view(&r.owner.view)?;
                if context.as_ref() != Some(&view.context)
                    || !ui.lifecycle.view_frozen(&view.id)
                    || !ui.lifecycle.view_input_work_allowed(&view.id)
                    || view.plugin_instance_id != r.owner.instance
                    || r.cancel.is_cancelled()
                {
                    return Err(error.into());
                }
                Owner {
                    context: view.context.clone(),
                    ..r.owner.clone()
                }
            }
        };
        if !p["expectedContext"].is_null()
            && serde_json::to_value(&owner.context).map_err(err)? != p["expectedContext"]
        {
            return Err(failure("contextChanged", "连接已更换，请重新操作"));
        }
        self.connection_session(&owner.context)?;
        let admission = self.ui_state();
        let current = admission.resource_view(&r.owner.view)?;
        if current.context != owner.context
            || current.plugin_instance_id != r.owner.instance
            || (admission.lifecycle.view_frozen(&current.id)
                && !admission.lifecycle.view_input_work_allowed(&current.id))
        {
            return Err(failure("contextChanged", "文件页面已关闭或更换上下文"));
        }
        let cancellation_guard = r.operations.cancelled_batches.lock().unwrap();
        if p["protocolVersion"] == 2
            && r.operations
                .batches
                .lock()
                .unwrap()
                .get(p["batchId"].as_str().unwrap_or(""))
                != Some(&owner.context)
        {
            return Err(failure("expired", "文件批次已结束或上下文改变"));
        }
        if cancellation_guard.contains(p["batchId"].as_str().unwrap_or("")) {
            return Err(failure("cancelled", "文件任务已取消"));
        }
        let request_id = string(p, "requestId")?;
        if request_id.len() > 160 || serde_json::to_vec(p).map_err(err)?.len() > 24000 {
            return Err(failure("invalid", "文件操作参数过大"));
        }
        let mut registry = r.operations.registry.lock().unwrap();
        if let Some(op) = registry
            .entries
            .values()
            .find(|op| op.request["requestId"] == request_id)
        {
            if op.request["operation"] != p["operation"]
                || op.context != owner.context
                || op.request["sequence"] != p["sequence"]
                || op.request["batchId"] != p["batchId"]
            {
                return Err(failure("invalid", "重复操作参数不一致"));
            }
            return Ok(op.state.lock().unwrap().clone());
        }
        let sequence = p["sequence"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(registry.last.saturating_add(1));
        if sequence <= registry.last {
            return Err(failure("expired", "操作序号已退役，请核对已有结果"));
        }
        if registry.entries.len() >= MAX_OPERATIONS {
            return Err(failure("busy", "文件操作已满"));
        }
        let task = r.tasks.enter()?;
        let slot = self
            .files
            .operation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| failure("busy", "核心文件操作已满"))?;
        registry.last = sequence;
        let id = self.next_id();
        let operation = Arc::new(Operation {
            input_task: Mutex::new(None),
            _slot: slot,
            id: id.clone(),
            request: p.clone(),
            state: Mutex::new(json!({"id":id,"state":"running","received":"0"})),
            cancel: r.cancel.child_token(),
            context: owner.context.clone(),
        });
        registry.entries.insert(id.clone(), operation.clone());
        drop(registry);
        drop(cancellation_guard);
        drop(admission);
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            let _task = task;
            let mut state = State {
                transports: r.transports.clone(),
                ..State::default()
            };
            let result = async {
                let session = if operation.request["operation"]["kind"] == "list" {
                    sftp(&core, &mut state, &owner.context).await?
                } else {
                    sftp(
                        &core,
                        &mut *r.operations.session.lock().await,
                        &owner.context,
                    )
                    .await?
                };
                let io = Io {
                    core: core.clone(),
                    owner,
                    resource: r.clone(),
                    session,
                    operation: operation.clone(),
                };
                let result = io.execute(&operation.request["operation"]).await;
                result
            };
            // Never drop an in-flight mutation: a lost reply may already have changed the filesystem.
            let readonly = ["inspect", "list", "next"].contains(
                &operation.request["operation"]["kind"]
                    .as_str()
                    .unwrap_or(""),
            );
            let result = if readonly {
                tokio::select! {biased;_=operation.cancel.cancelled()=>Err(failure("cancelled","文件操作已取消")),v=tokio::time::timeout(Duration::from_secs(40),result)=>v.map_err(|_|failure("timeout","文件操作超时")).and_then(|v|v)}
            } else {
                result.await
            };
            drop(state);
            *operation.state.lock().unwrap() = match result {
                Ok(value) => json!({"id":operation.id,"state":"completed","result":value}),
                Err(error) => {
                    let value = error_value(error);
                    json!({"id":operation.id,"state":if value["code"]=="unknown"{"unknown"}else if value["code"]=="cancelled"{"cancelled"}else{"failed"},"error":value})
                }
            };
        });
        Ok(json!({"id":id,"state":"running"}))
    }
}
impl Io {
    async fn parent_identity(&self, e: &Endpoint) -> Result<Endpoint, OperationError> {
        match e {
            Endpoint::Local(path) => {
                let parent = path
                    .parent()
                    .ok_or_else(|| failure("invalid", "目标没有父目录"))?;
                let attrs = tokio::fs::symlink_metadata(parent).await.map_err(err)?;
                if attrs.file_type().is_symlink() || !attrs.is_dir() {
                    return Err(failure("sourceChanged", "父目录不是普通目录"));
                }
                Ok(Endpoint::Local(
                    tokio::fs::canonicalize(parent).await.map_err(err)?,
                ))
            }
            Endpoint::Remote(path) => {
                let parent = path
                    .rsplit_once('/')
                    .map(|p| if p.0.is_empty() { "/" } else { p.0 })
                    .ok_or_else(|| failure("invalid", "操作需要完整远程路径"))?;
                if self
                    .inspect(&Endpoint::Remote(parent.into()))
                    .await?
                    .is_none_or(|a| a["kind"] != "directory")
                {
                    return Err(failure("sourceChanged", "父目录不是普通目录"));
                }
                Ok(Endpoint::Remote(
                    canonical(&self.session.raw, parent).await?,
                ))
            }
        }
    }
    fn endpoint(&self, p: &Value) -> Result<Endpoint, OperationError> {
        if p["side"] == "remote" {
            let path = string(p, "path")?;
            if path.is_empty() || path.len() > 4096 || path.contains('\0') {
                return Err(failure("invalid", "远程路径无效"));
            }
            return Ok(Endpoint::Remote(path.into()));
        }
        if p["side"] != "local" {
            return Err(failure("invalid", "未知文件端点"));
        }
        let inputs = self.resource.operations.inputs.lock().unwrap();
        let input = inputs
            .get(string(p, "inputId")?)
            .ok_or_else(|| failure("expired", "本地来源已失效"))?;
        if input.context != self.owner.context || !input.claimed {
            return Err(failure("expired", "本地来源未提交或连接已更换"));
        }
        let index = string(p, "rootId")?.parse::<usize>().map_err(err)?;
        let mut path = input
            .roots
            .get(index)
            .cloned()
            .ok_or_else(|| failure("invalid", "本地来源索引无效"))?;
        if let Some(name) = p["rootName"].as_str() {
            // A single-file save target may be renamed only within its originally selected parent.
            if !input.allow_sibling {
                return Err(failure("invalid", "此本地来源不支持更换顶层名称"));
            }
            valid_name(name)?;
            path.set_file_name(name);
        }
        for name in p["relative"]
            .as_array()
            .ok_or_else(|| failure("invalid", "缺少本地相对路径"))?
        {
            let name = name
                .as_str()
                .ok_or_else(|| failure("invalid", "本地文件名无效"))?;
            valid_name(name)?;
            let m = std::fs::symlink_metadata(&path).map_err(err)?;
            if m.file_type().is_symlink() || !m.is_dir() {
                return Err(failure("sourceChanged", "父目录已变化或为符号链接"));
            }
            path.push(name);
        }
        Ok(Endpoint::Local(path))
    }
    async fn inspect(&self, e: &Endpoint) -> Result<Option<Value>, OperationError> {
        match e {
            Endpoint::Local(path) => match tokio::fs::symlink_metadata(path).await {
                Ok(m) => Ok(Some(local_attrs(&m))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(err(e)),
            },
            Endpoint::Remote(path) => match self.session.raw.lstat(path).await {
                Ok(a) => {
                    let mut attrs = remote_attrs(&a.attrs);
                    if attrs["kind"] == "directory" {
                        attrs["resolved"] = json!(canonical(&self.session.raw, path).await?);
                    }
                    Ok(Some(attrs))
                }
                Err(Error::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(None),
                Err(e) => Err(err(e)),
            },
        }
    }
    async fn check_guard(
        &self,
        e: &Endpoint,
        guard: &Value,
    ) -> Result<Option<Value>, OperationError> {
        let current = self.inspect(e).await?;
        if guard["kind"] == "absent" && current.is_some() {
            return Err(failure("targetExists", "目标已存在"));
        }
        if guard["kind"] == "match"
            && current.as_ref().map(|v| &v["stamp"]) != Some(&guard["stamp"])
        {
            return Err(failure("targetChanged", "目标文件已被修改，请重新处理冲突"));
        }
        Ok(current)
    }
    async fn execute(&self, p: &Value) -> Result<Value, OperationError> {
        if self.operation.cancel.is_cancelled() {
            return Err(failure("cancelled", "文件操作已取消"));
        }
        self.core.connection_session(&self.owner.context)?;
        let kind = string(p, "kind")?;
        if kind == "renameNoReplace" {
            return Err(failure(
                "unsupported",
                "当前后端不能保证原子无覆盖移动，未修改源或目标",
            ));
        }
        if kind == "next" || kind == "closeCursor" {
            return self.cursor_action(p).await;
        }
        if kind == "copyFile" {
            return self.copy(p).await;
        }
        let endpoint = self.endpoint(&p["endpoint"])?;
        if kind == "inspect" {
            return Ok(json!({"attrs":self.inspect(&endpoint).await?}));
        }
        if kind == "list" {
            return self.open_cursor(endpoint).await;
        }
        let current = self.check_guard(&endpoint, &p["guard"]).await?;
        if let Some(expected) = p["sourceStamp"].as_str() {
            if current.as_ref().and_then(|v| v["stamp"].as_str()) != Some(expected) {
                return Err(failure("sourceChanged", "源条目已变化"));
            }
        }
        match (kind, &endpoint) {
            ("mkdir", Endpoint::Local(path)) => tokio::fs::create_dir(path).await.map_err(err)?,
            ("mkdir", Endpoint::Remote(path)) => {
                self.session
                    .raw
                    .mkdir(path, FileAttributes::default())
                    .await
                    .map_err(mutation_error)?;
            }
            ("createEmpty", Endpoint::Local(path)) => {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .await
                    .map_err(err)?;
            }
            ("createEmpty", Endpoint::Remote(path)) => {
                let h = self
                    .session
                    .raw
                    .open(
                        path,
                        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                        FileAttributes::default(),
                    )
                    .await
                    .map_err(mutation_error)?;
                if let Err(error) = self.session.raw.close(h.handle).await {
                    return Ok(
                        json!({"done":true,"cleanupWarning":format!("文件已创建，关闭句柄未确认：{error}")}),
                    );
                }
            }
            ("removeFile", Endpoint::Local(path)) => {
                tokio::fs::remove_file(path).await.map_err(err)?
            }
            ("removeDirectory", Endpoint::Local(path)) => {
                tokio::fs::remove_dir(path).await.map_err(err)?
            }
            ("removeFile", Endpoint::Remote(path)) => {
                self.session
                    .raw
                    .remove(path)
                    .await
                    .map_err(mutation_error)?;
            }
            ("removeDirectory", Endpoint::Remote(path)) => {
                self.session.raw.rmdir(path).await.map_err(mutation_error)?;
            }
            ("rename", Endpoint::Remote(path)) => {
                let Endpoint::Remote(target) = self.endpoint(&p["destination"])? else {
                    return Err(failure("invalid", "移动目标必须在同一服务器"));
                };
                self.check_guard(&Endpoint::Remote(target.clone()), &json!({"kind":"absent"}))
                    .await?;
                *self.operation.state.lock().unwrap() =
                    json!({"id":self.operation.id,"state":"committing"});
                self.session
                    .raw
                    .rename(path, target)
                    .await
                    .map_err(mutation_error)?;
            }
            _ => return Err(failure("invalid", "未知文件操作")),
        }
        Ok(json!({"done":true}))
    }
}
