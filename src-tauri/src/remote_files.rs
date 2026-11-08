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

mod operations;

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
    document: Option<Document>,
    upload: Option<Vec<u8>>,
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
    path: String,
    resolved: String,
    version: String,
    attrs: FileAttributes,
    bytes: Vec<u8>,
    revision: u64,
}
impl Files {
    pub(crate) fn saving(&self, view: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .values()
            .any(|r| r.owner.view == view && r.job.lock().unwrap()["state"] == "running")
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
            pending |= resource.operations.cancel_pending()
                || resource.job.lock().unwrap()["state"] == "running";
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
    pub async fn files_export(
        &self,
        window: &str,
        token: &str,
        p: Value,
        local: PathBuf,
    ) -> Result<Value, String> {
        self.validate_file_download(window, token, &p)?;
        let v = self.ui_data_writable(window, token)?;
        let r = self.file_resource(&v.plugin_id, &v.plugin_instance_id, string(&p, "id")?)?;
        let _task = r.tasks.enter()?;
        let bytes = r
            .state
            .lock()
            .await
            .document
            .as_ref()
            .ok_or("尚未打开文件")?
            .bytes
            .clone();
        let temporary = LocalTemporary(
            local
                .parent()
                .ok_or("目标没有父目录")?
                .join(format!(".sodeck-{}.part", self.next_id())),
        );
        let before = std::fs::metadata(&local)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary.0)
            .await
            .map_err(err)?;
        output.write_all(&bytes).await.map_err(err)?;
        output.sync_all().await.map_err(err)?;
        drop(output);
        if std::fs::metadata(&local)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
            != before
        {
            return Err("本地目标已变化，未覆盖".into());
        }
        tokio::fs::rename(&temporary.0, &local).await.map_err(err)?;
        Ok(json!({"path":local,"size":bytes.len()}))
    }
    pub fn validate_file_download(
        &self,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<(), String> {
        let v = self.ui_data_writable(window, token)?;
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
            return self.file_operation_control(r, p);
        }
        match p["action"].as_str() {
            Some("close") => {
                r.close();
                Ok(json!({}))
            }
            Some("status") => Ok(r.job.lock().unwrap().clone()),
            Some("save") => {
                // Close coordination can still save; it has already frozen new UI input.
                let context = self
                    .component_ui
                    .lock()
                    .unwrap()
                    .resource_view(&r.owner.view)?
                    .context
                    .clone();
                self.start_file_job(r, context, None)
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
        let view = self.ui_data_view(window, token)?;
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, string(&p, "id")?)?;
        let _task = r.tasks.enter()?;
        if r.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        let action = string(&p, "action")?;
        if action.starts_with("op.") || action.starts_with("input.") {
            self.ui_data_writable(window, token)?;
            return self.file_operation_control(r, &p);
        }
        if action == "status" {
            return Ok(r.job.lock().unwrap().clone());
        }
        if action == "cancel" {
            self.ui_data_writable(window, token)?;
            r.job_cancel.lock().unwrap().cancel();
            return Ok(json!({}));
        }
        // Local draft reads are allowed while a replacement surface is mounting.
        if !["describe", "chunk"].contains(&action) {
            self.ui_data_writable(window, token)?;
        }
        if action == "save" {
            return self.start_file_job(r, view.context, None);
        }
        let cancel = r.cancel.clone();
        let work = async {
            let mut s = r.state.lock().await;
            match action {
                "describe" => Ok(if let Some(d) = &s.document {
                    json!({"path":d.path,"size":d.bytes.len(),"revision":d.revision,"version":d.version})
                } else {
                    json!({})
                }),
                "chunk" => {
                    let d = s.document.as_ref().ok_or("尚未打开文件")?;
                    let offset = p["offset"].as_u64().ok_or("缺少 offset")? as usize;
                    if offset > d.bytes.len() {
                        return Err("文件数据偏移无效".into());
                    }
                    Ok(
                        json!({"data":&d.bytes[offset..(offset+BLOCK).min(d.bytes.len())],"size":d.bytes.len(),"revision":d.revision}),
                    )
                }
                "stage" => {
                    let bytes: Vec<u8> = serde_json::from_value(p["data"].clone()).map_err(err)?;
                    if bytes.len() > BLOCK {
                        return Err("文件数据块超过 16 KiB".into());
                    }
                    if p["begin"] == true {
                        s.upload = Some(Vec::new());
                    }
                    let buffer = s.upload.as_mut().ok_or("没有正在上传的草稿")?;
                    let offset = p["offset"].as_u64().ok_or("缺少 offset")? as usize;
                    if offset != buffer.len() || buffer.len() + bytes.len() > LIMIT {
                        return Err("草稿偏移无效或超过 1 MiB".into());
                    }
                    buffer.extend(bytes);
                    if p["done"] == true {
                        let bytes = s.upload.take().unwrap();
                        let d = s.document.as_mut().ok_or("尚未打开文件")?;
                        d.bytes = bytes;
                        d.revision += 1;
                        return Ok(json!({"revision":d.revision}));
                    }
                    Ok(json!({}))
                }
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
                "read" => {
                    if s.document.is_some() {
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
                    let result = json!({"path":target,"resolved":resolved,"size":bytes.len(),"version":version,"revision":0});
                    s.document = Some(Document {
                        path: target,
                        resolved,
                        version,
                        attrs: a,
                        bytes,
                        revision: 0,
                    });
                    Ok(result)
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
    ) -> Result<Value, String> {
        let task = r.tasks.enter()?;
        self.connection_session(&context)?;
        let ui = self.component_ui.lock().unwrap();
        let view = ui.resource_view(&r.owner.view)?;
        if view.context != context || view.plugin_instance_id != r.owner.instance {
            return Err("文件目标上下文已改变".into());
        }
        let close_step = if self.is_closing() || ui.frozen(&context.workspace_id) {
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
            return Ok(result.clone());
        }
        let id = self.next_id();
        let cancel = r.cancel.child_token();
        {
            let mut job = r.job.lock().unwrap();
            if job["state"] == "running" {
                return Err("文件任务正在进行".into());
            }
            // Publish the cancellation token before callers can observe running.
            *r.job_cancel.lock().unwrap() = cancel.clone();
            *job = json!({"id":id,"state":"running","kind":if download.is_some(){"download"}else{"save"},"received":"0"});
        }
        if let Some(step) = close_step {
            close_saves.insert(step, json!({"id":id,"state":"running"}));
        }
        drop(close_saves);
        drop(ui);
        let core = self.clone();
        let job_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let _task = task;
            let result = if let Some((remote, local)) = download {
                download_file(&core, &r, &context, &remote, &local, &cancel).await
            } else {
                save_file(&core, &r, &context, &cancel).await
            };
            let result = match result {
                Ok(data) => json!({"id":job_id,"state":"completed","result":data}),
                Err(error) => json!({"id":job_id,"state":"failed","error":error}),
            };
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
        self.start_file_job(r, view.context, Some((path(&p)?.into(), local)))
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
async fn save_file(
    core: &Arc<AppCore>,
    r: &Resource,
    context: &Context,
    cancel: &CancellationToken,
) -> Result<Value, String> {
    let mut state = r.state.lock().await;
    let s = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("保存已取消，草稿保留".into()),
        session = sftp(core, &mut state, context) => session?,
    };
    let doc = state.document.as_ref().ok_or("尚未打开文件")?.clone();
    drop(state);
    if !s.atomic {
        return Err("服务器不支持原子替换，未修改原文件；请下载后通过其他方式保存".into());
    }
    let temporary = format!("{}.sodeck-{}.tmp", doc.resolved, core.next_id());
    let mut committing = false;
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
        let mut data = Vec::new();
        ssh_string(&mut data, &temporary);
        ssh_string(&mut data, &doc.resolved);
        committing = true;
        match s
            .raw
            .extended("posix-rename@openssh.com", data)
            .await
            .map_err(err)?
        {
            Packet::Status(status) if status.status_code == StatusCode::Ok => (),
            other => return Err(format!("服务器未确认提交：{other:?}")),
        }
        let a = s.raw.stat(doc.resolved.clone()).await.map_err(err)?.attrs;
        let result = json!({"revision":doc.revision,"version":version(&a,&doc.bytes),"size":doc.bytes.len()});
        Ok((result, a))
    };
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err("保存已取消".into()),v=tokio::time::timeout(Duration::from_secs(120),work)=>v.map_err(|_|"保存超时".to_string()).and_then(|v|v)};
    // Never remove or truncate the original; temporary cleanup is best effort.
    let _ = tokio::time::timeout(Duration::from_secs(2), s.raw.remove(temporary)).await;
    match result {
        Ok((result, a)) => {
            let mut state = r.state.lock().await;
            let d = state.document.as_mut().unwrap();
            d.version = result["version"].as_str().unwrap().into();
            d.attrs = a;
            Ok(result)
        }
        Err(error) => Err(if committing {
            format!("提交结果未知，请重新读取远端确认；草稿保留：{error}")
        } else {
            error
        }),
    }
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
