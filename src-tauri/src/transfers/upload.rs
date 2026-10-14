use super::{
    direct::Controller,
    download::ConflictPolicy,
    model::{Source, WriteHandle},
    planner::{self, Entry},
};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path,
    remote_write::{self, Stage},
    State,
};
use ssh2::{OpenFlags, OpenType};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};

fn upload_file(item: &WriteHandle, sftp: &ssh2::Sftp, entry: &Entry, stage: &str) -> AppResult<()> {
    let path = Path::new(&entry.source);
    let metadata = fs::symlink_metadata(path)?;
    if super::download::is_link(&metadata) || !metadata.is_file() {
        return Err(AppError::new("fileType", "本地文件类型已经变化"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut input = options.open(path)?;
    let before = input.metadata()?;
    let modified = before
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    if !before.is_file() || before.len() != entry.size || modified != entry.modified {
        return Err(AppError::new(
            "changed",
            "扫描后本地文件发生变化，请重新创建任务",
        ));
    }
    let mut output = sftp.open_mode(
        Path::new(stage),
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
        0o644,
        OpenType::File,
    )?;
    let mut buffer = [0u8; 65536];
    let mut written = 0;
    loop {
        item.check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        written += count as u64;
        if written > entry.size {
            return Err(AppError::new(
                "changed",
                "上传期间本地文件增大，未提交目标文件",
            ));
        }
        output.write_all(&buffer[..count])?;
        item.task.lock().unwrap().bytes_done += count as u64;
    }
    output.close()?;
    let after = input.metadata()?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err(AppError::new(
            "changed",
            "上传期间本地文件发生变化，未提交目标文件",
        ));
    }
    Ok(())
}

pub fn run(state: &State, app: Option<&tauri::AppHandle>, item: &WriteHandle) -> AppResult<()> {
    item.check()?;
    item.task.lock().unwrap().status = "checking".into();
    let target = connection::connect_verified(&item.target, &item.cancel)?;
    let sftp = target.session().sftp()?;
    let requested = item.task.lock().unwrap().target_path.clone();
    let root = sftp
        .realpath(Path::new(&requested))?
        .to_str()
        .ok_or_else(|| AppError::new("path", "目标目录不是 UTF-8"))?
        .to_owned();
    if !sftp.stat(Path::new(&root))?.is_dir() {
        return Err(AppError::new("fileType", "目标不是目录"));
    }
    item.task.lock().unwrap().target_path = root.clone();
    // Hold the target's write lane across planning and commit. Other target
    // servers are independent, and registry locks are never held over I/O.
    let lane = state.transfers.write_lane(item.target.identity());
    let _writer = loop {
        item.check()?;
        match lane.try_lock() {
            Ok(writer) => break writer,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            Err(std::sync::TryLockError::Poisoned(error)) => {
                return Err(AppError::new("internal", error))
            }
        }
    };
    let mut controller = if matches!(item.source, Source::Remote { .. }) {
        Some(Controller::start(state, item)?)
    } else {
        None
    };
    let result = (|| {
        if let Some(controller) = controller.as_mut() {
            let stage = Stage::create(&sftp, &root)?;
            item.receipt
                .lock()
                .unwrap()
                .stage_dirs
                .push(stage.directory.clone());
            state.transfers.persist(item)?;
            let probe = Entry {
                source: format!("{}/probe", item.receipt.lock().unwrap().remote_root),
                parts: vec![],
                directory: false,
                size: 0,
                modified: None,
                target: None,
                outcome: "pending".into(),
            };
            controller.copy(item, usize::MAX, &probe, &stage.path)?;
            stage.cleanup(&sftp)?;
            item.receipt
                .lock()
                .unwrap()
                .stage_dirs
                .retain(|p| p != &stage.directory);
        }
        item.task.lock().unwrap().status = "scanning".into();
        let saved = item.plan.lock().unwrap().take();
        let mut plan = match saved {
            Some(plan) => plan,
            None => planner::scan(item)?,
        };
        {
            let mut task = item.task.lock().unwrap();
            task.files_total = plan.entries.iter().filter(|e| !e.directory).count() as u64;
            task.bytes_total = plan.entries.iter().map(|e| e.size).sum();
            task.files_done = plan
                .entries
                .iter()
                .filter(|e| !e.directory && matches!(e.outcome.as_str(), "completed" | "skipped"))
                .count() as u64;
            task.bytes_done = plan
                .entries
                .iter()
                .filter(|e| e.outcome == "completed")
                .map(|e| e.size)
                .sum();
            task.skipped = plan.skipped.len() as u64
                + plan
                    .entries
                    .iter()
                    .filter(|e| e.outcome == "skipped")
                    .count() as u64;
            task.status = "running".into();
            task.warnings = plan
                .skipped
                .iter()
                .take(100)
                .map(|p| format!("跳过符号链接或特殊文件：{p}"))
                .collect();
        }
        let policy = item.task.lock().unwrap().conflict;
        let mut directories: HashMap<Vec<String>, Option<String>> = HashMap::new();
        directories.insert(vec![], Some(root.clone()));
        let mut completed_bytes = item.task.lock().unwrap().bytes_done;
        let transfer_result = (|| {
            for index in 0..plan.entries.len() {
                item.check()?;
                let entry = plan.entries[index].clone();
                let parent_parts = &entry.parts[..entry.parts.len() - 1];
                let parent = directories.get(parent_parts).cloned().flatten();
                if parent.is_none() {
                    if entry.outcome != "skipped" {
                        plan.entries[index].outcome = "skipped".into();
                        item.task.lock().unwrap().skipped += 1;
                        if !entry.directory {
                            item.task.lock().unwrap().files_done += 1;
                        }
                    }
                    if entry.directory {
                        directories.insert(entry.parts.clone(), None);
                    }
                    continue;
                }
                let requested = entry.target.clone().unwrap_or(remote_path::join(
                    parent.as_ref().unwrap(),
                    entry.parts.last().unwrap(),
                )?);
                if entry.directory {
                    let destination = if entry.outcome == "completed" {
                        if !remote_write::metadata(&sftp, &requested)?.is_some_and(|s| s.is_dir()) {
                            return Err(AppError::new(
                                "changed",
                                format!("重试时目标目录已变化：{requested}"),
                            ));
                        }
                        Some(requested)
                    } else if entry.outcome == "skipped" {
                        None
                    } else {
                        let result = remote_write::directory_destination(
                            &sftp,
                            &requested,
                            policy,
                            |path| {
                                item.receipt.lock().unwrap().pending_commit = Some(path.into());
                                state.transfers.persist(item)
                            },
                        );
                        if !result.as_ref().is_err_and(|e| e.kind == "unknown") {
                            item.receipt.lock().unwrap().pending_commit = None;
                        }
                        result?
                    };
                    plan.entries[index].outcome = if destination.is_some() {
                        "completed"
                    } else {
                        "skipped"
                    }
                    .into();
                    plan.entries[index].target = destination.clone();
                    if destination.is_none() && entry.outcome != "skipped" {
                        item.task.lock().unwrap().skipped += 1;
                    }
                    directories.insert(entry.parts.clone(), destination);
                    state.transfers.persist(item)?;
                    continue;
                }
                if entry.outcome == "completed" {
                    if !remote_write::metadata(&sftp, &requested)?
                        .is_some_and(|s| s.is_file() && s.size == Some(entry.size))
                    {
                        return Err(AppError::new(
                            "changed",
                            format!("已完成的目标文件发生变化：{requested}"),
                        ));
                    }
                    continue;
                }
                if entry.outcome == "skipped" {
                    continue;
                }
                if policy == ConflictPolicy::Skip
                    && remote_write::metadata(&sftp, &requested)?.is_some()
                {
                    plan.entries[index].outcome = "skipped".into();
                    let mut task = item.task.lock().unwrap();
                    task.skipped += 1;
                    task.files_done += 1;
                    continue;
                }
                plan.entries[index].target = Some(requested.clone());
                let stage = Stage::create(&sftp, remote_path::parent(&requested))?;
                item.receipt
                    .lock()
                    .unwrap()
                    .stage_dirs
                    .push(stage.directory.clone());
                state.transfers.persist(item)?;
                item.task.lock().unwrap().status = "running".into();
                item.task.lock().unwrap().bytes_done = completed_bytes;
                let result = (|| {
                    if let Some(controller) = controller.as_mut() {
                        controller.copy(item, index, &entry, &stage.path)?;
                    } else {
                        upload_file(item, &sftp, &entry, &stage.path)?;
                    }
                    item.check()?;
                    if !remote_write::metadata(&sftp, &stage.path)?
                        .is_some_and(|s| s.is_file() && s.size == Some(entry.size))
                    {
                        return Err(AppError::new("changed", "临时文件大小或类型不符合传输计划"));
                    }
                    item.task.lock().unwrap().status = "committing".into();
                    let destination = remote_write::publish(
                        target.session(),
                        &sftp,
                        &stage.path,
                        &requested,
                        policy,
                        |path| {
                            item.receipt.lock().unwrap().pending_commit = Some(path.into());
                            state.transfers.persist(item)
                        },
                    );
                    if !destination.as_ref().is_err_and(|e| e.kind == "unknown") {
                        item.receipt.lock().unwrap().pending_commit = None;
                    }
                    let destination = destination?;
                    plan.entries[index].outcome = if destination.is_some() {
                        "completed"
                    } else {
                        "skipped"
                    }
                    .into();
                    plan.entries[index].target = destination;
                    {
                        let mut task = item.task.lock().unwrap();
                        task.files_done += 1;
                        task.bytes_done = completed_bytes
                            + if plan.entries[index].outcome == "completed" {
                                entry.size
                            } else {
                                0
                            };
                        if plan.entries[index].outcome == "skipped" {
                            task.skipped += 1;
                        }
                        completed_bytes = task.bytes_done;
                    }
                    Ok(())
                })();
                if result
                    .as_ref()
                    .is_err_and(|e: &AppError| e.kind == "unknown")
                {
                    return result;
                }
                match stage.cleanup(&sftp) {
                    Ok(()) => item
                        .receipt
                        .lock()
                        .unwrap()
                        .stage_dirs
                        .retain(|p| p != &stage.directory),
                    Err(e) => {
                        item.warn(format!(
                            "临时文件清理待完成：{}（{}）",
                            stage.directory, e.message
                        ));
                        state.transfers.persist(item)?;
                        result?;
                        return Err(AppError::new(
                            "cleanup",
                            "临时资源清理失败，任务已暂停写入；请先核对并清理后重试",
                        ));
                    }
                }
                state.transfers.persist(item)?;
                result?;
                if let Some(app) = app {
                    super::directory_changed(
                        app,
                        item.target.identity(),
                        remote_path::parent(&requested),
                    );
                }
            }
            Ok(())
        })();
        *item.plan.lock().unwrap() = Some(plan);
        transfer_result
    })();
    let mut result = result;
    if let Some(controller) = controller.as_mut() {
        let stopping = if result.is_ok() {
            controller.finish(item)
        } else {
            controller.stop(item)
        };
        if let Err(error) = stopping {
            if error.kind == "unknown" || result.is_ok() {
                result = Err(error);
            }
        }
        if controller.confirmed_stopped() {
            item.receipt.lock().unwrap().remote_stopped = true;
            if let Err(error) = state.transfers.persist(item) {
                item.warn(format!("停止确认保存失败：{}", error.message));
            }
        }
    }
    drop(controller);
    // Do not clean files while the source may still have them open.
    if !result.as_ref().is_err_and(|e| e.kind == "unknown") {
        if let Err(e) = super::cleanup(item) {
            item.warn(format!("临时资源清理失败：{}", e.message));
        }
    }
    if let Some(app) = app {
        super::directory_changed(app, item.target.identity(), &root);
    }
    result
}
