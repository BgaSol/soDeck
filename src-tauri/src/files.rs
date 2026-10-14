use crate::{
    blocking,
    model::{AppError, AppResult},
    ssh, State,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::{FileStat, OpenFlags, OpenType, RenameFlags, Sftp};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
use tauri::{Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const TEXT_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub struct FileState {
    editors: Mutex<HashMap<String, EditorHandle>>,
    downloads: Mutex<Vec<DownloadHandle>>,
    active_downloads: Mutex<usize>,
    download_slot: Condvar,
}

#[derive(Clone)]
struct EditorHandle {
    spec: ssh::ConnectionSpec,
    path: String,
    gate: Arc<Mutex<()>>,
    dirty: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDocument {
    server_name: String,
    path: String,
    text: String,
    version: String,
    size: usize,
}

fn remote_path(path: &str) -> AppResult<&Path> {
    if path.is_empty() || path.contains('\0') {
        return Err(AppError::new("path", "远端文件路径无效"));
    }
    Ok(Path::new(path))
}

fn text_bytes(sftp: &Sftp, path: &str) -> AppResult<(Vec<u8>, FileStat)> {
    let path = remote_path(path)?;
    let stat = sftp.lstat(path)?;
    if !stat.is_file() {
        return Err(AppError::new("fileType", "只能在线编辑普通文本文件"));
    }
    if stat.size.unwrap_or(0) > TEXT_LIMIT as u64 {
        return Err(AppError::new(
            "tooLarge",
            "在线编辑支持不超过 1 MB 的文本文件，请下载后编辑",
        ));
    }
    let mut bytes = Vec::new();
    sftp.open(path)?
        .take((TEXT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_text(&bytes)?;
    Ok((bytes, stat))
}

fn validate_text(bytes: &[u8]) -> AppResult<&str> {
    if bytes.len() > TEXT_LIMIT {
        return Err(AppError::new(
            "tooLarge",
            "在线编辑支持不超过 1 MB 的文本文件，请下载后编辑",
        ));
    }
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = std::str::from_utf8(content)
        .map_err(|_| AppError::new("encoding", "此文件不是 UTF-8 文本，请下载后编辑"))?;
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t' | '\u{c}'))
    {
        return Err(AppError::new(
            "binary",
            "此文件包含二进制内容，请下载后查看",
        ));
    }
    Ok(text)
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn document(editor: &EditorHandle, bytes: &[u8]) -> AppResult<EditorDocument> {
    Ok(EditorDocument {
        server_name: editor.spec.server.name.clone(),
        path: editor.path.clone(),
        text: validate_text(bytes)?.to_owned(),
        version: revision(bytes),
        size: bytes.len(),
    })
}

fn edited_bytes(original: &[u8], text: &str) -> AppResult<Vec<u8>> {
    let source = validate_text(original)?;
    // Code editors normalize to LF; preserve the existing file's CRLF convention.
    let normalized = text.replace("\r\n", "\n");
    if normalized == source.replace("\r\n", "\n") {
        return Ok(original.to_vec());
    }
    let crlf = source.contains("\r\n") && !source.replace("\r\n", "").contains('\n');
    let encoded = if crlf {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    let mut bytes = Vec::with_capacity(encoded.len() + 3);
    if original.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    bytes.extend_from_slice(encoded.as_bytes());
    validate_text(&bytes)?;
    Ok(bytes)
}

#[tauri::command]
pub async fn open_editor(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    path: String,
) -> AppResult<String> {
    let spec = state.spec(&session_id, window.label())?;
    let editor = blocking(move || {
        let session = ssh::establish_spec(&spec)?;
        let sftp = session.sftp()?;
        let canonical = sftp
            .realpath(remote_path(&path)?)?
            .to_string_lossy()
            .to_string();
        text_bytes(&sftp, &canonical)?;
        Ok(EditorHandle {
            spec,
            path: canonical,
            gate: Arc::new(Mutex::new(())),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    })
    .await?;
    let (label, create) = {
        let mut editors = state.files.editors.lock().unwrap();
        if let Some((label, _)) = editors.iter().find(|(_, item)| {
            item.spec.server.id == editor.spec.server.id && item.path == editor.path
        }) {
            (label.clone(), false)
        } else {
            let label = format!("editor-{}", uuid::Uuid::new_v4());
            editors.insert(label.clone(), editor.clone());
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
        return Ok(label);
    }
    let result =
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html?editor=1".into()))
            .title(format!("{} — {}", editor.path, editor.spec.server.name))
            .inner_size(960.0, 720.0)
            .min_inner_size(560.0, 360.0)
            .build();
    match result {
        Ok(editor_window) => {
            let state = state.inner().clone();
            let cleanup_label = label.clone();
            editor_window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    state.files.editors.lock().unwrap().remove(&cleanup_label);
                }
            });
            Ok(label)
        }
        Err(error) => {
            state.files.editors.lock().unwrap().remove(&label);
            Err(AppError::new("window", error))
        }
    }
}

fn editor_for(state: &State, window: &WebviewWindow) -> AppResult<EditorHandle> {
    state
        .files
        .editors
        .lock()
        .unwrap()
        .get(window.label())
        .cloned()
        .ok_or_else(|| AppError::new("permission", "此窗口没有可编辑的远程文件"))
}

impl FileState {
    pub fn has_server_activity(&self, id: &str) -> bool {
        self.editors
            .lock()
            .unwrap()
            .values()
            .any(|editor| editor.spec.server.id == id)
            || self.downloads.lock().unwrap().iter().any(|item| {
                item.spec.server.id == id
                    && matches!(
                        item.task.lock().unwrap().status.as_str(),
                        "queued" | "scanning" | "running"
                    )
            })
    }
    pub fn unsaved_count(&self) -> usize {
        self.editors
            .lock()
            .unwrap()
            .values()
            .filter(|editor| editor.dirty.load(Ordering::SeqCst))
            .count()
    }
}

#[tauri::command]
pub fn editor_set_dirty(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    dirty: bool,
) -> AppResult<()> {
    editor_for(&state, &window)?
        .dirty
        .store(dirty, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn unsaved_editor_count(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<usize> {
    crate::main_only(&window)?;
    Ok(state.files.unsaved_count())
}

#[tauri::command]
pub async fn editor_load(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<EditorDocument> {
    let editor = editor_for(&state, &window)?;
    blocking(move || {
        let _gate = editor.gate.lock().unwrap();
        let session = ssh::establish_spec(&editor.spec)?;
        let (bytes, _) = text_bytes(&session.sftp()?, &editor.path)?;
        document(&editor, &bytes)
    })
    .await
}

#[tauri::command]
pub async fn editor_save(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    text: String,
    version: String,
    force: bool,
) -> AppResult<EditorDocument> {
    let editor = editor_for(&state, &window)?;
    validate_text(text.as_bytes())?;
    blocking(move || {
        let _gate = editor.gate.lock().unwrap();
        let session = ssh::establish_spec(&editor.spec)?;
        let sftp = session.sftp()?;
        let (before, stat) = text_bytes(&sftp, &editor.path)?;
        if !force && revision(&before) != version {
            return Err(AppError::new(
                "conflict",
                "远端文件已被修改，请重新加载或确认覆盖",
            ));
        }
        let bytes = edited_bytes(&before, &text)?;
        let temp = format!("{}.ssh-workspace-{}.tmp", editor.path, uuid::Uuid::new_v4());
        let result = (|| -> AppResult<()> {
            let mut file = sftp.open_mode(
                Path::new(&temp),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                0o600,
                OpenType::File,
            )?;
            file.write_all(&bytes)?;
            file.flush()?;
            file.setstat(FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: stat.perm.map(|mode| mode & 0o7777),
                atime: None,
                mtime: None,
            })?;
            file.close()?;
            // Check once more immediately before publishing the staged file.
            let (current, _) = text_bytes(&sftp, &editor.path)?;
            if revision(&current) != revision(&before) {
                return Err(AppError::new(
                    "conflict",
                    "保存期间远端文件发生变化，请重试",
                ));
            }
            atomic_remote_replace(&session, &sftp, &temp, &editor.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = sftp.unlink(Path::new(&temp));
        }
        result?;
        document(&editor, &bytes)
    })
    .await
}

fn packet_string(packet: &mut Vec<u8>, value: &[u8]) {
    packet.extend_from_slice(&(value.len() as u32).to_be_bytes());
    packet.extend_from_slice(value);
}

fn send_packet(channel: &mut ssh2::Channel, payload: &[u8]) -> AppResult<()> {
    channel.write_all(&(payload.len() as u32).to_be_bytes())?;
    channel.write_all(payload)?;
    channel.flush()?;
    Ok(())
}

fn receive_packet(channel: &mut ssh2::Channel) -> AppResult<Vec<u8>> {
    let mut length = [0u8; 4];
    channel.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 256 * 1024 {
        return Err(AppError::new("sftp", "SFTP 返回的数据包大小无效"));
    }
    let mut packet = vec![0; length];
    channel.read_exact(&mut packet)?;
    Ok(packet)
}

fn read_packet_string<'a>(data: &'a [u8], cursor: &mut usize) -> AppResult<&'a [u8]> {
    let length_bytes = data
        .get(*cursor..*cursor + 4)
        .ok_or_else(|| AppError::new("sftp", "SFTP 扩展响应不完整"))?;
    let length = u32::from_be_bytes(length_bytes.try_into().unwrap()) as usize;
    *cursor += 4;
    let value = data
        .get(*cursor..*cursor + length)
        .ok_or_else(|| AppError::new("sftp", "SFTP 扩展响应不完整"))?;
    *cursor += length;
    Ok(value)
}

fn atomic_remote_replace(
    session: &ssh2::Session,
    sftp: &Sftp,
    source: &str,
    target: &str,
) -> AppResult<()> {
    // ssh2::Sftp::rename sends v3 RENAME, which cannot replace a file on OpenSSH.
    // Negotiate the standard OpenSSH extension on a separate SFTP channel.
    let mut channel = session.channel_session()?;
    channel.subsystem("sftp")?;
    send_packet(&mut channel, &[1, 0, 0, 0, 3])?;
    let greeting = receive_packet(&mut channel)?;
    if greeting.len() < 5 || greeting[0] != 2 {
        return Err(AppError::new("sftp", "服务器未返回 SFTP 协议版本"));
    }
    let mut cursor = 5;
    let mut posix_rename = false;
    while cursor < greeting.len() {
        let name = read_packet_string(&greeting, &mut cursor)?;
        let version = read_packet_string(&greeting, &mut cursor)?;
        if name == b"posix-rename@openssh.com" && version == b"1" {
            posix_rename = true;
        }
    }
    let result = if posix_rename {
        let mut request = vec![200, 0, 0, 0, 1];
        packet_string(&mut request, b"posix-rename@openssh.com");
        packet_string(&mut request, source.as_bytes());
        packet_string(&mut request, target.as_bytes());
        send_packet(&mut channel, &request)?;
        let response = receive_packet(&mut channel)?;
        if response.len() < 9 || response[0] != 101 || response[1..5] != [0, 0, 0, 1] {
            return Err(AppError::new("sftp", "SFTP 原子保存响应无效"));
        }
        let status = u32::from_be_bytes(response[5..9].try_into().unwrap());
        if status == 0 {
            Ok(())
        } else {
            Err(AppError::new(
                "save",
                format!("服务器拒绝原子替换文件（SFTP 状态 {status}），请检查目录和文件权限"),
            ))
        }
    } else {
        sftp.rename(
            Path::new(source),
            Path::new(target),
            Some(RenameFlags::OVERWRITE | RenameFlags::ATOMIC | RenameFlags::NATIVE),
        )
        .map_err(|error| {
            AppError::new(
                "save",
                format!("服务器不支持安全替换远端文件，原文件未主动删除：{error}"),
            )
        })
    };
    let _ = channel.send_eof();
    let _ = channel.close();
    result
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    Overwrite,
    Skip,
    Rename,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    id: String,
    server_name: String,
    remote_path: String,
    local_path: String,
    is_dir: bool,
    conflict: ConflictPolicy,
    status: String,
    bytes_done: u64,
    bytes_total: u64,
    files_done: u64,
    files_total: u64,
    skipped: u64,
    error: Option<String>,
}

#[derive(Clone)]
struct DownloadHandle {
    spec: ssh::ConnectionSpec,
    task: Arc<Mutex<DownloadTask>>,
    cancel: Arc<AtomicBool>,
    destination: PathBuf,
}

fn download_window(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == "main" || window.label().starts_with("workspace-") {
        Ok(())
    } else {
        Err(AppError::new("permission", "请在会话窗口中管理下载"))
    }
}

#[tauri::command]
pub fn list_downloads(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Vec<DownloadTask>> {
    download_window(&window)?;
    Ok(state
        .files
        .downloads
        .lock()
        .unwrap()
        .iter()
        .map(|item| item.task.lock().unwrap().clone())
        .rev()
        .collect())
}

#[tauri::command]
pub async fn start_download(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    path: String,
    is_dir: bool,
    conflict: ConflictPolicy,
) -> AppResult<Option<String>> {
    let spec = state.spec(&session_id, window.label())?;
    remote_path(&path)?;
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download")
        .to_owned();
    safe_component(&name)?;
    let chosen_name = name.clone();
    let destination = blocking(move || {
        #[cfg(debug_assertions)]
        if let Some(directory) = std::env::var_os("SSH_WORKSPACE_QA_DOWNLOAD_DIR") {
            // QA is an explicit process-launch setting, never an IPC-supplied local path.
            let directory = fs::canonicalize(directory)?;
            let qa_root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.qa"))?;
            if !directory.starts_with(&qa_root) {
                return Err(AppError::new("path", "QA 下载目录必须位于项目 .qa 目录内"));
            }
            reject_links(&directory)?;
            return Ok(Some(directory.join(&chosen_name)));
        }
        Ok(if is_dir {
            rfd::FileDialog::new()
                .set_title("选择下载目录")
                .pick_folder()
                .map(|parent| parent.join(chosen_name))
        } else {
            rfd::FileDialog::new()
                .set_title("下载文件")
                .set_file_name(chosen_name)
                .save_file()
        })
    })
    .await?;
    let Some(destination) = destination else {
        return Ok(None);
    };
    reject_links(&destination)?;
    let id = uuid::Uuid::new_v4().to_string();
    let item = DownloadHandle {
        spec: spec.clone(),
        destination: destination.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
        task: Arc::new(Mutex::new(DownloadTask {
            id: id.clone(),
            server_name: spec.server.name,
            remote_path: path,
            local_path: destination.to_string_lossy().to_string(),
            is_dir,
            conflict,
            status: "queued".into(),
            bytes_done: 0,
            bytes_total: 0,
            files_done: 0,
            files_total: 0,
            skipped: 0,
            error: None,
        })),
    };
    state.files.downloads.lock().unwrap().push(item.clone());
    launch_download(state.inner().clone(), item);
    Ok(Some(id))
}

#[tauri::command]
pub fn cancel_download(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    download_window(&window)?;
    let item = find_download(&state, &id)?;
    item.cancel.store(true, Ordering::Relaxed);
    state.files.download_slot.notify_all();
    Ok(())
}

#[tauri::command]
pub fn retry_download(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    download_window(&window)?;
    let item = find_download(&state, &id)?;
    {
        let mut task = item.task.lock().unwrap();
        if task.status != "failed" && task.status != "cancelled" {
            return Err(AppError::new("download", "只有失败或已取消的任务可以重试"));
        }
        task.status = "queued".into();
        task.bytes_done = 0;
        task.bytes_total = 0;
        task.files_done = 0;
        task.files_total = 0;
        task.skipped = 0;
        task.error = None;
    }
    item.cancel.store(false, Ordering::Relaxed);
    launch_download(state.inner().clone(), item);
    Ok(())
}

fn find_download(state: &State, id: &str) -> AppResult<DownloadHandle> {
    state
        .files
        .downloads
        .lock()
        .unwrap()
        .iter()
        .find(|item| item.task.lock().unwrap().id == id)
        .cloned()
        .ok_or_else(|| AppError::new("download", "下载任务不存在"))
}

fn check_cancel(item: &DownloadHandle) -> AppResult<()> {
    if item.cancel.load(Ordering::Relaxed) {
        Err(AppError::new("cancelled", "下载已取消"))
    } else {
        Ok(())
    }
}

fn launch_download(state: Arc<State>, item: DownloadHandle) {
    std::thread::spawn(move || {
        let mut active = state.files.active_downloads.lock().unwrap();
        while *active >= 3 && !item.cancel.load(Ordering::Relaxed) {
            active = state
                .files
                .download_slot
                .wait_timeout(active, Duration::from_millis(250))
                .unwrap()
                .0;
        }
        *active += 1;
        drop(active);
        let result = run_download(&item);
        {
            let mut task = item.task.lock().unwrap();
            match result {
                Ok(()) => task.status = "completed".into(),
                Err(error) => {
                    task.status = if error.kind == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    task.error = Some(error.message);
                }
            }
        }
        *state.files.active_downloads.lock().unwrap() -= 1;
        state.files.download_slot.notify_one();
    });
}

struct PlannedFile {
    remote: String,
    relative: PathBuf,
    size: u64,
}
#[derive(Default)]
struct DownloadPlan {
    directories: Vec<PathBuf>,
    files: Vec<PlannedFile>,
    skipped: u64,
}

fn scan_directory(
    sftp: &Sftp,
    item: &DownloadHandle,
    remote: &str,
    relative: &Path,
    plan: &mut DownloadPlan,
    depth: usize,
) -> AppResult<()> {
    check_cancel(item)?;
    if depth > 128 || plan.files.len() + plan.directories.len() > 100_000 {
        return Err(AppError::new(
            "download",
            "目录层级或文件数量过多，请分批下载",
        ));
    }
    plan.directories.push(relative.to_path_buf());
    for (entry, _) in sftp.readdir(remote_path(remote)?)? {
        check_cancel(item)?;
        let name = entry
            .file_name()
            .and_then(|part| part.to_str())
            .ok_or_else(|| AppError::new("path", "远端文件名不是有效 UTF-8，无法下载"))?;
        if name == "." || name == ".." {
            continue;
        }
        safe_component(name)?;
        let child_remote = format!("{}/{}", remote.trim_end_matches('/'), name);
        let child_local = relative.join(name);
        let stat = sftp.lstat(Path::new(&child_remote))?;
        if stat.is_dir() {
            scan_directory(sftp, item, &child_remote, &child_local, plan, depth + 1)?;
        } else if stat.is_file() {
            plan.files.push(PlannedFile {
                remote: child_remote,
                relative: child_local,
                size: stat.size.unwrap_or(0),
            });
        } else {
            plan.skipped += 1;
        }
    }
    Ok(())
}

fn run_download(item: &DownloadHandle) -> AppResult<()> {
    check_cancel(item)?;
    item.task.lock().unwrap().status = "scanning".into();
    let session = ssh::establish_spec(&item.spec)?;
    let sftp = session.sftp()?;
    let task = item.task.lock().unwrap().clone();
    let stat = sftp.lstat(remote_path(&task.remote_path)?)?;
    let mut plan = DownloadPlan::default();
    if task.is_dir && stat.is_dir() {
        scan_directory(&sftp, item, &task.remote_path, Path::new(""), &mut plan, 0)?;
    } else if !task.is_dir && stat.is_file() {
        plan.files.push(PlannedFile {
            remote: task.remote_path.clone(),
            relative: PathBuf::new(),
            size: stat.size.unwrap_or(0),
        });
    } else {
        return Err(AppError::new(
            "fileType",
            "远端文件类型已变化，符号链接和特殊文件不支持下载",
        ));
    }
    let destination = if task.is_dir {
        resolve_destination(&item.destination, true, task.conflict)?
    } else {
        Some(item.destination.clone())
    };
    let Some(destination) = destination else {
        item.task.lock().unwrap().skipped = plan.files.len() as u64 + plan.skipped;
        return Ok(());
    };
    {
        let mut task = item.task.lock().unwrap();
        task.status = "running".into();
        task.bytes_total = plan.files.iter().map(|file| file.size).sum();
        task.files_total = plan.files.len() as u64;
        task.skipped = plan.skipped;
        task.local_path = destination.to_string_lossy().to_string();
    }
    for directory in plan.directories {
        check_cancel(item)?;
        safe_create_directory(&destination.join(directory))?;
    }
    for file in plan.files {
        check_cancel(item)?;
        let requested = if task.is_dir {
            destination.join(&file.relative)
        } else {
            destination.clone()
        };
        let Some(target) = resolve_destination(&requested, false, task.conflict)? else {
            let mut task = item.task.lock().unwrap();
            task.skipped += 1;
            task.bytes_done += file.size;
            task.files_done += 1;
            continue;
        };
        if !task.is_dir {
            item.task.lock().unwrap().local_path = target.to_string_lossy().to_string();
        }
        download_file(&sftp, item, &file, &target)?;
        item.task.lock().unwrap().files_done += 1;
    }
    Ok(())
}

fn safe_component(name: &str) -> AppResult<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || reserved
        || name
            .chars()
            .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
    {
        Err(AppError::new(
            "path",
            format!("文件名无法安全保存到本地：{name}"),
        ))
    } else {
        Ok(())
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

fn reject_links(path: &Path) -> AppResult<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(AppError::new(
                    "path",
                    "下载目标包含符号链接或目录联接，请选择普通目录",
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn safe_create_directory(path: &Path) -> AppResult<()> {
    reject_links(path)?;
    fs::create_dir_all(path)?;
    reject_links(path)
}

fn resolve_destination(
    path: &Path,
    is_dir: bool,
    policy: ConflictPolicy,
) -> AppResult<Option<PathBuf>> {
    reject_links(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Some(path.to_path_buf()))
        }
        Err(error) => return Err(error.into()),
    };
    match policy {
        ConflictPolicy::Skip => Ok(None),
        ConflictPolicy::Overwrite if metadata.is_dir() == is_dir => Ok(Some(path.to_path_buf())),
        ConflictPolicy::Overwrite => Err(AppError::new(
            "conflict",
            "本地同名文件与目录类型冲突，请选择重命名",
        )),
        ConflictPolicy::Rename => {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AppError::new("path", "下载目标文件名无效"))?;
            let (stem, extension) = if is_dir {
                (name, String::new())
            } else {
                match name.rsplit_once('.') {
                    Some((base, ext)) if !base.is_empty() => (base, format!(".{ext}")),
                    _ => (name, String::new()),
                }
            };
            for suffix in 1..100_000 {
                let candidate = path.with_file_name(format!("{stem} ({suffix}){extension}"));
                if fs::symlink_metadata(&candidate)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                {
                    return Ok(Some(candidate));
                }
            }
            Err(AppError::new("conflict", "无法生成未使用的下载文件名"))
        }
    }
}

fn download_file(
    sftp: &Sftp,
    item: &DownloadHandle,
    file: &PlannedFile,
    target: &Path,
) -> AppResult<()> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::new("path", "下载目标缺少父目录"))?;
    safe_create_directory(parent)?;
    let output = tempfile::Builder::new()
        .prefix(".ssh-workspace-")
        .suffix(".part")
        .tempfile_in(parent)?;
    let result = (|| -> AppResult<()> {
        let mut output = output;
        // Recheck the remote type after scanning, so a replaced link is not followed.
        if !sftp.lstat(Path::new(&file.remote))?.is_file() {
            return Err(AppError::new("fileType", "下载期间远端文件类型已变化"));
        }
        let mut input = sftp.open(Path::new(&file.remote))?;
        let mut buffer = [0u8; 65536];
        loop {
            check_cancel(item)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
            item.task.lock().unwrap().bytes_done += count as u64;
        }
        output.as_file().sync_all()?;
        check_cancel(item)?;
        reject_links(target)?;
        let policy = item.task.lock().unwrap().conflict;
        if policy == ConflictPolicy::Overwrite {
            // std::fs::rename atomically replaces regular files on supported desktop OSes.
            output
                .persist(target)
                .map_err(|error| AppError::from(error.error))?;
        } else {
            // tempfile uses native no-replace rename on Windows, Linux and macOS.
            let mut candidate = target.to_path_buf();
            loop {
                reject_links(&candidate)?;
                match output.persist_noclobber(&candidate) {
                    Ok(_) => {
                        let mut task = item.task.lock().unwrap();
                        if !task.is_dir {
                            task.local_path = candidate.to_string_lossy().to_string();
                        }
                        break;
                    }
                    Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                        output = error.file;
                        if policy == ConflictPolicy::Skip {
                            item.task.lock().unwrap().skipped += 1;
                            break;
                        }
                        candidate = resolve_destination(target, false, policy)?
                            .ok_or_else(|| AppError::new("conflict", "下载目标已存在"))?;
                    }
                    Err(error) => return Err(error.error.into()),
                }
            }
        }
        Ok(())
    })();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_validation_and_encoding_preserve_bom_and_crlf() {
        assert!(validate_text(b"a\0b").is_err());
        assert!(validate_text(&[0xff, 0xfe]).is_err());
        assert!(validate_text(&vec![b'a'; TEXT_LIMIT + 1]).is_err());
        let original = b"\xef\xbb\xbfa\r\nb\r\n";
        assert_eq!(
            edited_bytes(original, "new\nline\n").unwrap(),
            b"\xef\xbb\xbfnew\r\nline\r\n"
        );
        assert_ne!(revision(original), revision(b"a\r\nb\r\n"));
    }

    #[test]
    fn path_components_cannot_escape_or_target_windows_devices() {
        for name in [
            "..", ".", "a/b", "a\\b", "C:abc", "CON", "nul.txt", "COM1", "x.", "x ",
        ] {
            assert!(safe_component(name).is_err(), "{name}");
        }
        for name in [".env", "配置文件.conf", "hello world", "a-b_1.txt"] {
            assert!(safe_component(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn conflict_policies_preserve_existing_files_and_empty_directories() {
        let root = tempfile::tempdir().unwrap();
        // macOS temp paths may start with /var, a symlink to /private/var.
        let directory = fs::canonicalize(root.path()).unwrap();
        let path = directory.join("a.txt");
        fs::write(&path, "old").unwrap();
        assert!(resolve_destination(&path, false, ConflictPolicy::Skip)
            .unwrap()
            .is_none());
        assert_eq!(
            resolve_destination(&path, false, ConflictPolicy::Rename)
                .unwrap()
                .unwrap(),
            directory.join("a (1).txt")
        );
        assert!(resolve_destination(&path, true, ConflictPolicy::Overwrite).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        let empty = directory.join("a/b/empty");
        safe_create_directory(&empty).unwrap();
        assert!(empty.is_dir());
    }

    #[test]
    fn sftp_extension_parser_rejects_truncated_strings() {
        let mut bytes = Vec::new();
        packet_string(&mut bytes, b"posix-rename@openssh.com");
        packet_string(&mut bytes, b"1");
        let mut cursor = 0;
        assert_eq!(
            read_packet_string(&bytes, &mut cursor).unwrap(),
            b"posix-rename@openssh.com"
        );
        assert_eq!(read_packet_string(&bytes, &mut cursor).unwrap(), b"1");
        assert_eq!(cursor, bytes.len());
        assert!(read_packet_string(&bytes, &mut cursor).is_err());
        assert!(read_packet_string(&[0, 0, 1, 0, b'a'], &mut 0).is_err());
    }

    #[test]
    fn downloaded_file_is_published_without_clobbering_existing_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("download.txt");
        fs::write(&target, "existing").unwrap();
        let mut staged = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        staged.write_all(b"downloaded").unwrap();
        let error = staged.persist_noclobber(&target).unwrap_err();
        assert_eq!(error.error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"existing");
        let next = root.path().join("download (1).txt");
        error.file.persist_noclobber(&next).unwrap();
        assert_eq!(fs::read(next).unwrap(), b"downloaded");
    }

    #[cfg(unix)]
    #[test]
    fn reject_existing_symlinks_in_destination_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let directory = fs::canonicalize(root.path()).unwrap();
        assert!(reject_links(&directory.join("file")).is_ok());
        std::os::unix::fs::symlink(&directory, directory.join("link")).unwrap();
        assert!(reject_links(&directory.join("link/file")).is_err());
    }
}
