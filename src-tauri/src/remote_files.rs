//! Session-scoped SFTP primitives. Navigation, encoding and editor decisions live in plugins.
use crate::{resource_owner::Owner, workspaces::Context, AppCore};
use russh_sftp::{
    client::{error::Error, RawSftpSession},
    protocol::{FileAttributes, OpenFlags, Packet, StatusCode},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

mod data;
mod directory;
mod document;
mod editor;
mod export;
mod save;
pub use export::ExportTask;
#[cfg(test)]
use save::{commit_file, SaveFailure};
mod location;
mod operations;
#[cfg(test)]
mod save_tests;
pub use operations::OperationError;

pub const LIMIT: usize = 1024 * 1024;
const BLOCK: usize = 16 * 1024;
pub struct Files {
    entries: Mutex<HashMap<String, Arc<Resource>>>,
    operation_slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Files {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            operation_slots: Arc::new(tokio::sync::Semaphore::new(128)),
        }
    }
}
struct Resource {
    owner: Owner,
    cancel: CancellationToken,
    tasks: Arc<crate::resource_tasks::Tasks>,
    transports: Arc<crate::resource_tasks::Tasks>,
    done: CancellationToken,
    cleanup_started: std::sync::atomic::AtomicBool,
    state: tokio::sync::Mutex<State>,
    editor: Mutex<editor::Editor>,
    job: Mutex<Value>,
    close_saves: Mutex<HashMap<String, Value>>,
    job_cancel: Mutex<CancellationToken>,
    operations: operations::Operations,
}
#[derive(Default)]
struct State {
    transports: Arc<crate::resource_tasks::Tasks>,
    session: Option<Arc<Sftp>>,
    directories: HashMap<String, Directory>,
    directory_order: VecDeque<String>,
}
struct Sftp {
    raw: RawSftpSession,
    context: Context,
    atomic: bool,
}
impl Drop for Sftp {
    fn drop(&mut self) {
        let _ = self.raw.close_session();
    }
}
struct Directory {
    path: String,
    handle: String,
    pending: VecDeque<Value>,
    eof: bool,
}
#[derive(Clone)]
struct Document {
    id: String,
    path: String,
    resolved: String,
    version: String,
    attrs: FileAttributes,
    bytes: Arc<[u8]>,
    revision: u64,
}
impl Files {
    pub(crate) fn saving(&self, view: &str) -> bool {
        self.entries.lock().unwrap().values().any(|r| {
            if r.owner.view != view {
                return false;
            }
            let saving = editor::pending(&r.job.lock().unwrap());
            saving || editor::pending(&r.editor.lock().unwrap().export)
        })
    }
    pub(crate) fn cancel_pending(&self, view: &str) -> bool {
        let mut pending = false;
        for resource in self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.owner.view == view)
        {
            resource.job_cancel.lock().unwrap().cancel();
            let e = resource.editor.lock().unwrap();
            e.export_cancel.cancel();
            pending |= editor::pending(&e.export);
            drop(e);
            pending |= resource.operations.cancel_pending()
                || ["running", "committing"]
                    .contains(&resource.job.lock().unwrap()["state"].as_str().unwrap_or(""));
        }
        pending
    }
    pub(crate) fn validation_handle(&self, view: &str) -> Option<String> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .find(|(_, r)| r.owner.view == view)
            .map(|(id, _)| id.clone())
    }
    pub(crate) fn cancel(&self, predicate: impl Fn(&Owner) -> bool) {
        self.entries.lock().unwrap().retain(|_, r| {
            if predicate(&r.owner) {
                r.close();
                !r.done.is_cancelled()
            } else {
                true
            }
        });
    }
    pub(crate) fn pending(&self, predicate: impl Fn(&Owner) -> bool) -> usize {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, r| !r.done.is_cancelled());
        entries.values().filter(|r| predicate(&r.owner)).count()
    }
}
impl Resource {
    fn close(self: &Arc<Self>) {
        // The token is set under the task admission lock before any late worker can enter.
        self.tasks.close();
        if self
            .cleanup_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        self.cancel.cancel();
        self.job_cancel.lock().unwrap().cancel();
        self.operations.cancel_pending();
        let resource = self.clone();
        tauri::async_runtime::spawn(async move {
            while !resource.tasks.finished() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            *resource.state.lock().await = State::default();
            *resource.editor.lock().unwrap() = editor::Editor::default();
            resource.operations.clear().await;
            resource.transports.close();
            while !resource.transports.finished() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            resource.done.cancel();
        });
    }
}
fn string<'a>(p: &'a Value, name: &str) -> Result<&'a str, String> {
    p[name].as_str().ok_or_else(|| format!("缺少 {name}"))
}
fn path(p: &Value) -> Result<&str, String> {
    let s = string(p, "path")?;
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err("远端路径无效".into());
    }
    Ok(s)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn eof(e: &Error) -> bool {
    matches!(e,Error::Status(s) if s.status_code==StatusCode::Eof)
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn attrs(a: &FileAttributes) -> Value {
    json!({"size":a.size.map(|s|s.to_string()),"mtime":a.mtime,"permissions":a.permissions,"kind":if a.is_dir(){"directory"}else if a.is_symlink(){"link"}else if a.is_regular(){"file"}else{"other"}})
}

impl AppCore {
    pub(crate) fn files_operations(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, OperationError> {
        let r = self.file_resource(plugin, instance, string(p, "id")?)?;
        let action = string(p, "action")?;
        if !["start", "lookup", "poll", "cancel", "release"].contains(&action) {
            return Err(OperationError {
                code: "invalid".into(),
                message: "未知文件原语操作".into(),
            });
        }
        if action == "start"
            && (p["requestId"].as_str().is_none_or(str::is_empty)
                || p["sequence"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .is_none()
                || p["expectedContext"].is_null()
                || p["operation"]["kind"] == "rename")
        {
            return Err(OperationError {
                code: "invalid".into(),
                message: "需要固定请求身份、上下文及新版操作".into(),
            });
        }
        let mut request = p.clone();
        request["action"] = json!(format!("operation.{action}"));
        self.file_operation_control(r, &request)
    }

    pub fn validate_file_input(&self, window: &str, token: &str, p: &Value) -> Result<(), String> {
        let (view, _call) = self.surface_call_admission(window, token)?;
        let resource =
            self.file_resource(&view.plugin_id, &view.plugin_instance_id, string(p, "id")?)?;
        if resource.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        Ok(())
    }
    pub(crate) fn files_open(
        &self,
        plugin: &str,
        instance: &str,
        view: &str,
    ) -> Result<Value, String> {
        let owner = self.resource_owner(plugin, instance, view)?;
        let admission = self.resource_admission(&owner)?;
        let mut all = self.files.entries.lock().unwrap();
        if let Some((id, _)) = all
            .iter()
            .find(|(_, r)| r.owner.view == view && r.owner.instance == instance)
        {
            return Ok(json!({"id":id}));
        }
        if all.len() >= 64 {
            return Err("文件视图已达上限，请关闭不用的页面".into());
        }
        let id = self.next_id();
        let transports = Arc::new(crate::resource_tasks::Tasks::default());
        all.insert(
            id.clone(),
            Arc::new(Resource {
                owner,
                cancel: CancellationToken::new(),
                tasks: Arc::new(crate::resource_tasks::Tasks::default()),
                transports: transports.clone(),
                done: CancellationToken::new(),
                cleanup_started: std::sync::atomic::AtomicBool::new(false),
                state: tokio::sync::Mutex::new(State {
                    transports: transports.clone(),
                    ..State::default()
                }),
                editor: Mutex::new(editor::Editor::default()),
                job: Mutex::new(json!({"state":"idle"})),
                close_saves: Mutex::new(HashMap::new()),
                job_cancel: Mutex::new(CancellationToken::new()),
                operations: operations::Operations::new(transports.clone()),
            }),
        );
        drop(all);
        drop(admission);
        if let Err(error) = self.resource_owner(plugin, instance, view) {
            self.files
                .cancel(|owner| owner.view == view && owner.instance == instance);
            return Err(error);
        }
        Ok(json!({"id":id}))
    }
    fn file_resource(
        &self,
        plugin: &str,
        instance: &str,
        id: &str,
    ) -> Result<Arc<Resource>, String> {
        self.files
            .entries
            .lock()
            .unwrap()
            .get(id)
            .filter(|r| r.owner.plugin == plugin && r.owner.instance == instance)
            .cloned()
            .ok_or("文件资源已关闭或不属于此插件".into())
    }
    pub fn files_control(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        input: &Value,
    ) -> Result<Value, String> {
        let mut params = input.clone();
        params["action"] = json!(method.strip_prefix("files.").ok_or("未知文件方法")?);
        let p = &params;
        if p["action"] == "open" {
            return self.files_open(plugin, instance, string(p, "viewId")?);
        }
        let id = string(p, "id")?;
        if p["action"] == "close" && !self.files.entries.lock().unwrap().contains_key(id) {
            return Ok(json!({}));
        }
        let r = self.file_resource(plugin, instance, id)?;
        if p["action"].as_str().is_some_and(|a| {
            a.starts_with("operation.") || a.starts_with("input.") || a.starts_with("batch.")
        }) {
            return self.file_operation_control(r, p).map_err(Into::into);
        }
        match p["action"].as_str() {
            Some("close") => {
                r.close();
                Ok(json!({}))
            }
            _ => Err("未知文件控制方法".into()),
        }
    }
}
async fn sftp(core: &AppCore, s: &mut State, context: &Context) -> Result<Arc<Sftp>, String> {
    if s.session.as_ref().is_none_or(|f| f.context != *context) {
        s.session = None;
        s.directories.clear();
        s.directory_order.clear();
        let session = core.connection_session(context)?;
        let channel = session.open_channel().await?;
        channel.request_subsystem(true, "sftp").await.map_err(err)?;
        let raw = RawSftpSession::new_with_config(
            crate::resource_tasks::TrackedStream {
                stream: channel.into_stream(),
                _task: s.transports.enter()?,
            },
            russh_sftp::client::Config {
                max_packet_len: 256 * 1024,
                max_concurrent_reads: 1,
                max_concurrent_writes: 1,
                request_timeout_secs: 20,
                ..Default::default()
            },
        );
        let init = raw
            .init()
            .await
            .map_err(|e| format!("服务器未提供可用 SFTP：{e}"))?;
        let atomic = init
            .extensions
            .get("posix-rename@openssh.com")
            .is_some_and(|v| v == "1");
        s.session = Some(Arc::new(Sftp {
            raw,
            context: context.clone(),
            atomic,
        }));
    }
    Ok(s.session.as_ref().unwrap().clone())
}
async fn canonical(raw: &RawSftpSession, path: &str) -> Result<String, String> {
    raw.realpath(path)
        .await
        .map_err(err)?
        .files
        .into_iter()
        .next()
        .map(|f| f.filename)
        .ok_or("服务器没有返回规范路径".into())
}
fn version(a: &FileAttributes, bytes: &[u8]) -> String {
    format!(
        "{}:{}:{}:{}",
        a.size.unwrap_or(bytes.len() as u64),
        a.mtime.unwrap_or(0),
        a.permissions.unwrap_or(0),
        digest(bytes)
    )
}
async fn read_small(
    raw: &RawSftpSession,
    path: &str,
    a: &FileAttributes,
    received: impl Fn(usize),
) -> Result<Vec<u8>, String> {
    if a.size.is_some_and(|s| s > LIMIT as u64) {
        return Err("文件超过 1 MiB，请下载后查看".into());
    }
    let handle = raw
        .open(path, OpenFlags::READ, FileAttributes::default())
        .await
        .map_err(err)?
        .handle;
    let result = async {
        let mut bytes = Vec::new();
        loop {
            match raw
                .read(handle.clone(), bytes.len() as u64, BLOCK as u32)
                .await
            {
                Ok(data) => {
                    received(data.data.len());
                    if data.data.is_empty() {
                        return Err("服务器返回空数据而未结束读取".into());
                    }
                    bytes.extend_from_slice(&data.data);
                    if bytes.len() > LIMIT {
                        return Err("文件超过 1 MiB，请下载后查看".into());
                    }
                }
                Err(e) if eof(&e) => break,
                Err(e) => return Err(err(e)),
            }
        }
        let after = raw.stat(path).await.map_err(err)?.attrs;
        if a.size != after.size || a.mtime != after.mtime || a.permissions != after.permissions {
            return Err("读取期间文件发生变化，请重新打开".into());
        }
        Ok(bytes)
    }
    .await;
    let closed = raw.close(handle).await.map_err(err);
    let bytes = result?;
    closed?;
    Ok(bytes)
}
struct LocalTemporary(PathBuf);
impl Drop for LocalTemporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn ssh_string(data: &mut Vec<u8>, s: &str) {
    data.extend_from_slice(&(s.len() as u32).to_be_bytes());
    data.extend_from_slice(s.as_bytes());
}
