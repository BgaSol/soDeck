use crate::connection::{self, SharedContext};
use crate::ssh_transport::Cancellation;
use crate::{
    blocking,
    model::{AppError, AppResult},
    State,
};
use serde::{Deserialize, Serialize};
use ssh2::Sftp;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{State as TauriState, WebviewWindow};
fn remote_path(path: &str) -> AppResult<&Path> {
    crate::remote_path::absolute(path)?;
    Ok(Path::new(path))
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    Overwrite,
    Skip,
    Rename,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub(super) id: String,
    pub(super) run_id: String,
    #[serde(flatten)]
    pub(super) presentation: super::model::TaskPresentation,
    pub(super) server_name: String,
    pub(super) remote_path: String,
    pub(super) local_path: String,
    pub(super) is_dir: bool,
    pub(super) conflict: ConflictPolicy,
    pub(super) status: String,
    pub(super) bytes_done: u64,
    pub(super) bytes_total: u64,
    pub(super) files_done: u64,
    pub(super) files_total: u64,
    pub(super) skipped: u64,
    pub(super) error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DownloadReceipt {
    pub(super) identity: String,
    pub(super) destination: PathBuf,
    pub(super) task: DownloadTask,
}
fn persist(state: &State, item: &DownloadHandle) -> AppResult<()> {
    let task = item.task.lock().unwrap().clone();
    crate::diagnostics::state_event("download", &task.id, &task.status);
    let receipt = DownloadReceipt {
        identity: item.spec.identity().into(),
        destination: item.destination.clone(),
        task,
    };
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .save("download", &receipt.task.id, &receipt)?;
    state.file_tasks.emit(&receipt.task.id);
    Ok(())
}
pub(super) fn view(t: &DownloadTask) -> super::TaskView {
    super::TaskView {
        id: t.id.clone(),
        run_id: t.run_id.clone(),
        presentation: t.presentation.clone(),
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
        warnings: vec![],
        cleanup_pending: false,
        items: vec![],
        direct_address: None,
        temporary_paths: vec![],
        can_resume: false,
    }
}

#[derive(Clone)]
pub(super) struct DownloadHandle {
    pub(super) spec: SharedContext,
    pub(super) task: Arc<Mutex<DownloadTask>>,
    pub(super) cancel: Cancellation,
    pub(super) destination: PathBuf,
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
        .transfers
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
    let _gate = state.configuration.lock().unwrap();
    state.check_context(&spec)?;
    reject_links(&destination)?;
    let id = uuid::Uuid::new_v4().to_string();
    let item = DownloadHandle {
        spec: spec.clone(),
        destination: destination.clone(),
        cancel: Cancellation::default(),
        task: Arc::new(Mutex::new(DownloadTask {
            id: id.clone(),
            run_id: uuid::Uuid::new_v4().to_string(),
            presentation: super::model::TaskPresentation::new(
                state.transfers.next_order(),
                vec![path.clone()],
            ),
            server_name: spec.target().name.clone(),
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
    persist(&state, &item)?;
    state.transfers.downloads.lock().unwrap().push(item.clone());
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
    let mut task = item.task.lock().unwrap();
    if super::model::active(&task.status) {
        task.status = "cancelling".into();
        item.cancel.cancel();
    }
    state.transfers.slot.notify_all();
    Ok(())
}

#[tauri::command]
pub fn retry_download(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    download_window(&window)?;
    let _action = state.file_tasks.actions.lock().unwrap();
    let _gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    let item = find_download(&state, &id)?;
    state.check_context(&item.spec)?;
    reject_links(&item.destination)?;
    remote_path(&item.task.lock().unwrap().remote_path)?;
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .restore_visibility(&id)?;
    let previous = item.task.lock().unwrap().clone();
    {
        let mut task = item.task.lock().unwrap();
        if !matches!(task.status.as_str(), "failed" | "cancelled" | "interrupted") {
            return Err(AppError::new("download", "只有失败或已取消的任务可以重试"));
        }
        item.cancel.reset()?;
        let previous = task.clone();
        task.presentation.restart(
            state.transfers.next_order(),
            previous.run_id,
            previous.status.as_str().into(),
            previous.error,
        );
        task.run_id = uuid::Uuid::new_v4().to_string();
        task.status = "queued".into();
        task.bytes_done = 0;
        task.bytes_total = 0;
        task.files_done = 0;
        task.files_total = 0;
        task.skipped = 0;
        task.error = None;
    }
    if let Err(e) = persist(&state, &item) {
        *item.task.lock().unwrap() = previous;
        return Err(e);
    }
    launch_download(state.inner().clone(), item);
    Ok(())
}

fn find_download(state: &State, id: &str) -> AppResult<DownloadHandle> {
    if let Some(item) = state
        .transfers
        .downloads
        .lock()
        .unwrap()
        .iter()
        .find(|item| item.task.lock().unwrap().id == id)
        .cloned()
    {
        return Ok(item);
    }
    let receipt = state
        .transfers
        .recovered_downloads
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.task.id == id)
        .cloned()
        .ok_or_else(|| AppError::new("download", "下载任务不存在"))?;
    let spec = state.connected_context(&receipt.identity)?;
    let item = DownloadHandle {
        spec,
        task: Arc::new(Mutex::new(receipt.task)),
        cancel: Cancellation::default(),
        destination: receipt.destination,
    };
    let mut downloads = state.transfers.downloads.lock().unwrap();
    if let Some(existing) = downloads.iter().find(|d| d.task.lock().unwrap().id == id) {
        return Ok(existing.clone());
    }
    downloads.push(item.clone());
    state
        .transfers
        .recovered_downloads
        .lock()
        .unwrap()
        .retain(|r| r.task.id != id);
    Ok(item)
}

fn check_cancel(item: &DownloadHandle) -> AppResult<()> {
    if item.cancel.is_cancelled() {
        Err(AppError::new("cancelled", "下载已取消"))
    } else {
        Ok(())
    }
}

pub(super) fn launch_download(state: Arc<State>, item: DownloadHandle) {
    std::thread::spawn(move || {
        let mut active = state.transfers.active.lock().unwrap();
        while *active >= 3 && !item.cancel.is_cancelled() {
            active = state
                .transfers
                .slot
                .wait_timeout(active, Duration::from_millis(250))
                .unwrap()
                .0;
        }
        *active += 1;
        drop(active);
        let result = run_download(&state, &item);
        {
            let mut task = item.task.lock().unwrap();
            match result {
                Ok(()) => task.status = "completed".into(),
                Err(error) => {
                    crate::diagnostics::task_failure("download", &task.id, &error);
                    task.status = if item.cancel.is_cancelled() || error.kind == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    task.error = Some(error.message);
                }
            }
        }
        item.task.lock().unwrap().presentation.finished_at = Some(crate::file_tasks::now());
        if let Err(e) = persist(&state, &item) {
            let mut task = item.task.lock().unwrap();
            task.status = "failed".into();
            task.error = Some(format!("下载结果保存失败：{}", e.message));
        }
        *state.transfers.active.lock().unwrap() -= 1;
        state.transfers.slot.notify_one();
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

fn run_download(state: &State, item: &DownloadHandle) -> AppResult<()> {
    check_cancel(item)?;
    item.task.lock().unwrap().status = "scanning".into();
    persist(state, item)?;
    let connection = connection::connect_verified(
        &item.spec,
        &item.cancel,
        crate::traffic::TrafficPurpose::Transfer,
    )?;
    let session = connection.session();
    let sftp = session.sftp()?;
    let task = item.task.lock().unwrap().clone();
    let resolved = sftp.realpath(remote_path(&task.remote_path)?)?;
    let resolved = resolved
        .to_str()
        .ok_or_else(|| AppError::new("path", "远端路径不是 UTF-8"))?;
    let claims = [task.remote_path.as_str(), resolved].map(|path| super::scheduler::Claim {
        key: item.spec.target_public_key().into(),
        path: path.into(),
        write: false,
    });
    let _paths = state
        .transfers
        .paths
        .acquire_many(&claims, &task.id, |conflict| {
            check_cancel(item)?;
            if let Some((path, task)) = conflict {
                return Err(AppError::new(
                    "busy",
                    format!("任务 {task} 正在修改 {path}，请等待结束后重新下载"),
                ));
            }
            Ok(())
        })?;
    if sftp.realpath(remote_path(&task.remote_path)?)?.to_str() != Some(resolved) {
        return Err(AppError::new("changed", "下载来源路径已变化，请重新选择"));
    }
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
    persist(state, item)?;
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
        persist(state, item)?;
    }
    Ok(())
}

pub(super) fn safe_component(name: &str) -> AppResult<()> {
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

pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
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

pub(super) fn reject_links(path: &Path) -> AppResult<()> {
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
pub(crate) mod tests {
    use super::*;
    use std::time::Instant;

    pub(crate) fn exercise_jump_downloads(
        state: Arc<State>,
        context: SharedContext,
        fixture: &Path,
    ) {
        let output = tempfile::tempdir().unwrap();
        let destination = fs::canonicalize(output.path()).unwrap();
        let make = |name: &str, remote: &str| DownloadHandle {
            spec: context.clone(),
            destination: destination.join(name),
            cancel: Cancellation::default(),
            task: Arc::new(Mutex::new(DownloadTask {
                id: name.into(),
                run_id: uuid::Uuid::new_v4().to_string(),
                presentation: super::super::model::TaskPresentation::default(),
                server_name: "target".into(),
                remote_path: remote.into(),
                local_path: destination.join(name).display().to_string(),
                is_dir: false,
                conflict: ConflictPolicy::Overwrite,
                status: "queued".into(),
                bytes_done: 0,
                bytes_total: 0,
                files_done: 0,
                files_total: 0,
                skipped: 0,
                error: None,
            })),
        };
        let item = make("readme.txt", "/readme.txt");
        state.transfers.downloads.lock().unwrap().push(item.clone());
        launch_download(state.clone(), item.clone());
        state.transfers.wait_transfers().unwrap();
        assert_eq!(item.task.lock().unwrap().status, "completed");
        assert_eq!(
            fs::read(&item.destination).unwrap(),
            fs::read(fixture.join("remote/readme.txt")).unwrap()
        );

        fs::write(fixture.join("sftp-read-delay"), "0.05").unwrap();
        let items: Vec<_> = (0..5)
            .map(|i| make(&format!("slow-{i}.bin"), "/download/slow.bin"))
            .collect();
        for item in &items {
            state.transfers.downloads.lock().unwrap().push(item.clone());
            launch_download(state.clone(), item.clone());
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        while !items.iter().any(|i| i.task.lock().unwrap().bytes_done > 0) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        let start = Instant::now();
        state.transfers.cancel_all();
        state.transfers.wait_transfers().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        for item in &items {
            assert_eq!(item.task.lock().unwrap().status, "cancelled");
            assert!(!item.destination.exists());
        }
        assert_eq!(
            fs::read_dir(&destination).unwrap().count(),
            1,
            "cancelled downloads leave no .part files"
        );
        fs::remove_file(fixture.join("sftp-read-delay")).unwrap();
        let retry = items[0].clone();
        retry.cancel.reset().unwrap();
        retry.task.lock().unwrap().status = "queued".into();
        launch_download(state.clone(), retry.clone());
        state.transfers.wait_transfers().unwrap();
        assert_eq!(retry.task.lock().unwrap().status, "completed");
        assert_eq!(
            fs::read(retry.destination).unwrap(),
            fs::read(fixture.join("remote/download/slow.bin")).unwrap()
        );
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
    #[test]
    fn download_history_recovers_without_credentials_or_automatic_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.sqlite");
        let task: DownloadTask = serde_json::from_value(serde_json::json!({
            "id":"download", "runId":"attempt", "sortOrder":42,
            "serverName":"server", "remotePath":"/data/file", "localPath":"/local/file",
            "isDir":false, "conflict":"rename", "status":"running",
            "bytesDone":64,"bytesTotal":128,"filesDone":0,"filesTotal":1,"skipped":0,"error":null
        }))
        .unwrap();
        let receipt = DownloadReceipt {
            identity: "verified-route".into(),
            destination: dir.path().join("file"),
            task,
        };
        {
            let history = crate::file_tasks::FileTaskState::open(&path).unwrap();
            history
                .store
                .lock()
                .unwrap()
                .save("download", "download", &receipt)
                .unwrap();
        }
        let history = crate::file_tasks::FileTaskState::open(&path).unwrap();
        let transfers = super::super::TransferState::default();
        transfers.restore_downloads(&history).unwrap();
        assert!(transfers.downloads.lock().unwrap().is_empty());
        let saved = transfers.recovered_downloads.lock().unwrap();
        assert_eq!(saved[0].task.status, "interrupted");
        assert_eq!(saved[0].task.bytes_done, 64);
        let json = serde_json::to_string(&saved[0]).unwrap();
        assert!(!json.contains("password") && !json.contains("credentials"));
        assert!(transfers.next_order() > 42);
        assert!(!dir.path().join("file").exists());
        drop(saved);
        let mut state = crate::connection::tests::state();
        Arc::get_mut(&mut state).unwrap().file_tasks = history;
        Arc::get_mut(&mut state).unwrap().transfers = transfers;
        assert_eq!(crate::file_tasks::snapshot(&state).unwrap().len(), 1);
        let receipts = state
            .file_tasks
            .store
            .lock()
            .unwrap()
            .load::<DownloadReceipt>("download")
            .unwrap();
        assert_eq!(
            receipts.len(),
            1,
            "listing tasks cannot replace download recovery metadata"
        );
    }
}
