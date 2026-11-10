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

mod editor;
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
    directory: Option<Directory>,
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
    format!("{:x}", Sha256::digest(bytes))
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
        request["action"] = json!(format!("op.{action}"));
        request["protocolVersion"] = json!(2);
        self.file_operation_control(r, &request)
    }
    pub async fn files_export(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
        local: PathBuf,
    ) -> Result<Value, String> {
        let task = self.files_export_legacy_begin(window, token, &p)?;
        let result = self.files_export_finish(task, Ok(Some(local))).await;
        if result["state"] == "failed" {
            Err(result["error"].as_str().unwrap_or("导出失败").into())
        } else {
            Ok(
                json!({"path":result["path"],"size":result["size"],"cancelled":result["state"]=="cancelled"}),
            )
        }
    }
    pub fn validate_file_download(
        &self,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<(), String> {
        let (v, _surface_call) = self.surface_call_admission(window, token)?;
        let r = self.file_resource(&v.plugin_id, &v.plugin_instance_id, string(p, "id")?)?;
        if r.owner.view != v.id {
            return Err("文件资源不属于此页面".into());
        }
        path(p)?;
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
    pub(crate) fn files_control(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, String> {
        if p["action"] == "open" {
            return self.files_open(plugin, instance, string(p, "viewId")?);
        }
        let id = string(p, "id")?;
        if p["action"] == "close" && !self.files.entries.lock().unwrap().contains_key(id) {
            return Ok(json!({}));
        }
        let r = self.file_resource(plugin, instance, id)?;
        if p["action"].as_str().is_some_and(|a| {
            a.starts_with("op.") || a.starts_with("input.") || a.starts_with("batch.")
        }) {
            return self.file_operation_control(r, p).map_err(Into::into);
        }
        match p["action"].as_str() {
            Some("close") => {
                r.close();
                Ok(json!({}))
            }
            Some("status") => Ok(editor::legacy_job(r.job.lock().unwrap().clone())),
            Some("save") => {
                // Close coordination can still save; it has already frozen new UI input.
                let context = self
                    .ui_state()
                    .resource_view(&r.owner.view)?
                    .context
                    .clone();
                self.start_file_job(r, context, None, None)
            }
            _ => Err("未知文件控制方法".into()),
        }
    }
    pub async fn files_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
    ) -> Result<Value, String> {
        let (view, _surface_call) = if p["action"] == "read" {
            // Initial document loading is allowed before prepared on the active source only.
            let state = self.workbench_state();
            let surface = state
                .ui
                .surfaces
                .get(token)
                .filter(|s| {
                    s.window == window && s.phase == crate::component_ui::SurfacePhase::Active
                })
                .ok_or("候选表面不能打开文件")?;
            let view = state.ui.resource_view(&surface.view)?.clone();
            state.workspaces.validate_owned(window, &view.context)?;
            if state.ui.lifecycle.view_frozen(&view.id) {
                return Err("文件页面正在关闭".into());
            }
            let task = view.calls.enter()?;
            (view, Some(task))
        } else if [
            "describe",
            "chunk",
            "status",
            "home",
            "stat",
            "list",
            "draft.describe",
            "draft.chunk",
            "draft.status",
            "save.status",
            "export.status",
        ]
        .contains(&p["action"].as_str().unwrap_or_default())
        {
            (self.ui_data_view(window, token)?, None)
        } else {
            let (v, task) = self.surface_call_admission(window, token)?;
            (v, Some(task))
        };
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, string(&p, "id")?)?;
        let _task = r.tasks.enter()?;
        if r.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        let action = string(&p, "action")?;
        if action == "list"
            && p["independent"] != true
            && self.ui_data_writable(window, token).is_err()
        {
            return Err("候选文件树只能使用独立目录游标".into());
        }
        if action.starts_with("op.") || action.starts_with("input.") {
            self.ui_data_writable(window, token)?;
            return self.file_operation_control(r, &p).map_err(Into::into);
        }
        if action == "status" {
            return Ok(editor::legacy_job(r.job.lock().unwrap().clone()));
        }
        if action == "cancel" {
            self.ui_data_writable(window, token)?;
            r.job_cancel.lock().unwrap().cancel();
            return Ok(json!({}));
        }
        // Local draft reads are allowed while a replacement surface is mounting.
        if ![
            "describe",
            "chunk",
            "home",
            "stat",
            "list",
            "read",
            "draft.describe",
            "draft.chunk",
            "draft.status",
            "save.status",
            "export.status",
        ]
        .contains(&action)
        {
            self.ui_data_writable(window, token)?;
        }
        if action == "save" {
            return self.start_file_job(r, view.context, None, None);
        }
        if action.starts_with("draft.") {
            let readonly = ["draft.describe", "draft.chunk", "draft.status"].contains(&action);
            let _admission = if readonly {
                None
            } else {
                Some(self.surface_write_admission(window, token)?)
            };
            return Ok(match r.editor.lock().unwrap().draft(token, &p) {
                Ok(value) => json!({"ok":true,"value":value}),
                Err(error) => json!({"ok":false,"error":{"code":"draftRejected","message":error}}),
            });
        }
        if action.starts_with("save.") || action.starts_with("export.") {
            return Ok(
                match self.files_editor(&view.plugin_id, &view.plugin_instance_id, &p) {
                    Ok(value) => json!({"ok":true,"value":value}),
                    Err(error) => {
                        json!({"ok":false,"error":{"code":"saveRejected","message":error}})
                    }
                },
            );
        }
        if ["describe", "chunk", "stage"].contains(&action) {
            let _admission = if action == "stage" {
                Some(self.surface_write_admission(window, token)?)
            } else {
                None
            };
            let mut e = r.editor.lock().unwrap();
            if action == "describe" {
                return Ok(e.describe());
            }
            if action == "chunk" {
                let d = e.document.as_ref().ok_or("尚未打开文件")?;
                let offset = p["offset"].as_u64().ok_or("缺少 offset")? as usize;
                if offset > d.bytes.len() {
                    return Err("文件数据偏移无效".into());
                }
                return Ok(
                    json!({"data":&d.bytes[offset..(offset+BLOCK).min(d.bytes.len())],"size":d.bytes.len(),"revision":d.revision}),
                );
            }
            if e.protocol {
                return Err("新版草稿不接纳旧写入协议".into());
            }
            let bytes: Vec<u8> = serde_json::from_value(p["data"].clone()).map_err(err)?;
            if bytes.len() > BLOCK {
                return Err("文件数据块超过 16 KiB".into());
            }
            if p["begin"] == true {
                e.legacy_upload = Some(Vec::new());
            }
            let buffer = e.legacy_upload.as_mut().ok_or("没有正在上传的草稿")?;
            if p["offset"].as_u64() != Some(buffer.len() as u64)
                || buffer.len() + bytes.len() > LIMIT
            {
                return Err("草稿偏移无效或超过 1 MiB".into());
            }
            buffer.extend(bytes);
            if p["done"] == true {
                let bytes = e.legacy_upload.take().unwrap();
                let d = e.document.as_mut().ok_or("尚未打开文件")?;
                d.bytes = bytes.into();
                d.revision += 1;
                return Ok(json!({"revision":d.revision}));
            }
            return Ok(json!({}));
        }
        let cancel = r.cancel.clone();
        let work = async {
            let mut s = r.state.lock().await;
            match action {
                "home" => {
                    let f = sftp(self, &mut s, &view.context).await?;
                    Ok(json!({"path":canonical(&f.raw,".").await?}))
                }
                "list" => {
                    let independent = p["independent"] == true || p["cursor"].is_string();
                    let mut cursor = p["cursor"].as_str().unwrap_or("").to_string();
                    if p["next"] != true {
                        let target = path(&p)?.to_string();
                        let old = if independent {
                            s.directories.remove(&cursor)
                        } else {
                            s.directory.take()
                        };
                        if let Some(old) = old {
                            if let Some(f) = &s.session {
                                if !old.eof {
                                    let _ = f.raw.close(old.handle).await;
                                }
                            }
                        }
                        let f = sftp(self, &mut s, &view.context).await?;
                        if independent {
                            let State {
                                directory_order,
                                directories,
                                ..
                            } = &mut *s;
                            directory_order.retain(|id| directories.contains_key(id));
                            if s.directories.len() >= 32 {
                                if let Some(id) = s.directory_order.pop_front() {
                                    if let Some(old) = s.directories.remove(&id) {
                                        if !old.eof {
                                            let _ = f.raw.close(old.handle).await;
                                        }
                                    }
                                }
                            }
                        }
                        let resolved = canonical(&f.raw, &target).await?;
                        let handle = f.raw.opendir(resolved.clone()).await.map_err(err)?.handle;
                        let directory = Directory {
                            path: resolved,
                            handle,
                            pending: VecDeque::new(),
                            eof: false,
                        };
                        if independent {
                            cursor = self.next_id();
                            s.directories.insert(cursor.clone(), directory);
                            s.directory_order.push_back(cursor.clone());
                        } else {
                            s.directory = Some(directory);
                        }
                    }
                    let State {
                        session,
                        directory,
                        directories,
                        ..
                    } = &mut *s;
                    let f = session.as_ref().ok_or("文件会话已失效，请刷新目录")?;
                    if f.context != view.context {
                        return Err("连接已更换，请刷新目录".into());
                    }
                    let d = if independent {
                        directories.get_mut(&cursor)
                    } else {
                        directory.as_mut()
                    }
                    .ok_or("目录游标已失效，请刷新此目录")?;
                    let mut entries = Vec::new();
                    while entries.len() < 100 {
                        if let Some(v) = d.pending.pop_front() {
                            entries.push(v);
                            continue;
                        }
                        if d.eof {
                            break;
                        }
                        match f.raw.readdir(d.handle.clone()).await {
                            Ok(batch) => {
                                for entry in batch.files {
                                    if entry.filename != "." && entry.filename != ".." {
                                        d.pending.push_back(json!({"name":entry.filename,"attrs":attrs(&entry.attrs)}));
                                    }
                                }
                            }
                            Err(e) if eof(&e) => {
                                d.eof = true;
                                let _ = f.raw.close(d.handle.clone()).await;
                            }
                            Err(e) => return Err(err(e)),
                        }
                    }
                    let more = !d.eof || !d.pending.is_empty();
                    let result = json!({"path":d.path,"entries":entries,"more":more,"cursor":if independent && more {Some(cursor.as_str())}else{None}});
                    if independent && !more {
                        directories.remove(&cursor);
                    }
                    Ok(result)
                }
                "closeDirectory" => {
                    if let Some(old) = s.directories.remove(string(&p, "cursor")?) {
                        if !old.eof {
                            if let Some(f) = &s.session {
                                let _ = f.raw.close(old.handle).await;
                            }
                        }
                    }
                    Ok(json!({}))
                }
                "stat" => {
                    let f = sftp(self, &mut s, &view.context).await?;
                    let target = path(&p)?;
                    let a = f.raw.stat(target).await.map_err(err)?.attrs;
                    Ok(json!({"path":canonical(&f.raw,target).await?,"attrs":attrs(&a)}))
                }
                "resolveLocation" => {
                    let target = location::normalize(&p)?;
                    let f = sftp(self, &mut s, &view.context).await?;
                    if p["filesystemView"] == "shell-reported" {
                        let witness = &p["witness"];
                        let marker = witness["path"]
                            .as_str()
                            .filter(|s| {
                                s.starts_with('/')
                                    && s.len() <= 4096
                                    && !s.chars().any(char::is_control)
                            })
                            .ok_or("unsupportedLocation: 缺少文件系统对应标记")?;
                        let expected = witness["value"]
                            .as_str()
                            .filter(|s| !s.is_empty() && s.len() <= 128)
                            .ok_or("unsupportedLocation: 文件系统标记无效")?;
                        let a = f
                            .raw
                            .lstat(marker)
                            .await
                            .map_err(|_| "unsupportedLocation: 终端与文件服务的文件系统无法对应")?
                            .attrs;
                        if !a.file_type().is_file() || a.size != Some(expected.len() as u64) {
                            return Err(
                                "unsupportedLocation: 终端与文件服务的文件系统无法对应".into()
                            );
                        }
                        let bytes = read_small(&f.raw, marker, &a, |n| {
                            self.record_business(&r.owner, 0, n as u64)
                        })
                        .await?;
                        if bytes != expected.as_bytes() {
                            return Err(
                                "unsupportedLocation: 终端与文件服务的文件系统无法对应".into()
                            );
                        }
                    }
                    let resolved = canonical(&f.raw, &target).await?;
                    let a = f.raw.stat(resolved.clone()).await.map_err(err)?.attrs;
                    self.workspace_state().validate_identity(&view.context)?;
                    Ok(json!({"path":resolved,"attrs":attrs(&a)}))
                }
                "read" => {
                    if r.editor.lock().unwrap().document.is_some() {
                        return Err("此编辑器已经打开文件，刷新不会覆盖草稿".into());
                    }
                    let target = path(&p)?.to_string();
                    let f = sftp(self, &mut s, &view.context).await?;
                    let resolved = canonical(&f.raw, &target).await?;
                    let a = f.raw.lstat(resolved.clone()).await.map_err(err)?.attrs;
                    if !a.file_type().is_file() {
                        return Err("只支持编辑普通文件".into());
                    }
                    let bytes = read_small(&f.raw, &resolved, &a, |n| {
                        self.record_business(&r.owner, 0, n as u64)
                    })
                    .await?;
                    let version = version(&a, &bytes);
                    drop(s);
                    let admission = self.workbench_state();
                    if !admission.ui.surfaces.get(token).is_some_and(|surface| {
                        surface.window == window
                            && surface.phase == crate::component_ui::SurfacePhase::Active
                    }) || admission.ui.resource_view(&view.id)?.context != view.context
                        || admission.ui.lifecycle.view_frozen(&view.id)
                    {
                        return Err("文件输入权已改变".into());
                    }
                    let mut editor = r.editor.lock().unwrap();
                    if editor.document.is_some() {
                        return Err("此编辑器已经打开文件".into());
                    }
                    editor.document = Some(Document {
                        id: self.next_id(),
                        path: target,
                        resolved,
                        version,
                        attrs: a,
                        bytes: bytes.into(),
                        revision: 0,
                    });
                    Ok(editor.describe())
                }
                _ => Err("未知文件数据方法".into()),
            }
        };
        let result = tokio::select! {biased;_=cancel.cancelled()=>Err("文件页面已关闭".into()),v=tokio::time::timeout(Duration::from_secs(35),work)=>v.map_err(|_|"文件操作超时；请刷新后重试".to_string()).and_then(|v|v)};
        if result.is_err() && ["list", "home", "stat", "read"].contains(&action) {
            let mut state = r.state.lock().await;
            if let Some(session) = state.session.take() {
                let _ = session.raw.close_session();
            }
            state.directory = None;
            state.directories.clear();
            state.directory_order.clear();
        }
        result
    }
    fn start_file_job(
        self: &Arc<Self>,
        r: Arc<Resource>,
        context: Context,
        download: Option<(String, PathBuf)>,
        save_params: Option<&Value>,
    ) -> Result<Value, String> {
        if let Some(p) = save_params {
            let e = r.editor.lock().unwrap();
            if e.save_request["requestId"] == p["requestId"] && p["requestId"].is_string() {
                for key in ["sequence", "expectedDraftRef", "expectedContext"] {
                    if e.save_request[key] != p[key] {
                        return Err("相同保存身份参数变化".into());
                    }
                }
                return Ok(e.save_result.clone());
            }
        }
        let task = r.tasks.enter()?;
        self.connection_session(&context)?;
        let ui = self.ui_state();
        let view = ui.resource_view(&r.owner.view)?;
        if view.context != context || view.plugin_instance_id != r.owner.instance {
            return Err("文件目标上下文已改变".into());
        }
        let close_step = if self.is_closing() || ui.lifecycle.view_frozen(&view.id) {
            if download.is_some() {
                return Err("关闭中不能开始新的下载".into());
            }
            Some(
                ui.lifecycle
                    .operations
                    .values()
                    .find(|o| {
                        o.adopted_by.is_none()
                            && o.phase == crate::lifecycle::Phase::Deciding
                            && o.decision.as_deref() == Some("save")
                            && o.working
                            && o.saving_view.as_deref() == Some(r.owner.view.as_str())
                    })
                    .ok_or("此文件保存不属于当前关闭步骤")?
                    .id
                    .clone(),
            )
        } else {
            None
        };
        let mut close_saves = r.close_saves.lock().unwrap();
        if let Some(result) = close_step.as_ref().and_then(|id| close_saves.get(id)) {
            return if r.job.lock().unwrap()["id"] == result["id"] {
                Ok(r.job.lock().unwrap().clone())
            } else {
                Err("关闭保存回执已退役".into())
            };
        }
        let mut editor = r.editor.lock().unwrap();
        if let Some(p) = save_params {
            if editor.save_request["requestId"] == p["requestId"] && p["requestId"].is_string() {
                for key in ["sequence", "expectedDraftRef", "expectedContext"] {
                    if editor.save_request[key] != p[key] {
                        return Err("相同保存身份参数变化".into());
                    }
                }
                return Ok(editor.save_result.clone());
            }
            if !editor.save_request.is_null() {
                return Err("请先确认并释放上次保存回执".into());
            }
        }
        let snapshot = if download.is_none() {
            if let Some(reason) = &editor.write_block {
                return Err(reason.clone());
            }
            let d = editor.document.as_ref().ok_or("尚未打开文件")?.clone();
            if let Some(p) = save_params {
                if p["expectedDraftRef"] != editor::reference(&d) {
                    return Err("revisionConflict: 草稿版本已变化".into());
                }
                if p["expectedContext"] != serde_json::to_value(&context).map_err(err)? {
                    return Err("保存上下文已变化".into());
                }
                let sequence = editor::number(p, "sequence")?;
                if sequence <= editor.save_sequence {
                    return Err("expired: 保存序号已退役".into());
                }
                if p["requestId"]
                    .as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > 160)
                {
                    return Err("保存请求身份无效".into());
                }
            } else if editor.protocol {
                return Err("新版草稿必须按固定修订保存".into());
            }
            Some(d)
        } else {
            None
        };
        let id = self.next_id();
        let cancel = r.cancel.child_token();
        {
            let mut job = r.job.lock().unwrap();
            if ["running", "committing"].contains(&job["state"].as_str().unwrap_or("")) {
                return Err("文件任务正在进行".into());
            }
            // Publish the cancellation token before callers can observe running.
            *r.job_cancel.lock().unwrap() = cancel.clone();
            *job = json!({"id":id,"state":"running","kind":if download.is_some(){"download"}else{"save"},"received":"0"});
        }
        if let Some(p) = save_params {
            editor.save_sequence = editor::number(p, "sequence")?;
            editor.save_request = p.clone();
            editor.save_result = r.job.lock().unwrap().clone();
            editor.protocol = true;
        }
        drop(editor);
        if let Some(step) = close_step {
            close_saves.insert(step, json!({"id":id,"state":"running"}));
        }
        drop(close_saves);
        drop(ui);
        let core = self.clone();
        let job_id = id.clone();
        let saving = download.is_none();
        tauri::async_runtime::spawn(async move {
            let _task = task;
            let result = if let Some((remote, local)) = download {
                download_file(&core, &r, &context, &remote, &local, &cancel)
                    .await
                    .map_err(SaveFailure::from)
            } else {
                save_file(&core, &r, &context, &cancel, snapshot.unwrap()).await
            };
            let result = match result {
                Ok(data) => {
                    json!({"id":job_id,"kind":if data.get("revision").is_some(){"save"}else{"download"},"state":"completed","result":data})
                }
                Err(error) => {
                    json!({"id":job_id,"kind":if saving{"save"}else{"download"},"state":if error.unknown{"unknown"}else{"failed"},"error":error.message})
                }
            };
            let mut editor = r.editor.lock().unwrap();
            if saving {
                editor.save_result = result.clone();
            }
            *r.job.lock().unwrap() = result;
        });
        Ok(json!({"id":id,"state":"running"}))
    }
    pub async fn files_download(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
        local: PathBuf,
    ) -> Result<Value, String> {
        let view = self.ui_data_writable(window, token)?;
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, string(&p, "id")?)?;
        if r.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        self.start_file_job(r, view.context, Some((path(&p)?.into(), local)), None)
    }
}
async fn sftp(core: &AppCore, s: &mut State, context: &Context) -> Result<Arc<Sftp>, String> {
    if s.session.as_ref().is_none_or(|f| f.context != *context) {
        s.session = None;
        s.directory = None;
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
fn ssh_string(data: &mut Vec<u8>, s: &str) {
    data.extend_from_slice(&(s.len() as u32).to_be_bytes());
    data.extend_from_slice(s.as_bytes());
}
#[derive(Debug)]
struct SaveFailure {
    unknown: bool,
    message: String,
}
impl From<String> for SaveFailure {
    fn from(message: String) -> Self {
        Self {
            unknown: false,
            message,
        }
    }
}
impl From<&str> for SaveFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
async fn save_file(
    core: &Arc<AppCore>,
    r: &Resource,
    context: &Context,
    cancel: &CancellationToken,
    doc: Document,
) -> Result<Value, SaveFailure> {
    let mut state = State {
        transports: r.transports.clone(),
        ..State::default()
    };
    let s = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("保存已取消，草稿保留".into()),
        session = sftp(core, &mut state, context) => session?,
    };
    drop(state);
    if !s.atomic {
        return Err("服务器不支持原子替换，未修改原文件；请下载后通过其他方式保存".into());
    }
    let temporary = format!("{}.sodeck-{}.tmp", doc.resolved, core.next_id());
    let work = async {
        let verify = async {
            if canonical(&s.raw, &doc.path).await? != doc.resolved {
                return Err("文件链接目标已经变化，草稿保留".to_string());
            }
            let attrs = s.raw.lstat(doc.resolved.clone()).await.map_err(err)?.attrs;
            if !attrs.file_type().is_file() {
                return Err("文件类型发生变化，草稿保留".into());
            }
            let current = read_small(&s.raw, &doc.resolved, &attrs, |n| {
                core.record_business(&r.owner, 0, n as u64)
            })
            .await?;
            if version(&attrs, &current) != doc.version {
                return Err("远端文件已被修改，未覆盖；请下载草稿并重新打开比较".into());
            }
            Ok(())
        };
        verify.await?;
        let handle = s
            .raw
            .open(
                temporary.clone(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await
            .map_err(err)?
            .handle;
        let written = async {
            for (i, block) in doc.bytes.chunks(BLOCK).enumerate() {
                s.raw
                    .write(handle.clone(), (i * BLOCK) as u64, block.to_vec())
                    .await
                    .map_err(err)?;
                core.record_business(&r.owner, block.len() as u64, 0);
            }
            s.raw
                .fsetstat(
                    handle.clone(),
                    FileAttributes {
                        permissions: doc.attrs.permissions.map(|p| p & 0o7777),
                        ..Default::default()
                    },
                )
                .await
                .map_err(err)?;
            Ok::<_, String>(())
        }
        .await;
        let closed = s.raw.close(handle).await.map_err(err);
        written?;
        closed?;
        if canonical(&s.raw, &doc.path).await? != doc.resolved {
            return Err("文件链接目标已经变化，未替换原文件".into());
        }
        let a = s.raw.lstat(doc.resolved.clone()).await.map_err(err)?.attrs;
        let bytes = read_small(&s.raw, &doc.resolved, &a, |n| {
            core.record_business(&r.owner, 0, n as u64)
        })
        .await?;
        if version(&a, &bytes) != doc.version {
            return Err("远端文件已被修改，未覆盖；草稿保留".into());
        }
        Ok::<_, String>(())
    };
    let prepared = tokio::select! {biased;_=cancel.cancelled()=>Err("保存已取消，草稿保留".into()),v=tokio::time::timeout(Duration::from_secs(120),work)=>v.map_err(|_|"保存准备超时".to_string()).and_then(|v|v)};
    if let Err(error) = prepared {
        let _ = s.raw.close_session();
        return Err(format!("{error}；如已创建临时文件则保留在 {temporary}").into());
    }
    if cancel.is_cancelled() {
        let _ = s.raw.remove(temporary).await;
        return Err("保存已取消，未提交".into());
    }
    r.job.lock().unwrap()["state"] = json!("committing");
    r.editor.lock().unwrap().save_result["state"] = json!("committing");
    let attrs = match commit_file(&s.raw, &temporary, &doc.resolved).await {
        Ok(attrs) => attrs,
        Err(error) => {
            if error.unknown {
                r.editor.lock().unwrap().write_block = Some(error.message.clone());
            }
            return Err(error);
        }
    };
    let ui = core.ui_state();
    let current = ui
        .resource_view(&r.owner.view)
        .is_ok_and(|v| v.context == *context);
    let mut editor = r.editor.lock().unwrap();
    let matches = editor
        .document
        .as_ref()
        .is_some_and(|d| d.id == doc.id && d.version == doc.version);
    if current && matches {
        editor.saved = doc.revision;
    }
    let mut result = json!({"revision":doc.revision,"draftRef":editor::reference(&doc),"size":doc.bytes.len(),"committed":true});
    if let Some(a) = attrs.filter(|_| current && matches) {
        let version = version(&a, &doc.bytes);
        let d = editor.document.as_mut().unwrap();
        d.version = version.clone();
        d.attrs = a;
        result["version"] = json!(version);
    } else {
        let warning =
            "该修订已提交，但后续基线或连接身份未确认；请核对后重新打开，期间可以导出草稿";
        editor.write_block = Some(warning.into());
        result["baselineUnavailable"] = json!(true);
        result["warning"] = json!(warning);
    }
    Ok(result)
}

// This phase has no cancellation select: sending a commit cannot be undone locally.
async fn commit_file(
    raw: &RawSftpSession,
    temporary: &str,
    destination: &str,
) -> Result<Option<FileAttributes>, SaveFailure> {
    let mut data = Vec::new();
    ssh_string(&mut data, temporary);
    ssh_string(&mut data, destination);
    match raw.extended("posix-rename@openssh.com", data).await {
        Ok(Packet::Status(status)) if status.status_code == StatusCode::Ok => (),
        Ok(Packet::Status(status)) | Err(Error::Status(status))
            if !matches!(
                status.status_code,
                StatusCode::NoConnection | StatusCode::ConnectionLost
            ) =>
        {
            let _ = raw.remove(temporary).await;
            return Err(format!("服务器拒绝保存：{}", status.error_message).into());
        }
        other => {
            let message = format!("提交结果未知，请核对服务器；草稿保留，不能直接重试：{other:?}");
            let _ = raw.close_session();
            return Err(SaveFailure {
                unknown: true,
                message,
            });
        }
    }
    Ok(raw.stat(destination).await.ok().map(|r| r.attrs))
}

struct LocalTemporary(PathBuf);
impl Drop for LocalTemporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
async fn download_file(
    core: &AppCore,
    r: &Resource,
    context: &Context,
    remote: &str,
    local: &Path,
    cancel: &CancellationToken,
) -> Result<Value, String> {
    let mut state = r.state.lock().await;
    let s = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("下载已取消，本地原文件保留".into()),
        session = sftp(core, &mut state, context) => session?,
    };
    drop(state);
    let parent = local.parent().ok_or("下载目标没有父目录")?;
    let temporary = LocalTemporary(parent.join(format!(".sodeck-{}.part", core.next_id())));
    let before = std::fs::metadata(local)
        .ok()
        .map(|m| (m.len(), m.modified().ok()));
    let work = async {
        let target = canonical(&s.raw, remote).await?;
        let a = s.raw.stat(target.clone()).await.map_err(err)?.attrs;
        if !a.file_type().is_file() {
            return Err("只支持下载普通文件".into());
        }
        let handle = s
            .raw
            .open(target.clone(), OpenFlags::READ, FileAttributes::default())
            .await
            .map_err(err)?
            .handle;
        let result = async {
            let mut output = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary.0)
                .await
                .map_err(err)?;
            let mut offset = 0u64;
            loop {
                match s.raw.read(handle.clone(), offset, BLOCK as u32).await {
                    Ok(data) => {
                        if data.data.is_empty() {
                            return Err("服务器返回空数据而未结束下载".into());
                        }
                        core.record_business(&r.owner, 0, data.data.len() as u64);
                        output.write_all(&data.data).await.map_err(err)?;
                        offset += data.data.len() as u64;
                        let mut job = r.job.lock().unwrap();
                        job["received"] = json!(offset.to_string());
                        job["total"] = json!(a.size.map(|s| s.to_string()));
                    }
                    Err(e) if eof(&e) => break,
                    Err(e) => return Err(err(e)),
                }
            }
            let after = s.raw.stat(target).await.map_err(err)?.attrs;
            if a.size != after.size || a.mtime != after.mtime || a.size.is_some_and(|n| n != offset)
            {
                return Err("下载期间远端文件变化，未替换本地文件".into());
            }
            output.flush().await.map_err(err)?;
            output.sync_all().await.map_err(err)?;
            drop(output);
            let current = std::fs::metadata(local)
                .ok()
                .map(|m| (m.len(), m.modified().ok()));
            if current != before {
                return Err("本地目标在下载期间变化，未覆盖".into());
            }
            tokio::fs::rename(&temporary.0, local).await.map_err(err)?;
            Ok(json!({"path":local,"size":offset.to_string()}))
        }
        .await;
        let closed = s.raw.close(handle).await.map_err(err);
        let result = result?;
        closed?;
        Ok(result)
    };
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err("下载已取消，本地原文件保留".into()),v=work=>v};
    if result.is_err() {
        let _ = s.raw.close_session();
        let mut state = r.state.lock().await;
        if state
            .session
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &s))
        {
            state.session = None;
            state.directory = None;
            state.directories.clear();
            state.directory_order.clear();
        }
    }
    result
}
